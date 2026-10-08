//! The scene depth at the world markers' pixels: a compute pass
//! ([`marker_depth_cs`](../../shaders/marker_depth_cs.hlsl)) reads the main depth buffer at each
//! marker's projected pixel on eye 0, at the end of the eye's draw, and the results read back
//! asynchronously through a small staging ring, never stalling the GPU. A result arrives a couple of
//! frames after its dispatch; the placement's smoothing absorbs the delay.
//!
//! The depth is linear view depth along the eye's forward axis, and so is the marker's own clip
//! `w`, so their ratio is the fraction of the way to the marker at which the line of sight meets a
//! surface, whatever the angle off-axis.

use std::time::Instant;

use anyhow::Context as _;
use glam::{Mat4, Vec3};
use jc3gi::graphics_engine::{device::Device, graphics_engine::GraphicsEngine};
use windows::Win32::Graphics::{
    Direct3D11::{
        D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_UNORDERED_ACCESS, D3D11_BUFFER_DESC,
        D3D11_BUFFER_UAV, D3D11_CPU_ACCESS_READ, D3D11_CPU_ACCESS_WRITE,
        D3D11_MAP_FLAG_DO_NOT_WAIT, D3D11_MAP_READ, D3D11_MAP_WRITE_DISCARD,
        D3D11_MAPPED_SUBRESOURCE, D3D11_UAV_DIMENSION_BUFFER, D3D11_UNORDERED_ACCESS_VIEW_DESC,
        D3D11_UNORDERED_ACCESS_VIEW_DESC_0, D3D11_USAGE_DYNAMIC, D3D11_USAGE_STAGING,
        D3D11_VIEWPORT, ID3D11Buffer, ID3D11ComputeShader, ID3D11DepthStencilView,
        ID3D11DeviceContext, ID3D11RenderTargetView, ID3D11UnorderedAccessView,
    },
    Dxgi::Common::DXGI_FORMAT_R32_FLOAT,
};

use crate::hud::world_markers::layout::Sight;

/// One point to sample: a marker id and its world point.
#[derive(Clone, Copy)]
pub(super) struct DepthPoint {
    pub id: u32,
    pub world: Vec3,
    /// The distance from the head to the point, in metres.
    pub true_distance: f32,
}

/// The depth probe: the compute pipeline and its readback ring.
pub(super) struct DepthProbe {
    shader: ID3D11ComputeShader,
    params: ID3D11Buffer,
    result: ID3D11Buffer,
    result_uav: ID3D11UnorderedAccessView,
    ring: [Slot; RING_DEPTH],
    frame: usize,
    /// How many times [`DepthProbe::sample`] has run: once a frame while the markers show.
    calls: u64,
}

impl DepthProbe {
    /// Build the compute pipeline and the readback ring.
    pub fn new(device: &Device) -> anyhow::Result<Self> {
        let d3d = &device.m_Device;
        // SAFETY: `d3d` is the live engine device; the descriptors are valid for these calls.
        unsafe {
            let mut shader: Option<ID3D11ComputeShader> = None;
            d3d.CreateComputeShader(COMPUTE_DXBC, None, Some(&mut shader))
                .context("hud world markers: creating the depth-probe compute shader")?;
            let shader =
                shader.context("hud world markers: the depth-probe shader was not created")?;

            let mut params: Option<ID3D11Buffer> = None;
            d3d.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: size_of::<Params>() as u32,
                    Usage: D3D11_USAGE_DYNAMIC,
                    BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                    CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                    ..Default::default()
                },
                None,
                Some(&mut params),
            )
            .context("hud world markers: creating the depth-probe constant buffer")?;
            let params = params
                .context("hud world markers: the depth-probe constant buffer was not created")?;

            let mut result: Option<ID3D11Buffer> = None;
            d3d.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: (MAX_POINTS * 4) as u32,
                    BindFlags: D3D11_BIND_UNORDERED_ACCESS.0 as u32,
                    ..Default::default()
                },
                None,
                Some(&mut result),
            )
            .context("hud world markers: creating the depth-probe result buffer")?;
            let result = result
                .context("hud world markers: the depth-probe result buffer was not created")?;

            let mut result_uav: Option<ID3D11UnorderedAccessView> = None;
            d3d.CreateUnorderedAccessView(
                &result,
                Some(&D3D11_UNORDERED_ACCESS_VIEW_DESC {
                    Format: DXGI_FORMAT_R32_FLOAT,
                    ViewDimension: D3D11_UAV_DIMENSION_BUFFER,
                    Anonymous: D3D11_UNORDERED_ACCESS_VIEW_DESC_0 {
                        Buffer: D3D11_BUFFER_UAV {
                            FirstElement: 0,
                            NumElements: MAX_POINTS as u32,
                            Flags: 0,
                        },
                    },
                }),
                Some(&mut result_uav),
            )
            .context("hud world markers: creating the depth-probe UAV")?;
            let result_uav =
                result_uav.context("hud world markers: the depth-probe UAV was not created")?;

            let staging = (0..RING_DEPTH)
                .map(|_| {
                    let mut buffer: Option<ID3D11Buffer> = None;
                    d3d.CreateBuffer(
                        &D3D11_BUFFER_DESC {
                            ByteWidth: (MAX_POINTS * 4) as u32,
                            Usage: D3D11_USAGE_STAGING,
                            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                            ..Default::default()
                        },
                        None,
                        Some(&mut buffer),
                    )
                    .ok()
                    .and(buffer)
                })
                .collect::<Option<Vec<_>>>()
                .context("hud world markers: creating the depth-probe staging ring")?;
            let ring: [Slot; RING_DEPTH] = staging
                .into_iter()
                .map(|buffer| Slot {
                    buffer,
                    points: Vec::new(),
                    in_flight: false,
                    dispatched: 0,
                })
                .collect::<Vec<_>>()
                .try_into()
                .map_err(|_| {
                    anyhow::anyhow!("hud world markers: the staging ring has the wrong size")
                })?;

            Ok(Self {
                shader,
                params,
                result,
                result_uav,
                ring,
                frame: 0,
                calls: 0,
            })
        }
    }

    /// Dispatch this frame's samples for `points` through the eye's view-projection `vp` and
    /// `viewport` (the region of the depth buffer the eye was drawn into), and return the results
    /// of the oldest completed dispatch as `(id, sight, when)`. Call once per frame on eye 0, at
    /// the end of the eye's draw, with the engine context mutex held.
    pub fn sample(
        &mut self,
        context: &ID3D11DeviceContext,
        graphics_engine: &GraphicsEngine,
        vp: Mat4,
        viewport: Option<D3D11_VIEWPORT>,
        points: &[DepthPoint],
    ) -> Vec<(u32, Sight)> {
        self.calls += 1;
        // SAFETY: the depth texture and its SRV belong to the live graphics engine; the buffers
        // are ours; the context is the engine's immediate context under its mutex.
        unsafe {
            if let Some(depth) = graphics_engine.m_MainDepthTexture.as_ref()
                && let Some((proj_a, proj_b)) = crate::hud::depth::projection_z_row()
            {
                let dims = (f32::from(depth.m_Width), f32::from(depth.m_Height));
                let viewport = viewport.unwrap_or(D3D11_VIEWPORT {
                    TopLeftX: 0.0,
                    TopLeftY: 0.0,
                    Width: dims.0,
                    Height: dims.1,
                    MinDepth: 0.0,
                    MaxDepth: 1.0,
                });
                let mut queued = Vec::with_capacity(points.len().min(MAX_POINTS));
                let mut pixels = [[0.0f32; 4]; MAX_POINTS];
                for point in points {
                    if queued.len() >= MAX_POINTS {
                        break;
                    }
                    let clip = vp * point.world.extend(1.0);
                    if clip.w <= f32::EPSILON {
                        continue;
                    }
                    let ndc = clip.truncate() / clip.w;
                    pixels[queued.len()] = [
                        viewport.TopLeftX + (ndc.x * 0.5 + 0.5) * viewport.Width,
                        viewport.TopLeftY + (0.5 - ndc.y * 0.5) * viewport.Height,
                        0.0,
                        0.0,
                    ];
                    queued.push(Queued {
                        id: point.id,
                        clip_w: clip.w,
                        true_distance: point.true_distance,
                    });
                }
                if dims.0 >= 1.0 && dims.1 >= 1.0 && !queued.is_empty() {
                    self.dispatch(
                        context,
                        depth,
                        Params {
                            depth_dims: [dims.0, dims.1],
                            proj_a,
                            proj_b,
                            count: queued.len() as u32,
                            spread: SPREAD_PIXELS,
                            _pad: [0.0; 2],
                            points: pixels,
                        },
                        queued,
                    );
                }
            }
            self.read_back(context)
        }
    }

    /// Upload, dispatch, and queue the copy into this frame's staging slot.
    ///
    /// # Safety
    /// As [`sample`](Self::sample).
    unsafe fn dispatch(
        &mut self,
        context: &ID3D11DeviceContext,
        depth: &jc3gi::graphics_engine::texture::Texture,
        params: Params,
        queued: Vec<Queued>,
    ) {
        unsafe {
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            if context
                .Map(
                    &self.params,
                    0,
                    D3D11_MAP_WRITE_DISCARD,
                    0,
                    Some(&mut mapped),
                )
                .is_err()
            {
                return;
            }
            (mapped.pData as *mut Params).write(params);
            context.Unmap(&self.params, 0);

            // The depth texture may still be bound as the depth target, and D3D refuses a
            // conflicting SRV bind; detach the output merger for the dispatch, as the histogram
            // pass does.
            let mut saved_rtvs: [Option<ID3D11RenderTargetView>; 8] = Default::default();
            let mut saved_dsv: Option<ID3D11DepthStencilView> = None;
            context.OMGetRenderTargets(Some(&mut saved_rtvs), Some(&mut saved_dsv));
            context.OMSetRenderTargets(None, None);

            context.CSSetShader(&self.shader, None);
            context.CSSetConstantBuffers(0, Some(&[Some(self.params.clone())]));
            context.CSSetShaderResources(0, Some(&[Some(depth.m_SRV.clone())]));
            context.CSSetUnorderedAccessViews(0, 1, Some(&Some(self.result_uav.clone())), None);
            context.Dispatch(1, 1, 1);
            context.CSSetShaderResources(0, Some(&[None]));
            context.CSSetUnorderedAccessViews(0, 1, Some(&None), None);
            context.CSSetShader(None, None);
            context.OMSetRenderTargets(Some(&saved_rtvs), saved_dsv.as_ref());

            let slot = &mut self.ring[self.frame % RING_DEPTH];
            context.CopyResource(&slot.buffer, &self.result);
            slot.points = queued;
            slot.in_flight = true;
            slot.dispatched = self.calls;
            self.frame += 1;
        }
    }

    /// Pick up the oldest in-flight readback without waiting.
    ///
    /// # Safety
    /// As [`sample`](Self::sample).
    unsafe fn read_back(&mut self, context: &ID3D11DeviceContext) -> Vec<(u32, Sight)> {
        let slot = &mut self.ring[self.frame % RING_DEPTH];
        if !slot.in_flight {
            return Vec::new();
        }
        // After a gap in the dispatches (no markers, or the probe not reached for a while) the
        // oldest slot can hold a dispatch from long ago, which says nothing about the scene now.
        if is_stale(slot.dispatched, self.calls) {
            slot.in_flight = false;
            slot.points.clear();
            return Vec::new();
        }
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: the staging buffer is ours; DO_NOT_WAIT never stalls on the GPU.
        let ready = unsafe {
            context
                .Map(
                    &slot.buffer,
                    0,
                    D3D11_MAP_READ,
                    D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32,
                    Some(&mut mapped),
                )
                .is_ok()
        };
        if !ready {
            return Vec::new();
        }
        let mut depths = [0.0f32; MAX_POINTS];
        // SAFETY: the mapped staging buffer holds MAX_POINTS floats.
        unsafe {
            (mapped.pData as *const f32).copy_to(depths.as_mut_ptr(), MAX_POINTS);
            context.Unmap(&slot.buffer, 0);
        }
        slot.in_flight = false;
        slot.points
            .drain(..)
            .zip(depths)
            .map(|(point, depth)| (point.id, sight(depth, point.clip_w, point.true_distance)))
            .collect()
    }
}

/// The time a probe result stays usable, after which the marker falls back to the raycast.
pub(super) const RESULT_TTL_S: f32 = 0.25;

/// A result together with when it was read back.
#[derive(Clone, Copy)]
pub(super) struct Timed {
    pub sight: Sight,
    pub at: Instant,
}

/// The committed, precompiled probe shader (entry point `main`).
const COMPUTE_DXBC: &[u8] = include_bytes!("../../shaders/marker_depth_cs.dxbc");

/// The shader's point capacity (matches `MAX_POINTS` in the HLSL).
const MAX_POINTS: usize = 32;

/// The staging-ring depth: reads happen this many frames behind the dispatch.
const RING_DEPTH: usize = 3;

/// The oldest a readback may be, in frames since its dispatch, before it is discarded: the ring's
/// depth, and a couple of frames for a GPU running behind.
const MAX_RESULT_AGE_CALLS: u64 = RING_DEPTH as u64 + 2;

/// The probe footprint's neighbour spacing, in depth-buffer pixels.
const SPREAD_PIXELS: f32 = 2.0;

/// How much nearer than the marker a sampled surface must be to count as blocking its line of
/// sight, as a fraction of the marker's depth: a surface at the marker (the object it is attached
/// to) does not move it.
const BLOCK_FRACTION: f32 = 0.97;

/// Constant buffer, matching the HLSL `Params`.
#[repr(C)]
struct Params {
    depth_dims: [f32; 2],
    proj_a: f32,
    proj_b: f32,
    count: u32,
    spread: f32,
    _pad: [f32; 2],
    points: [[f32; 4]; MAX_POINTS],
}

/// One sampled point awaiting its readback.
struct Queued {
    id: u32,
    clip_w: f32,
    true_distance: f32,
}

/// One staging slot and the points its copy holds.
struct Slot {
    buffer: ID3D11Buffer,
    points: Vec<Queued>,
    in_flight: bool,
    /// The [`DepthProbe::calls`] count of the dispatch whose copy the slot holds.
    dispatched: u64,
}

/// Whether a dispatch made on call `dispatched` is too old to use on call `now`.
fn is_stale(dispatched: u64, now: u64) -> bool {
    now.saturating_sub(dispatched) > MAX_RESULT_AGE_CALLS
}

/// What a sampled view depth says about a marker at clip depth `clip_w`, `true_distance` away.
fn sight(depth: f32, clip_w: f32, true_distance: f32) -> Sight {
    let ratio = depth / clip_w;
    if ratio.is_finite() && ratio > 0.0 && ratio < BLOCK_FRACTION {
        Sight::Surface(ratio * true_distance)
    } else {
        Sight::Clear
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_readback_from_before_a_gap_is_stale() {
        assert!(!is_stale(10, 10 + RING_DEPTH as u64));
        assert!(!is_stale(10, 10 + MAX_RESULT_AGE_CALLS));
        assert!(is_stale(10, 11 + MAX_RESULT_AGE_CALLS));
        assert!(!is_stale(10, 5));
    }

    #[test]
    fn sight_from_depth() {
        // A wall halfway to a marker 100 m away.
        assert_eq!(sight(25.0, 50.0, 100.0), Sight::Surface(50.0));
        // The surface at the marker itself does not move it.
        assert_eq!(sight(49.5, 50.0, 100.0), Sight::Clear);
        // Sky (the far clamp) and degenerate samples are clear.
        assert_eq!(sight(100_000.0, 50.0, 100.0), Sight::Clear);
        assert_eq!(sight(f32::NAN, 50.0, 100.0), Sight::Clear);
        assert_eq!(sight(0.0, 50.0, 100.0), Sight::Clear);
    }
}
