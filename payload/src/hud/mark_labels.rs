//! The grip markers' labels: a short name drawn upright above a labelled marker
//! ([`crate::controllers::grab::Mark::label`]), such as the cockpit's nitrous button, while a hand
//! is at it. The overlay that draws the markers has no text, so the HUD draws each label as a quad
//! of the grab hint's haloed text ([`crate::hud::grab_hint`]), with no backing.
//!
//! The grip markers publish the labels at the input tick's rate with each marker's anchor
//! ([`publish`]); the draw places them at render time from the same anchors the overlay resolves,
//! so a label rides with its marker.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use anyhow::Context as _;
use glam::{Quat, Vec3, Vec4};
use jc3gi::graphics_engine::{device::Device, texture::Texture};
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext;

use crate::{
    config::Config,
    controllers::{
        overlay::{
            primitive::Anchor,
            render_poses,
            resolve::{HandPoses, anchor_position},
        },
        scheme,
    },
    hud::{
        anchored::place::upright_corners,
        grab_hint::{TAG_TEXELS, paint_tag},
        log_once::LogOnce,
        quad::HudQuad,
        target::HudTarget,
        world_quads_active,
    },
};

/// One marker's label as the grip markers publish it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct MarkLabel {
    pub text: &'static str,
    /// Where the marker is drawn.
    pub at: Anchor,
    /// How far above the marker's centre its edge is, metres: the label sits above that.
    pub clearance: f32,
}

/// Publish the labels to draw. Called each input tick of the grip markers.
pub(crate) fn publish(labels: Vec<MarkLabel>) {
    *SAMPLE.lock() = Some((labels, Instant::now()));
}

/// Draw the labels for `eye` over `target`. Called from the HUD's draw, with the engine context
/// mutex held.
pub(crate) fn draw(context: &ID3D11DeviceContext, device: &Device, target: &Texture, eye: usize) {
    let mut state = STATE.lock();
    if eye == 0 {
        let cfg = Config::lock_query(|c| c.hud);
        let active = world_quads_active(&cfg) && scheme::is_controllers();
        let head = active.then(crate::hud::render_camera_pose).flatten();
        state.prepare(context, device, head);
    }
    state.draw(context, device, target);
}

/// The corners of `label`'s quad, seen from `head` (with `head_up`, the head's up), with the
/// hands' render-time poses `hands`: upright and facing the eye, [`HEIGHT_M`] tall, its lower edge
/// [`GAP_M`] above the marker's edge. `None` when the anchor's hand has no pose.
pub(crate) fn place(
    label: &MarkLabel,
    head: Vec3,
    head_up: Vec3,
    hands: &[Option<HandPoses>; 2],
) -> Option<[Vec3; 4]> {
    let at = match label.at {
        Anchor::Head(offset) => head + offset,
        anchor => anchor_position(anchor, hands)?,
    };
    let width = HEIGHT_M * TAG_TEXELS.0 as f32 / TAG_TEXELS.1 as f32;
    let centre = at + Vec3::Y * (label.clearance.max(0.0) + GAP_M + HEIGHT_M * 0.5);
    centre
        .is_finite()
        .then(|| upright_corners(centre, head, head_up, width, HEIGHT_M))
        .flatten()
}

/// A label's height, metres.
const HEIGHT_M: f32 = 0.03;

/// The gap between the marker's edge and the label's lower edge, metres.
const GAP_M: f32 = 0.01;

/// How long a published list stays current without a newer one.
const STALE: Duration = Duration::from_millis(250);

/// The most distinct labels kept as textures.
const MAX_TEXTURES: usize = 8;

static SAMPLE: Mutex<Option<(Vec<MarkLabel>, Instant)>> = Mutex::new(None);

static STATE: Mutex<State> = Mutex::new(State {
    placed: Vec::new(),
    gpu: None,
});

struct State {
    placed: Vec<(&'static str, [Vec3; 4])>,
    gpu: Option<Gpu>,
}

/// The quad pass and a painted texture per label, created on the first frame with a label.
struct Gpu {
    quad: HudQuad,
    textures: HashMap<&'static str, HudTarget>,
}

impl State {
    /// Eye 0: place the labels seen from `head`, and paint the ones not painted yet.
    fn prepare(
        &mut self,
        context: &ID3D11DeviceContext,
        device: &Device,
        head: Option<(Vec3, Quat)>,
    ) {
        self.placed.clear();
        let Some((head, rotation)) = head else {
            return;
        };
        let now = Instant::now();
        let labels = SAMPLE
            .lock()
            .as_ref()
            .filter(|(_, at)| now.duration_since(*at) <= STALE)
            .map(|(labels, _)| labels.clone())
            .unwrap_or_default();
        if labels.is_empty() {
            return;
        }
        if self.gpu.is_none() {
            match HudQuad::new(device) {
                Ok(quad) => {
                    self.gpu = Some(Gpu {
                        quad,
                        textures: HashMap::new(),
                    })
                }
                Err(e) => {
                    FAILURES.error(&e);
                    return;
                }
            }
        }
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };
        let hands = render_poses();
        for label in &labels {
            let Some(corners) = place(label, head, rotation * Vec3::Y, &hands) else {
                continue;
            };
            if !gpu.textures.contains_key(label.text) {
                if gpu.textures.len() >= MAX_TEXTURES {
                    continue;
                }
                match paint(context, device, &paint_tag(label.text)) {
                    Ok(texture) => {
                        gpu.textures.insert(label.text, texture);
                    }
                    Err(e) => {
                        FAILURES.error(&e);
                        continue;
                    }
                }
            }
            self.placed.push((label.text, corners));
        }
    }

    fn draw(&self, context: &ID3D11DeviceContext, device: &Device, target: &Texture) {
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };
        for (text, corners) in &self.placed {
            let Some(texture) = gpu.textures.get(text) else {
                continue;
            };
            let corners: [Vec4; 4] = corners.map(|c| c.extend(1.0));
            gpu.quad
                .draw_faded(context, device, target, texture.color_srv(), &corners, 1.0);
        }
    }
}

/// A texture of [`TAG_TEXELS`] holding `texels`.
fn paint(
    context: &ID3D11DeviceContext,
    device: &Device,
    texels: &[u8],
) -> anyhow::Result<HudTarget> {
    let (width, height) = TAG_TEXELS;
    let target =
        HudTarget::new(device, width, height).context("creating a marker label's texture")?;
    target.upload(context, texels);
    Ok(target)
}

static FAILURES: LogOnce = LogOnce::new("hud marker labels");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::{frame::Hand, overlay::primitive::HandPose};

    fn label(at: Anchor) -> MarkLabel {
        MarkLabel {
            text: "Nitrous",
            at,
            clearance: 0.045,
        }
    }

    fn centre(corners: &[Vec3; 4]) -> Vec3 {
        corners.iter().sum::<Vec3>() * 0.25
    }

    /// A head-anchored label rides the render-time head, above its marker, upright and facing the
    /// eye.
    #[test]
    fn a_label_sits_above_its_marker() {
        let head = Vec3::new(5.0, 1.7, 2.0);
        let offset = Vec3::new(0.3, -0.4, -0.5);
        let corners = place(&label(Anchor::Head(offset)), head, Vec3::Y, &[None; 2]).unwrap();
        let expected = head + offset + Vec3::Y * (0.045 + GAP_M + HEIGHT_M * 0.5);
        assert!(centre(&corners).abs_diff_eq(expected, 1e-5));
        let [tl, tr, bl, _] = corners;
        assert!((tl.y - tr.y).abs() < 1e-6, "level");
        assert!(((tl - bl).length() - HEIGHT_M).abs() < 1e-5);
    }

    #[test]
    fn a_label_on_a_hand_without_a_pose_is_not_placed() {
        let at = Anchor::hand(Hand::Left, HandPose::Grip);
        assert_eq!(place(&label(at), Vec3::ZERO, Vec3::Y, &[None; 2]), None);
        let world = place(
            &label(Anchor::World(Vec3::new(0.0, 0.0, -1.0))),
            Vec3::ZERO,
            Vec3::Y,
            &[None; 2],
        );
        assert!(world.is_some());
    }
}
