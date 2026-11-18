use anyhow::{Context as _, bail};
use windows::{
    Win32::Graphics::{
        Direct3D::D3D_SRV_DIMENSION_TEXTURE2D,
        Direct3D11::{
            D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_SHADER_RESOURCE_VIEW_DESC,
            D3D11_TEXTURE2D_DESC, ID3D11DeviceContext, ID3D11PixelShader, ID3D11ShaderResourceView,
            ID3D11Texture2D,
        },
        Dxgi::Common::{
            DXGI_FORMAT, DXGI_FORMAT_R11G11B10_FLOAT, DXGI_FORMAT_R32_FLOAT,
            DXGI_FORMAT_R32_TYPELESS,
        },
    },
    core::Interface,
};

use crate::screenshot::{
    create_staging,
    harness::stages::{
        STATE, Source, Stage, StageArtifact, StageImage, StageRecord, WATER_INPUT_SLOTS,
        WaterInputMetadata, capture_raster_state, format_byte_width, stage_image, validate_desc,
    },
};

pub(crate) fn capture_water_inputs(
    ctx: &ID3D11DeviceContext,
    eye: usize,
    capture_id: &str,
    draw_index: usize,
    tessellated: bool,
) {
    let Some(frame) = crate::vr::capture_views().map(|v| v.predicted_display_time_ns) else {
        return;
    };
    let mut state = STATE.lock();
    if state.frame != Some(frame) {
        state.frame = Some(frame);
        state.records.clear();
        state.images.clear();
    }
    let mut shader: Option<ID3D11PixelShader> = None;
    unsafe { ctx.PSGetShader(&mut shader, None, None) };
    let identity = shader.as_ref().map_or(
        crate::stereo::single_pass::shader_identity::IdentityResult::Unmapped,
        crate::stereo::single_pass::shader_identity::lookup,
    );
    let shader = shader.as_ref().map_or(0, |value| value.as_raw() as usize);
    for slot in WATER_INPUT_SLOTS {
        let stage = slot.stage();
        if state
            .records
            .iter()
            .any(|record| record.stage == stage && record.eye == eye)
        {
            continue;
        }
        let mut record = StageRecord {
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
            sun_input: None,
            water_input: Some(WaterInputMetadata {
                capture_id: capture_id.to_owned(),
                slot,
                shader_slot: slot.index(),
                treatment_draws: 0,
                pixel_shader: shader,
                shader_identity: identity.clone(),
                draw_index,
                tessellated,
                view_format: None,
                view_dimension: None,
                most_detailed_mip: None,
                mip_levels: None,
                resource: None,
                resource_format: None,
            }),
        };
        capture_raster_state(ctx, &mut record);
        let mut view: Option<ID3D11ShaderResourceView> = None;
        unsafe { ctx.PSGetShaderResources(slot.index(), Some(std::slice::from_mut(&mut view))) };
        let result = view
            .as_ref()
            .context("shader resource view is unbound")
            .and_then(|view| read_water_input(ctx, view, &mut record));
        match result {
            Ok(artifact) => {
                let index = state.records.len();
                state.images.push(StageImage {
                    record: index,
                    artifact,
                });
            }
            Err(error) => record.error = Some(format!("water input capture: {error:#}")),
        }
        state.records.push(record);
    }
}

pub(crate) fn record_water_treatment(eye: usize, id: &str, error: Option<&str>) {
    let Some(frame) = crate::vr::capture_views().map(|v| v.predicted_display_time_ns) else {
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
        .find(|r| r.stage == Stage::WaterReflectionTreatment && r.eye == eye)
    {
        if let Some(error) = error {
            record.error = Some(error.to_owned());
        } else if let Some(input) = record.water_input.as_mut() {
            input.treatment_draws = input.treatment_draws.saturating_add(1);
        }
        return;
    }
    let mut record = empty_treatment(frame, eye, id);
    if let Some(error) = error {
        record.error = Some(error.to_owned());
    } else if let Some(input) = record.water_input.as_mut() {
        input.treatment_draws = 1;
    }
    state.records.push(record);
}

pub(crate) fn complete_water_treatment(
    records: &mut Vec<StageRecord>,
    frame: i64,
    id: &str,
    eyes: [bool; 2],
) {
    for (eye, required) in eyes.into_iter().enumerate() {
        if !required {
            continue;
        }
        if records.iter().any(|r| {
            r.stage == Stage::WaterReflectionTreatment
                && r.eye == eye
                && r.water_input
                    .as_ref()
                    .is_some_and(|input| input.capture_id == id)
        }) {
            continue;
        }
        let mut record = empty_treatment(frame, eye, id);
        record.error = Some("water reflection treatment: no matching draw was treated".to_owned());
        records.push(record);
    }
}

fn empty_treatment(frame: i64, eye: usize, id: &str) -> StageRecord {
    StageRecord {
        frame,
        stage: Stage::WaterReflectionTreatment,
        eye,
        source: None,
        viewports: Vec::new(),
        scissors: Vec::new(),
        scissor_enabled: None,
        means: None,
        png: None,
        raw: None,
        error: None,
        sun_input: None,
        water_input: Some(WaterInputMetadata {
            capture_id: id.to_owned(),
            slot: crate::screenshot::harness::stages::WaterInputSlot::ReflectionAlpha,
            shader_slot: 12,
            treatment_draws: 0,
            pixel_shader: 0,
            shader_identity: crate::stereo::single_pass::shader_identity::IdentityResult::Unmapped,
            draw_index: 0,
            tessellated: false,
            view_format: None,
            view_dimension: None,
            most_detailed_mip: None,
            mip_levels: None,
            resource: None,
            resource_format: None,
        }),
    }
}

pub(crate) fn complete_water_inputs(
    records: &mut Vec<StageRecord>,
    frame: i64,
    id: &str,
    eyes: [bool; 2],
) {
    for (eye, required) in eyes.into_iter().enumerate() {
        if !required {
            continue;
        }
        for slot in WATER_INPUT_SLOTS {
            if records.iter().any(|record| {
                record.stage == slot.stage()
                    && record.eye == eye
                    && record
                        .water_input
                        .as_ref()
                        .is_some_and(|input| input.capture_id == id)
            }) {
                continue;
            }
            records.push(StageRecord {
                frame,
                stage: slot.stage(),
                eye,
                source: None,
                viewports: Vec::new(),
                scissors: Vec::new(),
                scissor_enabled: None,
                means: None,
                png: None,
                raw: None,
                error: Some(
                    "water input capture: no matching draw on the captured frame".to_owned(),
                ),
                sun_input: None,
                water_input: Some(WaterInputMetadata {
                    capture_id: id.to_owned(),
                    slot,
                    shader_slot: slot.index(),
                    treatment_draws: 0,
                    pixel_shader: 0,
                    shader_identity:
                        crate::stereo::single_pass::shader_identity::IdentityResult::Unmapped,
                    draw_index: 0,
                    tessellated: false,
                    view_format: None,
                    view_dimension: None,
                    most_detailed_mip: None,
                    mip_levels: None,
                    resource: None,
                    resource_format: None,
                }),
            });
        }
    }
}

fn read_water_input(
    ctx: &ID3D11DeviceContext,
    view: &ID3D11ShaderResourceView,
    record: &mut StageRecord,
) -> anyhow::Result<StageArtifact> {
    let mut view_desc = D3D11_SHADER_RESOURCE_VIEW_DESC::default();
    unsafe { view.GetDesc(&mut view_desc) };
    let input = record
        .water_input
        .as_mut()
        .context("missing water input metadata")?;
    input.view_format = Some(view_desc.Format.0);
    input.view_dimension = Some(view_desc.ViewDimension.0);
    if view_desc.ViewDimension != D3D_SRV_DIMENSION_TEXTURE2D {
        bail!("unsupported SRV dimension {:?}", view_desc.ViewDimension);
    }
    let window = unsafe { view_desc.Anonymous.Texture2D };
    input.most_detailed_mip = Some(window.MostDetailedMip);
    input.mip_levels = Some(window.MipLevels);
    let resource = unsafe { view.GetResource() }.context("getting SRV resource")?;
    input.resource = Some(resource.as_raw() as usize);
    let texture: ID3D11Texture2D = resource.cast().context("SRV resource is not 2D")?;
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { texture.GetDesc(&mut desc) };
    record.water_input.as_mut().unwrap().resource_format = Some(desc.Format.0);
    if desc.ArraySize != 1 || desc.SampleDesc.Count != 1 || window.MostDetailedMip >= desc.MipLevels
    {
        bail!("unsupported texture array, sample count, or mip window");
    }
    if window.MipLevels == 0
        || (window.MipLevels != u32::MAX
            && window.MipLevels > desc.MipLevels.saturating_sub(window.MostDetailedMip))
    {
        bail!("invalid SRV mip window");
    }
    let mip = window.MostDetailedMip;
    let view_format = desc_format_for_view(desc.Format, view_desc.Format)?;
    let mut raw_desc = desc;
    raw_desc.Format = view_format;
    desc.Width = desc.Width.checked_shr(mip).unwrap_or(0).max(1);
    desc.Height = desc.Height.checked_shr(mip).unwrap_or(0).max(1);
    desc.MipLevels = 1;
    raw_desc.Width = desc.Width;
    raw_desc.Height = desc.Height;
    raw_desc.MipLevels = desc.MipLevels;
    validate_desc(&desc)?;
    record.source = Some(Source {
        width: desc.Width,
        height: desc.Height,
        format: format!("{:?}", view_format),
        format_id: view_format.0,
        bytes_per_pixel: format_byte_width(view_format),
    });
    let device = unsafe { ctx.GetDevice() }.context("getting context device")?;
    let staging = unsafe { create_staging(&device, &desc) }?;
    unsafe {
        ctx.CopySubresourceRegion(&staging, 0, 0, 0, 0, &texture, mip, None);
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        ctx.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .context("mapping water input")?;
        let result = (|| {
            let len = (mapped.RowPitch as usize)
                .checked_mul(desc.Height as usize)
                .filter(|len| *len <= isize::MAX as usize)
                .context("water input mapping length overflow")?;
            if mapped.pData.is_null() {
                bail!("water input mapping has no data");
            }
            let bytes = std::slice::from_raw_parts(mapped.pData.cast::<u8>(), len);
            stage_image(record.stage, bytes, mapped.RowPitch as usize, &raw_desc)
        })();
        ctx.Unmap(&staging, 0);
        result
    }
}

fn desc_format_for_view(resource: DXGI_FORMAT, view: DXGI_FORMAT) -> anyhow::Result<DXGI_FORMAT> {
    if (resource == view && matches!(view, DXGI_FORMAT_R32_FLOAT | DXGI_FORMAT_R11G11B10_FLOAT)
        || (resource == DXGI_FORMAT_R32_TYPELESS && view == DXGI_FORMAT_R32_FLOAT))
        && format_byte_width(view).is_some()
    {
        return Ok(view);
    }
    bail!("unsupported resource/view format combination {resource:?}/{view:?}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_float_view_over_typeless_storage_is_explicit() {
        assert_eq!(
            desc_format_for_view(DXGI_FORMAT_R32_TYPELESS, DXGI_FORMAT_R32_FLOAT).unwrap(),
            DXGI_FORMAT_R32_FLOAT
        );
        assert!(desc_format_for_view(DXGI_FORMAT_R32_FLOAT, DXGI_FORMAT_R32_TYPELESS).is_err());
    }

    #[test]
    fn missing_treatment_is_an_error_for_each_required_eye() {
        let mut records = Vec::new();
        complete_water_treatment(&mut records, 42, "capture", [true, true]);
        assert_eq!(records.len(), 2);
        for (eye, record) in records.iter().enumerate() {
            assert_eq!(record.eye, eye);
            assert_eq!(
                record.error.as_deref(),
                Some("water reflection treatment: no matching draw was treated")
            );
            assert_eq!(record.water_input.as_ref().unwrap().treatment_draws, 0);
        }
        complete_water_treatment(&mut records, 42, "capture", [true, true]);
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn missing_requested_eyes_are_explicit_and_deduplicated() {
        let mut records = Vec::new();
        complete_water_inputs(&mut records, 42, "capture", [true, false]);
        assert_eq!(records.len(), 3);
        assert!(
            records
                .iter()
                .all(|record| record.eye == 0 && record.error.is_some())
        );
        complete_water_inputs(&mut records, 42, "capture", [true, false]);
        assert_eq!(records.len(), 3);
        complete_water_inputs(&mut records, 42, "capture", [true, true]);
        assert_eq!(records.len(), 6);
        assert!(
            records
                .iter()
                .all(|record| record.water_input.as_ref().unwrap().capture_id == "capture")
        );
    }
}
