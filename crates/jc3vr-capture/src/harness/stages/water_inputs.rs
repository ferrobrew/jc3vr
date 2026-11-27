use anyhow::{Context as _, bail, ensure};
use jc3vr_render::readback::{read_mapped, staging_texture};
use serde::Serialize;
use sha2::{Digest, Sha256};
use windows::{
    Win32::Graphics::{
        Direct3D::D3D_SRV_DIMENSION_TEXTURE2D,
        Direct3D11::{
            D3D11_SAMPLER_DESC, D3D11_SHADER_RESOURCE_VIEW_DESC, D3D11_TEXTURE2D_DESC,
            ID3D11DeviceContext, ID3D11PixelShader, ID3D11ShaderResourceView, ID3D11Texture2D,
        },
        Dxgi::Common::{
            DXGI_FORMAT, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R11G11B10_FLOAT,
            DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_R32_FLOAT, DXGI_FORMAT_R32_TYPELESS,
        },
    },
    core::Interface,
};

use crate::harness::stages::{
    STATE, Source, Stage, StageArtifact, StageImage, StageRecord, WaterInputMetadata,
    capture_raster_state, format_byte_width, stage_image, validate_desc,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WaterInputSlot {
    Reflection,
    ReflectionAlpha,
    StaticReflection,
    Refraction,
    RefractionDepth,
    Wave0,
    Wave1,
    Wave2,
    Wave3,
    AmbientOcclusion,
}

impl WaterInputSlot {
    fn stage(self) -> Stage {
        match self {
            Self::StaticReflection => Stage::WaterInputStaticReflection,
            Self::Reflection => Stage::WaterInputReflection,
            Self::ReflectionAlpha => Stage::WaterInputReflectionAlpha,
            Self::Refraction => Stage::WaterInputRefraction,
            Self::RefractionDepth => Stage::WaterInputRefractionDepth,
            Self::Wave0 => Stage::WaterInputWave0,
            Self::Wave1 => Stage::WaterInputWave1,
            Self::Wave2 => Stage::WaterInputWave2,
            Self::Wave3 => Stage::WaterInputWave3,
            Self::AmbientOcclusion => Stage::WaterInputAmbientOcclusion,
        }
    }

    fn index(self) -> u32 {
        match self {
            Self::Reflection => 1,
            Self::StaticReflection => 2,
            Self::ReflectionAlpha => 12,
            Self::Refraction => 3,
            Self::RefractionDepth => 0,
            Self::Wave0 => 7,
            Self::Wave1 => 8,
            Self::Wave2 => 9,
            Self::Wave3 => 10,
            Self::AmbientOcclusion => 11,
        }
    }
}

const WATER_INPUT_SLOTS: [WaterInputSlot; 4] = [
    WaterInputSlot::Reflection,
    WaterInputSlot::ReflectionAlpha,
    WaterInputSlot::StaticReflection,
    WaterInputSlot::Refraction,
];

fn requested_slots(surface: bool) -> impl Iterator<Item = WaterInputSlot> {
    const SURFACE: [WaterInputSlot; 6] = [
        WaterInputSlot::RefractionDepth,
        WaterInputSlot::Wave0,
        WaterInputSlot::Wave1,
        WaterInputSlot::Wave2,
        WaterInputSlot::Wave3,
        WaterInputSlot::AmbientOcclusion,
    ];
    WATER_INPUT_SLOTS
        .into_iter()
        .chain(surface.then_some(SURFACE).into_iter().flatten())
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct WaveFootprint {
    mip_sha256: Vec<String>,
    max_anisotropy: u32,
    lod_bits: [u32; 4],
}

fn wave_footprint(
    ctx: &ID3D11DeviceContext,
    view: &ID3D11ShaderResourceView,
    sampler: &D3D11_SAMPLER_DESC,
) -> anyhow::Result<WaveFootprint> {
    let mut vd = D3D11_SHADER_RESOURCE_VIEW_DESC::default();
    let mut td = D3D11_TEXTURE2D_DESC::default();
    let resource = unsafe { view.GetResource()? };
    let texture: ID3D11Texture2D = resource.cast()?;
    unsafe {
        view.GetDesc(&mut vd);
        texture.GetDesc(&mut td);
    }
    ensure!(
        vd.ViewDimension == D3D_SRV_DIMENSION_TEXTURE2D
            && vd.Format == DXGI_FORMAT_R16G16B16A16_FLOAT
            && td.Format == vd.Format
            && td.ArraySize == 1
            && td.SampleDesc.Count == 1
            && (td.Width, td.Height, td.MipLevels) == (128, 128, 8)
            && unsafe {
                vd.Anonymous.Texture2D.MostDetailedMip == 0 && vd.Anonymous.Texture2D.MipLevels == 8
            },
        "lake footprint: unexpected wave texture or view"
    );
    let device = unsafe { ctx.GetDevice()? };
    let mut mip_sha256 = Vec::with_capacity(8);
    for level in 0..8 {
        let dim = 128 >> level;
        let staging = unsafe {
            staging_texture(
                &device,
                &D3D11_TEXTURE2D_DESC {
                    Width: dim,
                    Height: dim,
                    MipLevels: 1,
                    ..td
                },
            )
        }?;
        // SAFETY: the context is locked, and every row read lies within the mip's mapped rows.
        let hash = unsafe {
            ctx.CopySubresourceRegion(&staging, 0, 0, 0, 0, &texture, level, None);
            read_mapped(ctx, &staging, |m| -> anyhow::Result<String> {
                ensure!(
                    m.row_pitch() >= dim as usize * 8,
                    "lake footprint: invalid mip mapping"
                );
                let mut digest = Sha256::new();
                for y in 0..dim as usize {
                    digest.update(m.row(y, dim as usize * 8));
                }
                Ok(format!("{:x}", digest.finalize()))
            })?
        };
        mip_sha256.push(hash?);
    }
    Ok(WaveFootprint {
        mip_sha256,
        max_anisotropy: sampler.MaxAnisotropy,
        lod_bits: [sampler.MipLODBias, sampler.MinLOD, sampler.MaxLOD, unsafe {
            ctx.GetResourceMinLOD(&resource)
        }]
        .map(f32::to_bits),
    })
}

pub(crate) fn capture_water_inputs(
    ctx: &ID3D11DeviceContext,
    eye: usize,
    capture_id: &str,
    draw_index: usize,
    tess_index: Option<usize>,
) {
    let Some(frame) = jc3vr_view::vr::capture_views().map(|v| v.predicted_display_time_ns) else {
        return;
    };
    let surface = crate::harness::water_surface_inputs_requested();
    let samplers =
        surface && tess_index == Some(7) && crate::harness::lake_samplers_requested(frame);
    let waves = samplers && crate::harness::lake_waves_requested(frame);
    let gradients = waves && crate::harness::lake_gradients_requested(frame);
    let mut state = STATE.lock();
    if state.frame != Some(frame) {
        state.frame = Some(frame);
        state.records.clear();
        state.images.clear();
    }
    let mut shader: Option<ID3D11PixelShader> = None;
    unsafe { ctx.PSGetShader(&mut shader, None, None) };
    let identity = shader.as_ref().map_or(
        jc3vr_render::stereo::single_pass::shader_identity::IdentityResult::Unmapped,
        jc3vr_render::stereo::single_pass::shader_identity::lookup,
    );
    let shader = shader.as_ref().map_or(0, |value| value.as_raw() as usize);
    for slot in requested_slots(surface) {
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
            velocity: None,
            aa: None,
            sun_input: None,
            water_input: Some(WaterInputMetadata {
                capture_id: capture_id.to_owned(),
                slot,
                shader_slot: slot.index(),
                treatment_draws: 0,
                projection: None,
                sampler: None,
                wave_footprint: None,
                pixel_shader: shader,
                shader_identity: identity.clone(),
                draw_index,
                tessellated: tess_index.is_some(),
                tess_index,
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
        let result = result.and_then(|artifact| {
            let index = match slot {
                WaterInputSlot::Refraction if samplers => Some(2),
                WaterInputSlot::RefractionDepth if samplers => Some(0),
                WaterInputSlot::Wave0
                | WaterInputSlot::Wave1
                | WaterInputSlot::Wave2
                | WaterInputSlot::Wave3
                    if waves =>
                {
                    Some(slot.index())
                }
                _ => None,
            };
            if let Some(index) = index {
                let mut sampler = [None];
                unsafe { ctx.PSGetSamplers(index, Some(&mut sampler)) };
                let mut desc = D3D11_SAMPLER_DESC::default();
                unsafe {
                    sampler[0]
                        .as_ref()
                        .context("lake input: missing sampler")?
                        .GetDesc(&mut desc)
                };
                if gradients && matches!(index, 8 | 9) {
                    record
                        .water_input
                        .as_mut()
                        .context("missing wave input")?
                        .wave_footprint = Some(wave_footprint(
                        ctx,
                        view.as_ref().context("missing wave view")?,
                        &desc,
                    )?);
                }
                record
                    .water_input
                    .as_mut()
                    .context("missing lake input")?
                    .sampler = Some([
                    index as i32,
                    desc.Filter.0,
                    desc.AddressU.0,
                    desc.AddressV.0,
                    desc.AddressW.0,
                ]);
            }
            if surface && matches!(slot, WaterInputSlot::StaticReflection) {
                let projection =
                    crate::harness::stages::lake_projection::capture(ctx, capture_id, frame, eye)?;
                record
                    .water_input
                    .as_mut()
                    .context("missing lake input")?
                    .projection = Some(projection);
            }
            Ok(artifact)
        });
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
    let Some(frame) = jc3vr_view::vr::capture_views().map(|v| v.predicted_display_time_ns) else {
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
        velocity: None,
        aa: None,
        sun_input: None,
        water_input: Some(WaterInputMetadata {
            capture_id: id.to_owned(),
            slot: crate::harness::stages::WaterInputSlot::ReflectionAlpha,
            shader_slot: 12,
            treatment_draws: 0,
            projection: None,
            sampler: None,
            wave_footprint: None,
            pixel_shader: 0,
            shader_identity:
                jc3vr_render::stereo::single_pass::shader_identity::IdentityResult::Unmapped,
            draw_index: 0,
            tessellated: false,
            tess_index: None,
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
    surface: bool,
    records: &mut Vec<StageRecord>,
    frame: i64,
    id: &str,
    eyes: [bool; 2],
) {
    for (eye, required) in eyes.into_iter().enumerate() {
        if !required {
            continue;
        }
        for slot in requested_slots(surface) {
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
                velocity: None,
                aa: None,
                sun_input: None,
                water_input: Some(WaterInputMetadata {
                    capture_id: id.to_owned(),
                    slot,
                    shader_slot: slot.index(),
                    treatment_draws: 0,
                    projection: None,
                    sampler: None,
                    wave_footprint: None,
                    pixel_shader: 0,
                    shader_identity:
                        jc3vr_render::stereo::single_pass::shader_identity::IdentityResult::Unmapped,
                    draw_index: 0,
                    tessellated: false,
                    tess_index: None,
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
    ensure!(
        u64::from(desc.Width) * u64::from(desc.Height)
            <= 16 * 1024 * 1024 / format_byte_width(view_format).unwrap() as u64,
        "selected mip exceeds the 16 MiB water input capture limit"
    );
    record.source = Some(Source {
        width: desc.Width,
        height: desc.Height,
        format: format!("{:?}", view_format),
        format_id: view_format.0,
        bytes_per_pixel: format_byte_width(view_format),
    });
    let device = unsafe { ctx.GetDevice() }.context("getting context device")?;
    let staging = unsafe { staging_texture(&device, &desc) }?;
    // SAFETY: the context is locked, and the staging copy is the selected mip's size.
    unsafe {
        ctx.CopySubresourceRegion(&staging, 0, 0, 0, 0, &texture, mip, None);
        read_mapped(ctx, &staging, |mapped| {
            stage_image(
                record.stage,
                mapped.rows(desc.Height)?,
                mapped.row_pitch(),
                &raw_desc,
            )
        })
        .context("mapping water input")?
    }
}

fn desc_format_for_view(resource: DXGI_FORMAT, view: DXGI_FORMAT) -> anyhow::Result<DXGI_FORMAT> {
    if (resource == view
        && matches!(
            view,
            DXGI_FORMAT_R32_FLOAT
                | DXGI_FORMAT_R11G11B10_FLOAT
                | DXGI_FORMAT_R16G16B16A16_FLOAT
                | DXGI_FORMAT_R8G8B8A8_UNORM
        )
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
        complete_water_inputs(false, &mut records, 42, "capture", [true, false]);
        assert_eq!(records.len(), WATER_INPUT_SLOTS.len());
        assert!(
            records
                .iter()
                .all(|record| record.eye == 0 && record.error.is_some())
        );
        complete_water_inputs(false, &mut records, 42, "capture", [true, false]);
        assert_eq!(records.len(), WATER_INPUT_SLOTS.len());
        complete_water_inputs(false, &mut records, 42, "capture", [true, true]);
        assert_eq!(records.len(), 2 * WATER_INPUT_SLOTS.len());
        assert!(
            records
                .iter()
                .all(|record| record.water_input.as_ref().unwrap().capture_id == "capture")
        );
    }
}
