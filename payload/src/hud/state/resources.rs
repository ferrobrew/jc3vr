//! Render-owner surface replacement and deferred renderer registration retirement.

use std::time::{Duration, Instant};

use jc3gi::graphics_engine::device::Device;

use crate::hud::{binding, state::AppliedRedirect, target::HudTarget};

pub(super) trait Adapter {
    type Target;
    fn size(target: &Self::Target) -> (u32, u32);
    fn create(&mut self, size: (u32, u32)) -> anyhow::Result<Self::Target>;
    fn matches(&mut self, size: (u32, u32)) -> bool;
    fn bind(&mut self, target: &Self::Target, requested: AppliedRedirect) -> bool;
    fn restore(&mut self, render: (u32, u32));
}

pub(super) struct Native<'a>(pub &'a Device);

impl Adapter for Native<'_> {
    type Target = HudTarget;
    fn size(target: &HudTarget) -> (u32, u32) {
        target.size()
    }
    fn create(&mut self, size: (u32, u32)) -> anyhow::Result<HudTarget> {
        HudTarget::new(self.0, size.0, size.1)
    }
    fn matches(&mut self, size: (u32, u32)) -> bool {
        binding::movie_viewport_matches(size.0, size.1)
    }
    fn bind(&mut self, target: &HudTarget, requested: AppliedRedirect) -> bool {
        binding::redirect_to(
            target,
            requested.texture.0,
            requested.texture.1,
            requested.stage_safe_area,
        )
    }
    fn restore(&mut self, render: (u32, u32)) {
        binding::restore_engine_binding(render.0, render.1);
    }
}

pub(super) struct Surface<'a, T> {
    pub target: &'a mut Option<T>,
    pub redirected: &'a mut bool,
    pub render_size: &'a mut Option<(u32, u32)>,
    pub stage_safe_area: &'a mut bool,
    pub retry: &'a mut Retry,
}

/// Returns whether the owned target changed, so its old preview can be retired.
pub(super) fn redirect<A: Adapter>(
    surface: Surface<'_, A::Target>,
    native: &mut A,
    requested: AppliedRedirect,
    render: (u32, u32),
    now: Instant,
) -> bool {
    let old_size = surface.target.as_ref().map(A::size);
    let old_valid = *surface.redirected
        && *surface.render_size == Some(render)
        && old_size.is_some_and(|size| native.matches(size));
    if !old_valid {
        if *surface.redirected {
            native.restore(render);
        }
        *surface.redirected = false;
    }
    if !surface.retry.ready(requested.texture, now) {
        return false;
    }
    if let Err(error) =
        crate::config::validate::hud::dimension("HUD target width", requested.texture.0).and_then(
            |()| crate::config::validate::hud::dimension("HUD target height", requested.texture.1),
        )
    {
        surface.retry.failed(requested.texture, now, || {
            tracing::error!("HUD target: {error}")
        });
        return false;
    }
    let replacing = old_size != Some(requested.texture);
    let candidate = if replacing {
        match native.create(requested.texture) {
            Ok(target) => Some(target),
            Err(error) => {
                surface.retry.failed(requested.texture, now, || {
                    tracing::error!("HUD target: {error:#}")
                });
                if !old_valid {
                    native.restore(render);
                }
                return false;
            }
        }
    } else {
        None
    };
    if !replacing && old_valid && *surface.stage_safe_area == requested.stage_safe_area {
        return false;
    }
    let Some(target) = candidate.as_ref().or(surface.target.as_ref()) else {
        return false;
    };
    if !native.bind(target, requested) {
        native.restore(render);
        *surface.redirected = false;
        surface.retry.failed(requested.texture, now, || {
            tracing::warn!("HUD redirect: waiting for a usable movie render buffer")
        });
        return false;
    }
    if let Some(candidate) = candidate {
        *surface.target = Some(candidate);
    }
    *surface.redirected = true;
    *surface.render_size = Some(render);
    *surface.stage_safe_area = requested.stage_safe_area;
    surface.retry.succeeded();
    replacing
}

/// Build every missing layer before replacing any live layer.
pub(super) fn layers<A: Adapter>(
    targets: &mut [Option<A::Target>; 2],
    native: &mut A,
    size: (u32, u32),
    retry: &mut Retry,
    now: Instant,
) -> [bool; 2] {
    if !retry.ready(size, now) {
        return [false; 2];
    }
    let mut candidates = [None, None];
    for (target, candidate) in targets.iter().zip(&mut candidates) {
        if target.as_ref().map(A::size) != Some(size) {
            match native.create(size) {
                Ok(target) => *candidate = Some(target),
                Err(error) => {
                    retry.failed(size, now, || tracing::error!("HUD split layer: {error:#}"));
                    return [false; 2];
                }
            }
        }
    }
    let mut changed = [false; 2];
    for ((target, candidate), changed) in targets.iter_mut().zip(candidates).zip(&mut changed) {
        if let Some(candidate) = candidate {
            *target = Some(candidate);
            *changed = true;
        }
    }
    retry.succeeded();
    changed
}

pub(super) struct Retry {
    failed: Option<((u32, u32), Instant)>,
}

impl Retry {
    pub const fn new() -> Self {
        Self { failed: None }
    }
    pub fn ready(&self, size: (u32, u32), now: Instant) -> bool {
        self.failed.is_none_or(|(failed, at)| {
            failed != size || now.saturating_duration_since(at) >= Duration::from_secs(1)
        })
    }
    pub fn failed(&mut self, size: (u32, u32), now: Instant, log: impl FnOnce()) {
        if self.failed.is_none_or(|(failed, _)| failed != size) {
            log();
        }
        self.failed = Some((size, now));
    }
    pub fn succeeded(&mut self) {
        self.failed = None;
    }
}

pub(super) fn retire(id: &mut Option<egui::TextureId>, pending: &mut Vec<egui::TextureId>) {
    if let Some(id) = id.take() {
        pending.push(id);
    }
}

pub(super) fn drain(
    pending: &mut Vec<egui::TextureId>,
    mut unregister: impl FnMut(egui::TextureId),
) {
    for id in pending.drain(..) {
        unregister(id);
    }
}

#[cfg(test)]
mod tests;
