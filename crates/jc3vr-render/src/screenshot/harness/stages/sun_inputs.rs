use anyhow::{Context as _, bail};
use windows::{
    Win32::Graphics::{
        Direct3D11::{
            D3D11_DEPTH_STENCIL_VIEW_DESC, D3D11_DSV_DIMENSION_TEXTURE2D, D3D11_MAP_READ,
            D3D11_MAPPED_SUBRESOURCE, D3D11_RENDER_TARGET_VIEW_DESC, D3D11_RTV_DIMENSION_TEXTURE2D,
            D3D11_TEXTURE2D_DESC, ID3D11DepthStencilView, ID3D11DeviceContext,
            ID3D11RenderTargetView, ID3D11Resource, ID3D11Texture2D,
        },
        Dxgi::Common::{
            DXGI_FORMAT, DXGI_FORMAT_D16_UNORM, DXGI_FORMAT_D24_UNORM_S8_UINT,
            DXGI_FORMAT_D32_FLOAT, DXGI_FORMAT_D32_FLOAT_S8X24_UINT, DXGI_FORMAT_R11G11B10_FLOAT,
            DXGI_FORMAT_R16_TYPELESS, DXGI_FORMAT_R24G8_TYPELESS, DXGI_FORMAT_R32_TYPELESS,
            DXGI_FORMAT_R32G8X24_TYPELESS,
        },
    },
    core::Interface,
};

use crate::screenshot::{
    create_staging,
    harness::{
        diagnostic_capture_id,
        stages::{
            STATE, Source, Stage, StageArtifact, StageImage, StageRecord, SunInputMetadata,
            capture_raster_state, format_byte_width, stage_image, staging_format, validate_desc,
        },
        sun_inputs_requested,
    },
};

pub(crate) fn capture_before_sun(ctx: &ID3D11DeviceContext, eye: usize) {
    if !sun_inputs_requested() {
        return;
    }
    let Some(id) = diagnostic_capture_id() else {
        return;
    };
    let Some(frame) = crate::vr::capture_views().map(|views| views.predicted_display_time_ns)
    else {
        return;
    };
    let mut state = STATE.lock();
    if state.frame != Some(frame) {
        state.frame = Some(frame);
        state.records.clear();
        state.images.clear();
    }
    let mut rtv: Option<ID3D11RenderTargetView> = None;
    let mut dsv: Option<ID3D11DepthStencilView> = None;
    unsafe { ctx.OMGetRenderTargets(Some(std::slice::from_mut(&mut rtv)), Some(&mut dsv)) };
    for stage in [Stage::PreSunColor, Stage::PreSunDepth] {
        if state
            .records
            .iter()
            .any(|record| record.stage == stage && record.eye == eye)
        {
            continue;
        }
        let mut record = empty_record(frame, eye, stage, &id);
        capture_raster_state(ctx, &mut record);
        let result = match stage {
            Stage::PreSunColor => read_color(ctx, rtv.as_ref(), &mut record),
            Stage::PreSunDepth => read_depth(ctx, dsv.as_ref(), &mut record),
            _ => unreachable!(),
        };
        match result {
            Ok(artifact) => {
                let index = state.records.len();
                state.images.push(StageImage {
                    record: index,
                    artifact,
                });
            }
            Err(error) => record.error = Some(format!("sun input capture: {error:#}")),
        }
        state.records.push(record);
    }
}

pub(crate) fn fail_sun_treatment(eye: usize, error: &str) {
    treatment_record(eye, Some(error));
}

pub(crate) fn record_sun_treatment(eye: usize) {
    treatment_record(eye, None);
}

fn treatment_record(eye: usize, error: Option<&str>) {
    let Some(id) = diagnostic_capture_id() else {
        return;
    };
    let Some(frame) = crate::vr::capture_views().map(|views| views.predicted_display_time_ns)
    else {
        return;
    };
    let mut state = STATE.lock();
    if state.frame != Some(frame) {
        state.frame = Some(frame);
        state.records.clear();
        state.images.clear();
    }
    if let Some(record) = state
        .records
        .iter_mut()
        .find(|record| record.stage == Stage::SunDepthClipTreatment && record.eye == eye)
    {
        if let Some(error) = error {
            record.error = Some(error.to_owned());
        }
        return;
    }
    let mut record = empty_record(frame, eye, Stage::SunDepthClipTreatment, &id);
    record.error = error.map(str::to_owned);
    state.records.push(record);
}

pub(crate) fn complete_sun_treatment(records: &mut Vec<StageRecord>, frame: i64, id: &str) {
    if records.iter().any(|record| {
        record.stage == Stage::SunDepthClipTreatment
            && record.eye == 1
            && record
                .sun_input
                .as_ref()
                .is_some_and(|input| input.capture_id == id)
    }) {
        return;
    }
    let mut record = empty_record(frame, 1, Stage::SunDepthClipTreatment, id);
    record.error = Some("sun depth clip: requested right-eye treatment was not applied".to_owned());
    records.push(record);
}

pub(crate) fn complete_sun_inputs(
    records: &mut Vec<StageRecord>,
    frame: i64,
    id: &str,
    eyes: [bool; 2],
) {
    for (eye, required) in eyes.into_iter().enumerate() {
        if !required {
            continue;
        }
        for stage in [Stage::PreSunColor, Stage::PreSunDepth] {
            if records.iter().any(|record| {
                record.stage == stage
                    && record.eye == eye
                    && record
                        .sun_input
                        .as_ref()
                        .is_some_and(|input| input.capture_id == id)
            }) {
                continue;
            }
            let mut record = empty_record(frame, eye, stage, id);
            record.error =
                Some("sun input capture: no matching draw on the captured frame".to_owned());
            records.push(record);
        }
    }
}

fn empty_record(frame: i64, eye: usize, stage: Stage, id: &str) -> StageRecord {
    StageRecord {
        frame,
        stage,
        eye,
        source: None,
        viewports: Vec::new(),
        scissors: Vec::new(),
        scissor_enabled: None,
        means: None,
        png: None,
        raw: None,
        error: None,
        water_input: None,
        velocity: None,
        aa: None,
        sun_input: Some(SunInputMetadata {
            capture_id: id.to_owned(),
            view: 0,
            resource: 0,
            view_format: None,
            view_dimension: None,
            mip: None,
        }),
    }
}

fn read_color(
    ctx: &ID3D11DeviceContext,
    view: Option<&ID3D11RenderTargetView>,
    record: &mut StageRecord,
) -> anyhow::Result<StageArtifact> {
    let view = view.context("render target is unbound")?;
    let mut desc = D3D11_RENDER_TARGET_VIEW_DESC::default();
    unsafe { view.GetDesc(&mut desc) };
    let metadata = record.sun_input.as_mut().unwrap();
    metadata.view = view.as_raw() as usize;
    metadata.view_format = Some(desc.Format.0);
    metadata.view_dimension = Some(desc.ViewDimension.0);
    if desc.ViewDimension != D3D11_RTV_DIMENSION_TEXTURE2D {
        bail!("unsupported RTV dimension");
    }
    let mip = unsafe { desc.Anonymous.Texture2D.MipSlice };
    metadata.mip = Some(mip);
    let resource = unsafe { view.GetResource() }.context("getting render target resource")?;
    read_target(ctx, &resource, mip, record)
}

fn read_depth(
    ctx: &ID3D11DeviceContext,
    view: Option<&ID3D11DepthStencilView>,
    record: &mut StageRecord,
) -> anyhow::Result<StageArtifact> {
    let view = view.context("depth target is unbound")?;
    let mut desc = D3D11_DEPTH_STENCIL_VIEW_DESC::default();
    unsafe { view.GetDesc(&mut desc) };
    let metadata = record.sun_input.as_mut().unwrap();
    metadata.view = view.as_raw() as usize;
    metadata.view_format = Some(desc.Format.0);
    metadata.view_dimension = Some(desc.ViewDimension.0);
    if desc.ViewDimension != D3D11_DSV_DIMENSION_TEXTURE2D {
        bail!("unsupported DSV dimension");
    }
    let mip = unsafe { desc.Anonymous.Texture2D.MipSlice };
    metadata.mip = Some(mip);
    let resource = unsafe { view.GetResource() }.context("getting depth target resource")?;
    read_target(ctx, &resource, mip, record)
}

fn target_format_supported(stage: Stage, resource: DXGI_FORMAT, view: DXGI_FORMAT) -> bool {
    match stage {
        Stage::PreSunColor => resource == DXGI_FORMAT_R11G11B10_FLOAT && view == resource,
        Stage::PreSunDepth => matches!(
            (resource, view),
            (
                DXGI_FORMAT_R32G8X24_TYPELESS | DXGI_FORMAT_D32_FLOAT_S8X24_UINT,
                DXGI_FORMAT_D32_FLOAT_S8X24_UINT
            ) | (
                DXGI_FORMAT_R32_TYPELESS | DXGI_FORMAT_D32_FLOAT,
                DXGI_FORMAT_D32_FLOAT
            ) | (
                DXGI_FORMAT_R24G8_TYPELESS | DXGI_FORMAT_D24_UNORM_S8_UINT,
                DXGI_FORMAT_D24_UNORM_S8_UINT
            ) | (
                DXGI_FORMAT_R16_TYPELESS | DXGI_FORMAT_D16_UNORM,
                DXGI_FORMAT_D16_UNORM
            )
        ),
        _ => false,
    }
}

fn read_target(
    ctx: &ID3D11DeviceContext,
    resource: &ID3D11Resource,
    mip: u32,
    record: &mut StageRecord,
) -> anyhow::Result<StageArtifact> {
    record.sun_input.as_mut().unwrap().resource = resource.as_raw() as usize;
    let texture: ID3D11Texture2D = resource.cast().context("target resource is not 2D")?;
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { texture.GetDesc(&mut desc) };
    if mip != 0 || desc.MipLevels != 1 {
        bail!("unsupported target mip window");
    }
    let view_format = record
        .sun_input
        .as_ref()
        .and_then(|input| input.view_format)
        .map(DXGI_FORMAT)
        .context("missing target view format")?;
    if !target_format_supported(record.stage, desc.Format, view_format) {
        bail!(
            "unsupported target resource/view formats {:?}/{:?}",
            desc.Format,
            view_format
        );
    }
    validate_desc(&desc)?;
    record.source = Some(Source {
        width: desc.Width,
        height: desc.Height,
        format: format!("{:?}", desc.Format),
        format_id: desc.Format.0,
        bytes_per_pixel: format_byte_width(desc.Format),
    });
    let device = unsafe { ctx.GetDevice() }.context("getting context device")?;
    let staging_desc = D3D11_TEXTURE2D_DESC {
        Format: staging_format(desc.Format),
        ..desc
    };
    let staging = unsafe { create_staging(&device, &staging_desc) }?;
    unsafe {
        ctx.CopyResource(&staging, &texture);
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        ctx.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .context("mapping target")?;
        let result = (|| {
            let len = (mapped.RowPitch as usize)
                .checked_mul(desc.Height as usize)
                .filter(|len| *len <= isize::MAX as usize)
                .context("mapping length overflow")?;
            if mapped.pData.is_null() {
                bail!("target mapping has no data");
            }
            let bytes = std::slice::from_raw_parts(mapped.pData.cast::<u8>(), len);
            stage_image(record.stage, bytes, mapped.RowPitch as usize, &desc)
        })();
        ctx.Unmap(&staging, 0);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_sun_stages_are_explicit_and_deduplicated() {
        let mut records = Vec::new();
        complete_sun_inputs(&mut records, 12, "capture", [true, false]);
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|record| record.error.is_some()));
        complete_sun_inputs(&mut records, 12, "capture", [true, true]);
        assert_eq!(records.len(), 4);
    }

    #[test]
    fn missing_sun_treatment_is_explicit() {
        let mut records = Vec::new();
        complete_sun_treatment(&mut records, 42, "capture");
        assert_eq!(records.len(), 1);
        assert!(records[0].error.is_some());
        complete_sun_treatment(&mut records, 42, "capture");
        assert_eq!(records.len(), 1);
    }

    #[test]
    fn depth_view_must_match_storage_family() {
        assert!(target_format_supported(
            Stage::PreSunDepth,
            DXGI_FORMAT_R32G8X24_TYPELESS,
            DXGI_FORMAT_D32_FLOAT_S8X24_UINT
        ));
        assert!(!target_format_supported(
            Stage::PreSunDepth,
            DXGI_FORMAT_R32_TYPELESS,
            DXGI_FORMAT_D32_FLOAT_S8X24_UINT
        ));
        assert!(!target_format_supported(
            Stage::PreSunColor,
            DXGI_FORMAT_R32_TYPELESS,
            DXGI_FORMAT_D32_FLOAT
        ));
    }
}
