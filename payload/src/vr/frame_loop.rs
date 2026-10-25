//! The per-frame OpenXR loop: [`frame_begin`] (`wait_frame` + `begin_frame` + `locate_views`), the
//! [`FrameContext`] that holds the runtime lock for the frame, the per-eye views and swapchain images
//! it hands to the blit, and the frame submit. The session lifecycle around it lives in
//! [`crate::vr::state`].

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use anyhow::Context as _;
use openxr as xr;
use parking_lot::MutexGuard;

use crate::{
    config::Config,
    controllers::{
        body::rig::{
            live_observation::{self, LocatedTracking},
            origin_provenance::{self, OriginEvidence, OriginUnavailable, TrackingReceipt},
        },
        frame as controller_frame,
    },
    frame_timing::{self, Slot},
    vr::{
        Fov, FreezeMode, OffAxisProjection, VIEW_TYPE, VrConfig,
        input::flight_pose::{self, LocatedHead},
        pose_control,
        recenter::mid_pose,
        replay, stage,
        state::{VrState, lock_state},
        swapchain::Swapchain,
    },
};

/// Begin an OpenXR frame: `wait_frame` + `begin_frame` + `locate_views`, returning a [`FrameContext`]
/// that holds the runtime lock for the duration of the frame. Returns `None` when no session is
/// running or the frame could not begin (the caller then renders flatscreen). The returned context
/// carries the per-eye poses (relative to the recenter baseline), FOVs, off-axis projections, and the
/// predicted display time; call [`FrameContext::should_render`] to decide whether to render or submit
/// an empty frame. Called from `hooks::game::game_update_render`.
pub fn frame_begin() -> Option<FrameContext> {
    live_observation::tracking_unavailable();
    // Acquiring the runtime lock blocks on the deferred frame tail (which holds it until it
    // submits the previous frame), so this scope measures how much of the previous frame's tail /
    // GPU work the main thread's prologue failed to overlap -- the pipelining slack still on the
    // table. (`vr::update` earlier takes the same lock, so the block usually lands there first; it
    // is scoped too.)
    #[cfg(feature = "profiler")]
    let lock_scope = puffin::profile_scope_custom!("VR runtime lock (tail block)");
    let mut guard = lock_state();
    #[cfg(feature = "profiler")]
    drop(lock_scope);

    // No controller frame is sampled on either early return, so the last one published must not
    // keep standing for the controllers' state.
    if !guard.is_running() {
        flight_pose::unavailable();
        controller_frame::clear();
        return None;
    }

    let cfg = Config::lock_query(|c| c.vr.clone());
    match guard.begin_frame(&cfg) {
        Ok(frame) => {
            // Recovered from a failure streak (if any): report how long it lasted, mirroring the
            // transition log below so the two bracket the outage in the log.
            if FRAME_BEGIN_FAILING.swap(false, Ordering::Relaxed) {
                let frames = FRAME_BEGIN_FAIL_COUNT.swap(0, Ordering::Relaxed);
                tracing::info!(target: "vr", frames, "frame begin recovered");
            }
            Some(FrameContext {
                guard,
                frame,
                image_acquired: false,
            })
        }
        Err(e) => {
            flight_pose::unavailable();
            controller_frame::clear();
            let count = FRAME_BEGIN_FAIL_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
            // This runs once a frame, so a session state that fails `wait_frame`/`begin_frame`
            // persistently (rather than a one-off) would otherwise flood the log at the frame rate and
            // scroll the actual diagnosis out of view. Log in full at the transition into the failure
            // streak, then only a periodic reminder (roughly once a second at a 90 Hz frame rate) while
            // it continues; [`FRAME_BEGIN_FAILING`] resets on recovery above.
            if !FRAME_BEGIN_FAILING.swap(true, Ordering::Relaxed) {
                tracing::warn!(target: "vr", "frame begin failed: {e:#}");
            } else if count.is_multiple_of(90) {
                tracing::warn!(target: "vr", count, "frame begin still failing: {e:#}");
            }
            None
        }
    }
}

/// A per-eye view for the frame in flight: pose relative to the recenter baseline, the raw HMD FOV,
/// and the off-axis projection built from it (both depth conventions, see [`crate::vr::projection`]).
#[derive(Copy, Clone)]
pub struct EyeView {
    /// The eye pose (position + orientation) relative to the recenter baseline, in the cockpit
    /// frame. When no baseline is set this is the raw LOCAL-space pose. This drives the *game camera*
    /// (so recentering re-orients the game world); it is NOT the compositor submission pose.
    pub pose: xr::Posef,
    /// The raw located eye pose in LOCAL space (before rebasing), i.e. where the eye actually is. This
    /// is the pose submitted to the compositor's projection layer: the layer is composited in LOCAL
    /// space, so its pose must describe the eye's true position, or the compositor reprojects the image
    /// to a plane offset by the recenter baseline. Equal to [`pose`](Self::pose) until the first
    /// recenter.
    pub raw_pose: xr::Posef,
    /// The eye's field of view, as reported by `locate_views`.
    pub fov: xr::Fovf,
    /// The off-axis projection for [`fov`](Self::fov). Write [`standard_depth`]
    /// (`OffAxisProjection::standard_depth`) into `m_Projection` before `SetupRenderCamera`
    /// (`docs/engine/rendering/rendering.md` §2.7 / blocker 1).
    pub projection: OffAxisProjection,
}

/// A swapchain image reference for one eye, handed to the per-eye blit. The swapchain is a single
/// 2-slice texture array; both eyes share the same acquired texture and are distinguished by
/// [`array_index`](Self::array_index). The texture is runtime-owned -- wrap it borrowed (no `AddRef`)
/// and do not release it.
#[derive(Copy, Clone)]
pub struct EyeImage {
    /// The acquired swapchain texture (`ID3D11Texture2D`), as a raw COM pointer. Wrap with
    /// `ID3D11Texture2D::from_raw` borrowed for the blit; the runtime owns it.
    pub texture: *mut std::ffi::c_void,
    /// The array slice for this eye (`0` = left, `1` = right).
    pub array_index: u32,
    /// The swapchain's DXGI format, so the per-eye blit can build a matching view / conversion.
    pub format: u32,
}

/// The frame in flight. Holds the runtime lock, so it must be dropped (or consumed via [`frame_end`])
/// before [`crate::vr::update`] or another [`frame_begin`] is called on the same thread. Carries the
/// per-eye views and the predicted display time; exposes the swapchain acquire/release the per-eye
/// blit needs.
///
/// [`frame_end`]: Self::frame_end
pub struct FrameContext {
    guard: MutexGuard<'static, VrState>,
    frame: FrameData,
    image_acquired: bool,
}

impl FrameContext {
    /// Whether the runtime wants the scene rendered this frame. When `false` the caller should skip
    /// rendering and call [`frame_end`](Self::frame_end) to submit an empty frame (the runtime is
    /// idle/occluded).
    pub fn should_render(&self) -> bool {
        self.frame.should_render
    }

    /// The predicted display time for this frame, for pose-dependent work and the frame submit.
    pub fn predicted_display_time(&self) -> xr::Time {
        self.frame.predicted_display_time
    }

    /// The per-eye view (pose relative to the recenter baseline, FOV, off-axis projection). `eye` is
    /// `0` (left) or `1` (right).
    pub fn eye_view(&self, eye: usize) -> EyeView {
        self.frame.eyes[eye]
    }

    pub(crate) fn tracking_origin(&self) -> (OriginEvidence, Option<TrackingReceipt>) {
        (self.frame.tracking_origin, self.frame.tracking_receipt)
    }

    /// Acquire and wait on the stereo swapchain image (created lazily on first use). Call once per
    /// frame before rendering; the two eyes are array slices of the returned image
    /// ([`eye_image`](Self::eye_image)). No-op if already acquired this frame.
    pub fn acquire(&mut self) -> anyhow::Result<()> {
        if self.image_acquired {
            return Ok(());
        }
        let cfg = Config::lock_query(|c| c.vr.clone());
        let _timing = frame_timing::scope(Slot::VrSwapchainWait);
        self.guard.acquire_swapchain_image(&cfg)?;
        self.image_acquired = true;
        self.frame.image_ever_acquired = true;
        Ok(())
    }

    /// The swapchain image for `eye` (`0` = left, `1` = right), valid only between [`acquire`] and
    /// [`release`]. `None` until [`acquire`] has run. The per-eye blit copies the game's captured eye
    /// texture into this image's `array_index` slice.
    ///
    /// [`acquire`]: Self::acquire
    /// [`release`]: Self::release
    pub fn eye_image(&self, eye: usize) -> Option<EyeImage> {
        if !self.image_acquired {
            return None;
        }
        let sc = self.guard.session.as_ref()?.swapchain.as_ref()?;
        Some(EyeImage {
            texture: sc.acquired_texture()?,
            array_index: eye as u32,
            format: sc.format,
        })
    }

    /// Release the swapchain image after the blit. No-op if not acquired.
    pub fn release(&mut self) -> anyhow::Result<()> {
        if !self.image_acquired {
            return Ok(());
        }
        self.guard.release_swapchain_image()?;
        self.image_acquired = false;
        Ok(())
    }

    /// End the frame: submit the world projection layer (or an empty frame when
    /// [`should_render`](Self::should_render) is false or the swapchain was never acquired) and
    /// consume the context, releasing the runtime lock. HUD quad layers become additional layers here
    /// in a later wave (`docs/mod/hud.md`); the surface takes only the world layer today.
    pub fn frame_end(mut self) -> anyhow::Result<()> {
        // Release any still-held image before submitting, so a caller that forgot to release does
        // not deadlock the swapchain.
        if self.image_acquired {
            self.release()?;
        }
        let submit_world = self.frame.should_render && self.frame.image_ever_acquired;
        let _timing = frame_timing::scope(Slot::VrEndFrame);
        self.guard.end_frame(&self.frame, submit_world)
    }
}

/// Whether the last located views had a valid pose, so a tracking loss and its recovery log once
/// each.
static HEAD_TRACKED: AtomicBool = AtomicBool::new(true);

/// Whether [`frame_begin`] is currently in a `wait_frame`/`begin_frame` failure streak, so repeated
/// failures log a transition and a recovery instead of one line per frame. See the rate-limiting
/// comment in [`frame_begin`].
static FRAME_BEGIN_FAILING: AtomicBool = AtomicBool::new(false);
/// Frames lost to the current (or, once recovery is logged, the just-ended) frame-begin failure
/// streak. Paired with [`FRAME_BEGIN_FAILING`].
static FRAME_BEGIN_FAIL_COUNT: AtomicU32 = AtomicU32::new(0);

/// The per-frame data captured at [`frame_begin`], carried by the [`FrameContext`].
struct FrameData {
    predicted_display_time: xr::Time,
    should_render: bool,
    eyes: [EyeView; 2],
    tracking_origin: OriginEvidence,
    tracking_receipt: Option<TrackingReceipt>,
    /// Whether the swapchain image was acquired at some point this frame (so `frame_end` knows
    /// whether a world layer can be submitted).
    image_ever_acquired: bool,
}

impl VrState {
    /// Begin a frame and locate the per-eye views, re-based into the cockpit frame. Updates the
    /// latest head pose (for [`crate::vr::recenter`]) from the mid-eye pose.
    fn begin_frame(&mut self, cfg: &VrConfig) -> anyhow::Result<FrameData> {
        flight_pose::unavailable();
        let _timing = frame_timing::scope(Slot::VrBeginFrame);
        let session = self
            .session
            .as_mut()
            .context("vr: no session for frame begin")?;

        // The real compositor pacing wait, isolated from the lock block above and the pose location
        // below: at a frame rate under the compositor's, this should return near-instantly (we are
        // the slow one), so a large value here is genuine reclaimable slack, not our own cost.
        let frame_state = {
            #[cfg(feature = "profiler")]
            puffin::profile_scope!("xrWaitFrame");
            let _timing = frame_timing::scope(Slot::VrWaitFrame);
            session.frame_wait.wait().context("vr: wait_frame failed")?
        };
        session
            .frame_stream
            .begin()
            .context("vr: begin_frame failed")?;

        // The engine's live active-camera planes are the single source of truth for near/far, so the
        // eyes render against the same frustum the engine reconstructs and culls with. Fall back to the
        // configured planes until the first camera update publishes the live values.
        let (near_clip, far_clip) =
            crate::hooks::camera::main_camera_planes_or((cfg.near_clip, cfg.far_clip));

        let mut eyes = [EyeView {
            pose: xr::Posef::IDENTITY,
            raw_pose: xr::Posef::IDENTITY,
            fov: xr::Fovf {
                angle_left: 0.0,
                angle_right: 0.0,
                angle_up: 0.0,
                angle_down: 0.0,
            },
            projection: OffAxisProjection::new(
                Fov {
                    left: 0.0,
                    right: 0.0,
                    up: 0.0,
                    down: 0.0,
                },
                near_clip,
                far_clip,
            ),
        }; 2];

        let mut head_pose = None;
        let mut head_tracked = false;
        let mut located = false;
        let mut flight_head = None;
        let mut body_head = None;
        let origin_readiness = self
            .origin_reference_space
            .prepare(frame_state.predicted_display_time.as_nanos());
        let mut tracking_origin = OriginEvidence::Unavailable(OriginUnavailable::SourceAbsent);
        let mut tracking_receipt = None;
        if frame_state.should_render {
            tracking_origin = origin_provenance::begin(origin_readiness);
            let (view_flags, views) = session
                .handle
                .locate_views(
                    VIEW_TYPE,
                    frame_state.predicted_display_time,
                    &session.local,
                )
                .context("vr: locate_views failed")?;
            // Without a valid position the views' positions are undefined, so the head pose stays
            // unpublished: the recenter keeps the last tracked pose, and the STAGE head height keeps
            // the last tracked height, rather than either taking whatever the runtime returns while
            // tracking is lost. Both follow the head again on the first tracked frame.
            let tracked = view_flags.contains(
                xr::ViewStateFlags::POSITION_VALID | xr::ViewStateFlags::ORIENTATION_VALID,
            );
            head_tracked = tracked;
            if HEAD_TRACKED.swap(tracked, Ordering::Relaxed) != tracked {
                if tracked {
                    tracing::info!(target: "vr", "head tracking: the view pose is valid again");
                } else {
                    tracing::warn!(
                        target: "vr",
                        flags = ?view_flags,
                        "head tracking: the view pose is not valid; holding the last head height",
                    );
                }
            }
            if views.len() >= 2 {
                head_pose = tracked.then(|| mid_pose(views[0].pose, views[1].pose));
                // The compositor submission poses. While a freeze diagnostic holds the render still,
                // the image is no longer rendered from where the head is, so the pair latched at
                // freeze time is submitted instead of the live one -- otherwise the runtime keeps
                // reprojecting a static image toward the moving head and the headset view warps even
                // though nothing in the render moved (see `pose_control::submission_poses`).
                let raw_poses =
                    pose_control::submission_poses(cfg.freeze_mode != FreezeMode::Off, || {
                        [views[0].pose, views[1].pose]
                    });
                for ((eye, view), raw_pose) in eyes.iter_mut().zip(views.iter()).zip(raw_poses) {
                    let pose = match self.baseline {
                        Some(b) => b.rebase(view.pose),
                        None => view.pose,
                    };
                    *eye = EyeView {
                        pose,
                        raw_pose,
                        fov: view.fov,
                        projection: OffAxisProjection::new(
                            fov_from_xr(view.fov),
                            near_clip,
                            far_clip,
                        ),
                    };
                }
                flight_head = Some(LocatedHead {
                    eyes: eyes.map(|eye| eye.pose),
                    flags: view_flags,
                    display_time_ns: frame_state.predicted_display_time.as_nanos(),
                });
                body_head = Some(LocatedTracking {
                    head: mid_pose(eyes[0].pose, eyes[1].pose),
                    flags: view_flags,
                    display_time_ns: frame_state.predicted_display_time.as_nanos(),
                    origin: tracking_origin,
                    cockpit_from_local: self.baseline.map_or(xr::Posef::IDENTITY, |baseline| {
                        baseline.rebase(xr::Posef::IDENTITY)
                    }),
                });
                located = true;
            }
        }
        let flight_head = flight_pose::begin_located(flight_head);
        if located {
            replay::replace_located_head(&mut eyes);
        }

        if let Some(p) = head_pose {
            self.latest_head_pose = Some(p);
        }
        stage::locate_head(
            &session.handle,
            &session.local,
            session.stage.as_ref(),
            frame_state.predicted_display_time,
            head_pose,
        );
        if let Some(info) = crate::testing::replay::stage() {
            stage::publish_replayed(info);
        }

        // The controllers are sampled at the same predicted display time and re-based against the
        // same baseline as the eye views, so a hand and the head it is drawn beside agree.
        if let Some(input) = self.input.as_mut() {
            let controllers = Config::lock_query(|c| c.controllers);
            let _timing = frame_timing::scope(Slot::VrInput);
            let sampled = input.update(
                &session.handle,
                &session.local,
                frame_state.predicted_display_time,
                self.baseline,
                &controllers,
                flight_head,
            );
            tracking_receipt = sampled.as_ref().map(TrackingReceipt::from_frame);
            tracking_origin = if sampled.is_some() {
                tracking_origin.finish()
            } else {
                OriginEvidence::Unavailable(OriginUnavailable::SourceAbsent)
            };
            if let Some(head) = body_head.as_mut() {
                head.origin = tracking_origin;
            }
            live_observation::publish_tracking(body_head, sampled);
        } else {
            flight_pose::unavailable();
            tracking_origin = OriginEvidence::Unavailable(OriginUnavailable::SourceAbsent);
        }
        if !located {
            tracking_origin = OriginEvidence::Unavailable(OriginUnavailable::SourceAbsent);
            tracking_receipt = None;
        }

        // After the controllers, so the frame records what the input update published.
        crate::recorder::tracking::capture(replay::capture_sample(
            frame_state.predicted_display_time,
            frame_state.should_render,
            head_tracked,
            located.then_some(&eyes),
        ));

        Ok(FrameData {
            predicted_display_time: frame_state.predicted_display_time,
            should_render: frame_state.should_render,
            eyes,
            tracking_origin,
            tracking_receipt,
            image_ever_acquired: false,
        })
    }

    /// Acquire and wait on the swapchain image, creating the swapchain lazily on first use.
    fn acquire_swapchain_image(&mut self, cfg: &VrConfig) -> anyhow::Result<()> {
        let instance = self
            .instance
            .as_ref()
            .context("vr: no instance for swapchain acquire")?
            .clone();
        let system = self.system.context("vr: no system for swapchain acquire")?;
        let session = self
            .session
            .as_mut()
            .context("vr: no session for swapchain acquire")?;
        if session.swapchain.is_none() {
            session.swapchain = Some(Swapchain::create(&instance, system, &session.handle, cfg)?);
        }
        let sc = session
            .swapchain
            .as_mut()
            .expect("swapchain was just ensured");
        sc.acquire()
    }

    /// Release the swapchain image.
    fn release_swapchain_image(&mut self) -> anyhow::Result<()> {
        let session = self
            .session
            .as_mut()
            .context("vr: no session for swapchain release")?;
        let sc = session
            .swapchain
            .as_mut()
            .context("vr: no swapchain to release")?;
        sc.release()
    }

    /// End the frame, submitting the world projection layer when `submit_world`, else an empty
    /// frame. Borrows the session's fields disjointly so the layer can reference the swapchain and
    /// local space while `frame_stream` is borrowed mutably.
    fn end_frame(&mut self, frame: &FrameData, submit_world: bool) -> anyhow::Result<()> {
        let blend_mode = self.blend_mode;
        let session = self
            .session
            .as_mut()
            .context("vr: no session for frame end")?;

        if !submit_world {
            return session
                .frame_stream
                .end(frame.predicted_display_time, blend_mode, &[])
                .context("vr: end_frame (empty) failed");
        }

        let sc = session
            .swapchain
            .as_ref()
            .context("vr: no swapchain for world layer")?;
        let extent = xr::Extent2Di {
            width: sc.width as i32,
            height: sc.height as i32,
        };
        let rect = xr::Rect2Di {
            offset: xr::Offset2Di { x: 0, y: 0 },
            extent,
        };
        // Submit the RAW located eye poses (where the eyes actually are in LOCAL space), not the
        // rebased poses that drive the game camera. The projection layer is composited in LOCAL space,
        // so the compositor treats the submitted pose as the image's viewpoint and reprojects to the
        // real eye; feeding it the rebased pose displaces the image by the recenter baseline (the
        // floating, angled plane after F7). The recenter is already baked into the rendered content
        // via the game camera, so the compositor must see the true eye pose here.
        let views = [
            xr::CompositionLayerProjectionView::new()
                .pose(frame.eyes[0].raw_pose)
                .fov(frame.eyes[0].fov)
                .sub_image(
                    xr::SwapchainSubImage::new()
                        .swapchain(&sc.handle)
                        .image_array_index(0)
                        .image_rect(rect),
                ),
            xr::CompositionLayerProjectionView::new()
                .pose(frame.eyes[1].raw_pose)
                .fov(frame.eyes[1].fov)
                .sub_image(
                    xr::SwapchainSubImage::new()
                        .swapchain(&sc.handle)
                        .image_array_index(1)
                        .image_rect(rect),
                ),
        ];
        let layer = xr::CompositionLayerProjection::new()
            .space(&session.local)
            .views(&views);
        session
            .frame_stream
            .end(frame.predicted_display_time, blend_mode, &[&layer])
            .context("vr: end_frame failed")
    }
}

/// Convert an `xr::Fovf` (radian half-angles) into a [`Fov`] for the projection builder.
fn fov_from_xr(fov: xr::Fovf) -> Fov {
    Fov {
        left: fov.angle_left,
        right: fov.angle_right,
        up: fov.angle_up,
        down: fov.angle_down,
    }
}
