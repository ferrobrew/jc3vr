use anyhow::{Context, Result, ensure};
use serde::Serialize;
use windows::{
    Win32::Graphics::{
        Direct3D::{
            D3D_SRV_DIMENSION_BUFFER, D3D_SRV_DIMENSION_BUFFEREX, D3D_SRV_DIMENSION_TEXTURE2DARRAY,
        },
        Direct3D11::*,
        Dxgi::Common::{DXGI_FORMAT_R16_UINT, DXGI_FORMAT_R32_UINT},
    },
    core::Interface,
};

use crate::screenshot::harness::{
    bound_buffer::{ShaderStage, read_bound_constant_buffer},
    stages::velocity::BufferPrefix,
};

#[derive(Clone, Debug, Serialize)]
pub(super) struct ImpostorInputs {
    view_dimension: i32,
    first_element: u32,
    elements: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) raster: Option<GroupRaster>,
    cards: Vec<[u32; 3]>,
    indices: Vec<u32>,
    pub(super) cb: Vec<BufferPrefix>,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct GroupRaster {
    pub(super) scissor_enabled: bool,
    scissors: Vec<[i32; 4]>,
    cull: i32,
    front_ccw: bool,
    atlas: Vec<AtlasView>,
    sampler: [i32; 4],
    lod: [f32; 3],
}

#[derive(Clone, Debug, Serialize)]
struct AtlasView {
    resource: usize,
    format: i32,
    extent: [u32; 4],
    range: [u32; 4],
}

pub(super) fn impostor_inputs(
    ctx: &ID3D11DeviceContext,
    index_count: u32,
) -> Result<ImpostorInputs> {
    ensure!(
        matches!(index_count, 144 | 3300 | 1392 | 4872 | 5946 | 6534 | 7542),
        "impostor inputs: unexpected count"
    );
    let mut cb = Vec::new();
    for (stage, slot, rows) in [
        (ShaderStage::Vertex, 0, 10),
        (ShaderStage::Vertex, 1, 4),
        (ShaderStage::Vertex, 3, 640),
        (ShaderStage::Vertex, 13, 18),
        (ShaderStage::Pixel, 0, 9),
        (ShaderStage::Pixel, 1, 6),
        (ShaderStage::Pixel, 4, 1),
    ] {
        let r = read_bound_constant_buffer(ctx, stage, slot, rows)?;
        ensure!(
            (slot == 13 && r.values.is_none())
                || r.range_known
                    && (slot == 13
                        || r.values
                            .as_ref()
                            .is_some_and(|v| v.len() == rows as usize * 4)),
            "impostor inputs: missing {} b{} range",
            stage.name(),
            slot
        );
        cb.push(BufferPrefix {
            stage: stage.name(),
            slot,
            first: r.first_constant,
            count: r.num_constants,
            range_known: r.range_known,
            bits: r.values.map(|v| v.into_iter().map(f32::to_bits).collect()),
        });
    }
    let mut view = [None];
    let mut ib = None;
    let mut format = DXGI_FORMAT_R16_UINT;
    let mut offset = 0;
    unsafe {
        ctx.VSGetShaderResources(0, Some(&mut view));
        ctx.IAGetIndexBuffer(Some(&mut ib), Some(&mut format), Some(&mut offset));
    }
    let view = view[0]
        .as_ref()
        .context("impostor inputs: missing card SRV")?;
    let ib = ib.context("impostor inputs: missing index buffer")?;
    let width = match format {
        DXGI_FORMAT_R16_UINT => 2,
        DXGI_FORMAT_R32_UINT => 4,
        _ => anyhow::bail!("impostor inputs: unsupported index format"),
    };
    let bytes = buffer_slice(ctx, &ib, offset, index_count * width)?;
    let indices: Vec<_> = bytes
        .chunks_exact(width as usize)
        .map(|v| {
            if width == 2 {
                u16::from_le_bytes(v.try_into().unwrap()) as u32
            } else {
                u32::from_le_bytes(v.try_into().unwrap())
            }
        })
        .collect();
    let cards = indices
        .iter()
        .copied()
        .max()
        .context("impostor inputs: no indices")?
        / 4
        + 1;
    let mut desc = D3D11_SHADER_RESOURCE_VIEW_DESC::default();
    unsafe { view.GetDesc(&mut desc) };
    let (first_element, elements) = match desc.ViewDimension {
        D3D_SRV_DIMENSION_BUFFER => unsafe {
            (
                desc.Anonymous.Buffer.Anonymous1.FirstElement,
                desc.Anonymous.Buffer.Anonymous2.NumElements,
            )
        },
        D3D_SRV_DIMENSION_BUFFEREX => unsafe {
            let v = desc.Anonymous.BufferEx;
            ensure!(
                v.Flags == 0,
                "impostor inputs: unsupported extended view flags {}",
                v.Flags
            );
            (v.FirstElement, v.NumElements)
        },
        dimension => anyhow::bail!(
            "impostor inputs: unsupported view dimension {dimension:?} format {:?}",
            desc.Format
        ),
    };
    let buf: ID3D11Buffer = unsafe { view.GetResource()? }.cast()?;
    let mut bd = D3D11_BUFFER_DESC::default();
    unsafe { buf.GetDesc(&mut bd) };
    ensure!(
        bd.StructureByteStride == 12 && cards <= index_count / 6 && cards <= elements,
        "impostor inputs: stride {}, {cards} addressed cards, {elements} available",
        bd.StructureByteStride
    );
    let bytes = buffer_slice(
        ctx,
        &buf,
        first_element
            .checked_mul(12)
            .context("card offset overflow")?,
        cards * 12,
    )?;
    let cards = bytes
        .as_chunks::<12>()
        .0
        .iter()
        .map(|v| {
            std::array::from_fn(|i| u32::from_le_bytes(v[4 * i..4 * i + 4].try_into().unwrap()))
        })
        .collect();
    Ok(ImpostorInputs {
        raster: matches!(index_count, 144 | 1392 | 4872 | 5946 | 6534 | 7542)
            .then(|| group_raster(ctx))
            .transpose()?,
        view_dimension: desc.ViewDimension.0,
        first_element,
        elements,
        cards,
        indices,
        cb,
    })
}

fn group_raster(ctx: &ID3D11DeviceContext) -> Result<GroupRaster> {
    let mut rects = [windows::Win32::Foundation::RECT::default(); 16];
    let mut n = rects.len() as u32;
    let mut views = [None, None];
    let mut sampler = [None];
    unsafe {
        ctx.RSGetScissorRects(&mut n, Some(rects.as_mut_ptr()));
        ctx.PSGetShaderResources(0, Some(&mut views));
        ctx.PSGetSamplers(0, Some(&mut sampler));
    }
    ensure!(n <= 16, "impostor inputs: invalid scissor count");
    let mut sd = D3D11_SAMPLER_DESC::default();
    unsafe {
        sampler[0]
            .as_ref()
            .context("impostor inputs: missing s0")?
            .GetDesc(&mut sd)
    };
    let mut atlas = Vec::new();
    for view in views {
        let view = view.context("impostor inputs: missing atlas")?;
        let tex: ID3D11Texture2D = unsafe { view.GetResource()? }.cast()?;
        let mut td = D3D11_TEXTURE2D_DESC::default();
        let mut vd = D3D11_SHADER_RESOURCE_VIEW_DESC::default();
        unsafe {
            tex.GetDesc(&mut td);
            view.GetDesc(&mut vd)
        };
        ensure!(
            vd.ViewDimension == D3D_SRV_DIMENSION_TEXTURE2DARRAY,
            "impostor inputs: unexpected atlas dimension"
        );
        let a = unsafe { vd.Anonymous.Texture2DArray };
        atlas.push(AtlasView {
            resource: tex.as_raw() as usize,
            format: vd.Format.0,
            extent: [td.Width, td.Height, td.MipLevels, td.ArraySize],
            range: [
                a.MostDetailedMip,
                a.MipLevels,
                a.FirstArraySlice,
                a.ArraySize,
            ],
        });
    }
    let mut rs = D3D11_RASTERIZER_DESC {
        CullMode: D3D11_CULL_BACK,
        ..Default::default()
    };
    if let Ok(state) = unsafe { ctx.RSGetState() } {
        unsafe { state.GetDesc(&mut rs) };
    }
    Ok(GroupRaster {
        scissor_enabled: rs.ScissorEnable.as_bool(),
        scissors: rects[..n as usize]
            .iter()
            .map(|r| [r.left, r.top, r.right, r.bottom])
            .collect(),
        cull: rs.CullMode.0,
        front_ccw: rs.FrontCounterClockwise.as_bool(),
        atlas,
        sampler: [sd.Filter.0, sd.AddressU.0, sd.AddressV.0, sd.AddressW.0],
        lod: [sd.MipLODBias, sd.MinLOD, sd.MaxLOD],
    })
}

pub(super) fn buffer_slice(
    ctx: &ID3D11DeviceContext,
    buffer: &ID3D11Buffer,
    start: u32,
    bytes: u32,
) -> Result<Vec<u8>> {
    let mut desc = D3D11_BUFFER_DESC::default();
    unsafe { buffer.GetDesc(&mut desc) };
    let end = start.checked_add(bytes).context("buffer slice overflow")?;
    ensure!(
        bytes > 0 && end <= desc.ByteWidth,
        "impostor inputs: truncated buffer"
    );
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: bytes,
        Usage: D3D11_USAGE_STAGING,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        ..Default::default()
    };
    let mut staging = None;
    unsafe {
        ctx.GetDevice()?
            .CreateBuffer(&desc, None, Some(&mut staging))?
    };
    let staging = staging.context("missing staging buffer")?;
    let region = D3D11_BOX {
        left: start,
        right: end,
        top: 0,
        bottom: 1,
        front: 0,
        back: 1,
    };
    let mut map = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe {
        ctx.CopySubresourceRegion(&staging, 0, 0, 0, 0, buffer, 0, Some(&region));
        ctx.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut map))?
    };
    let result = (!map.pData.is_null()).then(|| {
        unsafe { std::slice::from_raw_parts(map.pData.cast::<u8>(), bytes as usize) }.to_vec()
    });
    unsafe { ctx.Unmap(&staging, 0) };
    result.context("empty mapped buffer")
}
