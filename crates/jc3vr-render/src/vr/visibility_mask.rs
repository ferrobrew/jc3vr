//! The runtime's hidden-area mesh per eye (`XR_KHR_visibility_mask`): queried, logged, and shown in
//! the debug UI. Nothing is masked with it yet.
//!
//! The extension is enabled whenever the runtime offers it (`crate::vr::state`). The hidden
//! triangle mesh of each eye is fetched on the first located frame of a session and again after a
//! `XrEventDataVisibilityMaskChangedKHR` ([`mark_dirty`]). The spec places the mesh's vertices in
//! the view's own space on the plane `z = -1`, so a vertex's `x` and `y` are the tangents of its
//! horizontal and vertical angles from the view axis, the same space the tangents of the view's
//! `XrFovf` angles bound: the field-of-view rectangle is `[tan(left), tan(right)]` by
//! `[tan(down), tan(up)]`. The logged area is the mesh's triangle area over that rectangle's.

use std::sync::atomic::{AtomicBool, Ordering};

use openxr as xr;
use parking_lot::Mutex;

/// One eye's hidden-area mesh and the field of view it was fetched against.
#[derive(Clone, Debug, Default)]
pub(crate) struct EyeMask {
    /// The vertices on the `z = -1` plane: tangents of the view angles.
    pub(crate) vertices: Vec<[f32; 2]>,
    /// Three indices per hidden triangle.
    pub(crate) indices: Vec<u32>,
    /// The field-of-view rectangle in the same space: left, right, down, and up tangents.
    pub(crate) fov_tangents: [f32; 4],
}

impl EyeMask {
    /// The hidden triangles' total area as a fraction of the field-of-view rectangle's.
    pub(crate) fn hidden_fraction(&self) -> f32 {
        let [left, right, down, up] = self.fov_tangents;
        let rect = (right - left) * (up - down);
        if rect <= 0.0 {
            return 0.0;
        }
        hidden_area(&self.vertices, &self.indices) / rect
    }
}

/// What the runtime returned, for the debug UI.
#[derive(Clone, Debug, Default)]
pub(crate) enum MaskStatus {
    /// No session has located a frame yet.
    #[default]
    NotFetched,
    /// The runtime does not offer `XR_KHR_visibility_mask`, or the instance predates enabling it.
    Unsupported,
    /// The query failed.
    Failed(String),
    /// Each eye's mesh.
    Fetched(Box<[EyeMask; 2]>),
}

/// Record whether the instance has the extension enabled; called when the runtime comes up.
pub(super) fn set_supported(supported: bool) {
    SUPPORTED.store(supported, Ordering::Relaxed);
    mark_dirty();
}

/// Ask for the meshes to be fetched again on the next located frame: a new session, or the
/// runtime's visibility-changed event.
pub(super) fn mark_dirty() {
    DIRTY.store(true, Ordering::Relaxed);
}

/// Fetch and log the meshes when they are due, against the located views' fields of view.
pub(super) fn refresh(session: &xr::Session<xr::D3D11>, fovs: [xr::Fovf; 2]) {
    if !DIRTY.swap(false, Ordering::Relaxed) {
        return;
    }
    let status = if SUPPORTED.load(Ordering::Relaxed) {
        fetch(session, fovs)
    } else {
        MaskStatus::Unsupported
    };
    match &status {
        MaskStatus::Fetched(masks) => {
            for (eye, mask) in masks.iter().enumerate() {
                tracing::info!(
                    target: "vr",
                    eye,
                    vertices = mask.vertices.len(),
                    triangles = mask.indices.len() / 3,
                    hidden_fraction = mask.hidden_fraction(),
                    "visibility mask: the runtime's hidden-area mesh for this eye, and its area \
                     over the field-of-view rectangle",
                );
            }
        }
        MaskStatus::Unsupported => {
            tracing::info!(target: "vr", "visibility mask: the runtime does not offer XR_KHR_visibility_mask");
        }
        MaskStatus::Failed(error) => {
            tracing::warn!(target: "vr", "visibility mask: the query failed: {error}");
        }
        MaskStatus::NotFetched => {}
    }
    *STATUS.lock() = status;
}

/// The last fetch's result.
pub(crate) fn status() -> MaskStatus {
    STATUS.lock().clone()
}

static SUPPORTED: AtomicBool = AtomicBool::new(false);
static DIRTY: AtomicBool = AtomicBool::new(true);
static STATUS: Mutex<MaskStatus> = Mutex::new(MaskStatus::NotFetched);

fn fetch(session: &xr::Session<xr::D3D11>, fovs: [xr::Fovf; 2]) -> MaskStatus {
    let mut masks: [EyeMask; 2] = Default::default();
    for (eye, (mask, fov)) in masks.iter_mut().zip(fovs).enumerate() {
        let mesh = match session.get_visibility_mask_khr(
            xr::ViewConfigurationType::PRIMARY_STEREO,
            eye as u32,
            xr::VisibilityMaskTypeKHR::HIDDEN_TRIANGLE_MESH,
        ) {
            Ok(mesh) => mesh,
            Err(error) => return MaskStatus::Failed(format!("eye {eye}: {error}")),
        };
        *mask = EyeMask {
            vertices: mesh.vertices.iter().map(|v| [v.x, v.y]).collect(),
            indices: mesh.indices,
            fov_tangents: [
                fov.angle_left.tan(),
                fov.angle_right.tan(),
                fov.angle_down.tan(),
                fov.angle_up.tan(),
            ],
        };
    }
    MaskStatus::Fetched(Box::new(masks))
}

/// The summed area of the indexed triangles; an index out of range drops its triangle.
fn hidden_area(vertices: &[[f32; 2]], indices: &[u32]) -> f32 {
    indices
        .as_chunks::<3>()
        .0
        .iter()
        .filter_map(|triangle| {
            let [a, b, c] = triangle.map(|index| vertices.get(index as usize).copied());
            let (a, b, c) = (a?, b?, c?);
            Some(0.5 * ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs())
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hidden_corner_is_its_share_of_the_rectangle() {
        // A unit field of view (tangents -1..1 each way) with its lower-left quarter hidden as two
        // triangles: a quarter of the rectangle.
        let mask = EyeMask {
            vertices: vec![[-1.0, -1.0], [0.0, -1.0], [0.0, 0.0], [-1.0, 0.0]],
            indices: vec![0, 1, 2, 0, 2, 3],
            fov_tangents: [-1.0, 1.0, -1.0, 1.0],
        };
        assert!((mask.hidden_fraction() - 0.25).abs() < 1e-6);
    }

    #[test]
    fn an_index_out_of_range_drops_its_triangle() {
        assert_eq!(hidden_area(&[[0.0, 0.0], [1.0, 0.0]], &[0, 1, 2]), 0.0);
    }
}
