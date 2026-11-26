use std::cell::RefCell;

use anyhow::{Context, Result, ensure};
use jc3gi::graphics_engine::graphics_engine::GraphicsEngine;
use jc3vr_view::stereo::{draw_index, engine_context::EngineContext};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R32_UINT, DXGI_FORMAT_R32G8X24_TYPELESS};

use crate::screenshot::{
    harness,
    harness::stages::{
        STATE, Source, Stage, StageArtifact, StageImage, empty_stage_record, read_stage,
    },
};

thread_local! { static ACTIVE: RefCell<Option<Atlas>> = const { RefCell::new(None) }; }

struct Atlas {
    id: String,
    frame: i64,
    eye: usize,
    extent: Option<(u32, u32)>,
    owners: Vec<u32>,
    before: Option<Vec<u8>>,
    draws: usize,
    error: Option<String>,
}

pub(crate) fn scope(draw: impl FnOnce()) {
    let Some((id, frame)) = harness::diagnostic_capture_id().zip(harness::frame_identity()) else {
        draw();
        return;
    };
    let eye = draw_index();
    let previous = ACTIVE.replace(Some(Atlas {
        id,
        frame,
        eye,
        extent: None,
        owners: Vec::new(),
        before: None,
        draws: 0,
        error: None,
    }));
    draw();
    if let Some(atlas) = ACTIVE.replace(previous) {
        atlas.finish();
    }
}

pub(crate) fn before(offset: u32) -> bool {
    ACTIVE.with_borrow_mut(|active| {
        let Some(a) = active.as_mut().filter(|a| a.error.is_none()) else {
            return false;
        };
        a.draws += 1;
        let result = (|| {
            ensure!(
                a.draws <= 256 && a.before.is_none(),
                "draw count exceeded or nested submission"
            );
            a.before = Some(a.depth()?);
            Ok(())
        })();
        if let Err(e) = result {
            a.error = Some(format!(
                "patch owners: draw {} offset {offset}: {e:#}",
                a.draws
            ));
            return false;
        }
        true
    })
}

pub(crate) fn after(offset: u32) {
    ACTIVE.with_borrow_mut(|active| {
        let Some(a) = active.as_mut() else { return };
        let result = (|| {
            let before = a.before.take().context("missing before image")?;
            let after = a.depth()?;
            let mut changes = 0;
            for ((b,e), owner) in before.as_chunks::<8>().0.iter().zip(after.as_chunks::<8>().0).zip(&mut a.owners) {
                if b[..4] != e[..4] { *owner = a.draws as u32; changes += 1; }
            }
            tracing::info!(target:"patch_owner",id=%a.id,frame=a.frame,eye=a.eye,ordinal=a.draws,offset,changes,"native depth changes");
            Ok::<_,anyhow::Error>(())
        })();
        if let Err(e) = result { a.error = Some(format!("patch owners: draw {} offset {offset}: {e:#}",a.draws)); }
    });
}

impl Atlas {
    fn depth(&mut self) -> Result<Vec<u8>> {
        let mut record = empty_stage_record(self.frame, self.eye, Stage::GeometryDepth);
        let engine = EngineContext::get().context("graphics context unavailable")?;
        let raw = engine.with_lock(|ctx| {
            let ge = unsafe { GraphicsEngine::get() }.context("graphics engine unavailable")?;
            let depth =
                unsafe { ge.m_MainDepthTexture.as_ref() }.context("missing depth texture")?;
            read_stage(ctx, depth, &mut record)
        })?;
        let src = record.source.context("missing depth descriptor")?;
        let extent = (src.width, src.height);
        ensure!(
            src.format_id == DXGI_FORMAT_R32G8X24_TYPELESS.0
                && record.error.is_none()
                && self.extent.get_or_insert(extent) == &extent,
            "unexpected depth format, size or readback error"
        );
        let StageArtifact::Raw(bytes) = raw else {
            anyhow::bail!("missing raw depth plane");
        };
        self.owners.resize(extent.0 as usize * extent.1 as usize, 0);
        ensure!(
            bytes.len() == self.owners.len() * 8,
            "unexpected depth byte count"
        );
        Ok(bytes)
    }

    fn finish(self) {
        let mut state = STATE.lock();
        if state.frame != Some(self.frame) {
            return;
        }
        let mut record = empty_stage_record(self.frame, self.eye, Stage::PatchDepthOwners);
        record.error = self.error;
        if let Some((width, height)) = self.extent {
            record.source = Some(Source {
                width,
                height,
                format: format!("{:?}", DXGI_FORMAT_R32_UINT),
                format_id: DXGI_FORMAT_R32_UINT.0,
                bytes_per_pixel: Some(4),
            });
            let index = state.records.len();
            state.images.push(StageImage {
                record: index,
                artifact: StageArtifact::Raw(
                    self.owners.into_iter().flat_map(u32::to_le_bytes).collect(),
                ),
            });
        } else {
            record
                .error
                .get_or_insert_with(|| "patch owners: no depth observations".into());
        }
        state.records.push(record);
    }
}
