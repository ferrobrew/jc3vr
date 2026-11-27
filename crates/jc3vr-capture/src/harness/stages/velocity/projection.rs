use anyhow::{Context, Result, ensure};
use windows::{Win32::Graphics::Direct3D11::*, core::Interface};

use crate::harness::{
    bound_buffer::{ShaderStage, read_bound_constant_buffer},
    stages::velocity::BaseProjection,
};

pub(super) fn submit(
    ctx: &ID3D11DeviceContext,
    probe: &BaseProjection,
    draw: impl FnOnce(),
) -> Result<()> {
    let seen = &probe.buffers[0];
    let initial = seen
        .bits
        .as_deref()
        .context("basemesh VP: missing bound rows")?;
    let physical = probe
        .generated
        .as_ref()
        .context("basemesh VP: missing physical rows")?;
    ensure!(
        initial.len() == 16
            && seen.first == 0
            && seen.count == 49
            && initial != physical
            && physical.iter().all(|v| f32::from_bits(*v).is_finite()),
        "basemesh VP: unexpected source"
    );
    let mut bound = [None];
    unsafe { ctx.VSGetConstantBuffers(0, Some(&mut bound)) };
    let original = bound[0].take().context("basemesh VP: missing b0")?;
    let mut desc = D3D11_BUFFER_DESC::default();
    unsafe { original.GetDesc(&mut desc) };
    ensure!(
        desc.ByteWidth == seen.count * 16,
        "basemesh VP: original is not bound in full"
    );
    let data = if probe.applied == Some(true) {
        physical.as_slice()
    } else {
        initial
    };
    let mut clone = None;
    unsafe {
        ctx.GetDevice()?.CreateBuffer(
            &D3D11_BUFFER_DESC {
                ByteWidth: 64,
                Usage: D3D11_USAGE_IMMUTABLE,
                BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                ..Default::default()
            },
            Some(&D3D11_SUBRESOURCE_DATA {
                pSysMem: data.as_ptr().cast(),
                ..Default::default()
            }),
            Some(&mut clone),
        )?
    };
    let clone = clone.context("basemesh VP: missing clone")?;
    let verify = |buffer: &ID3D11Buffer, count, rows: &[u32]| -> Result<()> {
        let b = read_bound_constant_buffer(ctx, ShaderStage::Vertex, 0, 4)?;
        ensure!(
            b.pointer == buffer.as_raw() as usize
                && b.range_known
                && b.first_constant == 0
                && b.num_constants == count
                && b.range_error.is_none()
                && b.values.is_some_and(|v| v
                    .iter()
                    .copied()
                    .map(f32::to_bits)
                    .eq(rows.iter().copied())),
            "basemesh VP: bound buffer, range or rows differ"
        );
        Ok(())
    };
    unsafe { ctx.VSSetConstantBuffers(0, Some(&[Some(clone.clone())])) };
    let submitted = verify(&clone, 4, data).map(|()| draw());
    unsafe { ctx.VSSetConstantBuffers(0, Some(&[Some(original.clone())])) };
    verify(&original, seen.count, initial)?;
    submitted
}
