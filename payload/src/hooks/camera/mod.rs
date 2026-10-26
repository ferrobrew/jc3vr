use std::{
    ffi::c_void,
    sync::atomic::{AtomicU32, Ordering},
};

use detours_macro::detour;
use jc3gi::{
    camera::{
        camera::Camera,
        camera_context::{CameraContext, CameraControlContext},
        camera_manager::CameraManager,
        game_camera_manager::GameCameraManager,
    },
    character::character::{Character, SafeBoneIndex},
    graphics_engine::graphics_engine::GraphicsEngine,
    hash::hashlittle,
    types::math::Matrix4,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    debug::trace::{TraceEvent, TraceState},
    stereo::STEREO_STATE,
};

pub(crate) mod authored;
pub(crate) mod config;
pub mod height;
pub(crate) use config::CameraConfig;

use crate::{
    fsr::apply_jitter_to_projection,
    headpose::{
        HeadPose, Source, anchor, anchor_prev, eye_arm, is_active, neck_delta, query, query_prev,
        source, xr::follow_turn,
    },
    hooks::in_gameplay,
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&CAMERA_UPDATE_RENDER_BINDER)
        .with_static_binder(&CAMERA_TREE_UPDATE_RENDER_CONTEXTS_BINDER)
        .with_static_binder(&SETUP_RENDER_CAMERA_BINDER)
        .with_static_binder(&GAME_CAMERA_MANAGER_GET_CAMERA_MATRIX_BINDER)
}

/// The last `dtf` the active camera's `UpdateRender` received, for the debug UI: the engine's
/// sub-frame interpolation fraction (see `docs/issues/20-animation-judder.md`). If it sits at 0.0
/// or 1.0 every frame, the engine's T0 → T1 lerp is inert and nothing smooths the sim-tick
/// cadence.
pub fn last_dtf() -> f32 {
    f32::from_bits(LAST_DTF.load(Ordering::Relaxed))
}

/// The scene render camera is the engine-owned copy (`GraphicsEngine::m_RenderCamera`), rebuilt
/// each Draw by `SetupRenderCamera` (reverse-Z + jitter, then `m_ViewProjection`/`m_ViewProjectionF`
/// from `m_View`). For the stereo double-Draw we offset that copy's `m_View` laterally per eye,
/// *before* the rebuild, so the two dispatches diverge. See `docs/engine/rendering/rendering.md` section 2.
#[detour(address = jc3gi::camera::camera::Camera::SetupRenderCamera_ADDRESS)]
fn setup_render_camera(camera: *mut Camera, jitter: bool) -> *mut c_void {
    let is_render_camera = unsafe {
        GraphicsEngine::get().is_some_and(|ge| std::ptr::eq(&raw const ge.m_RenderCamera, camera))
    };
    if is_render_camera {
        TraceState::record_eye(TraceEvent::SetupRenderCamera);
        let mut stereo = STEREO_STATE.lock();
        // Clear the shadow-anchor delta; the parallax block below sets it when disparity is on, so a
        // stale value can't leak into the sun-shadow cascade correction when disparity is off.
        stereo.shadow_anchor_delta = glam::Vec3::ZERO;
        // The frame's first dispatch opens a new real frame: last frame's view-projection
        // snapshots become "previous". Keyed on the dispatch ordinal, not the eye index — a share
        // frame's far dispatch and eye 0's near dispatch both carry eye index 0, and rotating
        // twice would collapse the FSR reprojection history to intra-frame deltas.
        if stereo.dispatch_ordinal == 0 {
            stereo.vp_history.rotate();
        }
    }

    // Snapshot the stereo + FSR config once; drop the lock before the engine call below.
    let (stereo_active, force_smaa_1x, stereo_cameras, ipd, fsr_enabled) = {
        let active = crate::stereo::active();
        Config::lock_query(|c| {
            (
                active,
                c.stereo.force_smaa_1x,
                c.stereo.cameras,
                c.stereo.ipd,
                c.fsr.enabled,
            )
        })
    };

    // The engine's TAA jitter only feeds its own SMAA T2X. Drop it when that resolve is gone --
    // either because we force SMAA 1x in stereo, or because FSR has replaced the engine AA. FSR still
    // needs jitter, but its own Halton sequence, applied below.
    let jitter = jitter && !fsr_enabled && !(stereo_active && force_smaa_1x);

    // The VR per-eye off-axis projection and world offset (docs/mod/vr-runtime.md blockers 1 & 3).
    // Fetch this eye's parameters once; `None` on flatscreen frames. Under the preferred convention,
    // write the standard-depth off-axis projection into `m_Projection` *before* SetupRenderCamera so
    // the engine applies its reverse-Z remap and TAA jitter to it exactly once (§2.7).
    let vr_eye = is_render_camera
        .then(|| crate::vr::render_params(crate::stereo::draw_index()))
        .flatten();
    if let Some(vr) = vr_eye
        && vr.convention == crate::vr::ProjectionConvention::EnginePreReverseZ
        && let Some(camera) = unsafe { camera.as_mut() }
    {
        camera.m_Projection = vr.projection_standard;
    }

    // Outside gameplay (loading screens, fast-travel teleports) the engine parks its own camera at the
    // teleport destination and drives world streaming from it, so the mod must leave that sim camera
    // untouched or the load never completes (issue #27). Head-track the *render* copy only -- which the
    // streaming/LOD system does not read -- around the frozen upright loading base, so the player can
    // look around during the load. Written before the engine call so it builds the view-projections
    // from the head-tracked view; the per-eye offset below then applies on top.
    let pointer_cockpit = crate::headpose::xr::cockpit_pose();
    let pointer_scale = Config::lock_query(|c| c.vr.world_scale);
    if is_render_camera
        && vr_loading_view_active()
        && let Some(camera) = unsafe { camera.as_mut() }
    {
        head_track_render_camera(camera, pointer_cockpit, pointer_scale);
    }

    // The full-camera freeze diagnostic. This is the last write to the scene camera before the engine
    // consumes it, so every contribution above -- the headpose the sim camera was placed from, the
    // body frame, the animated head-bone anchor, the loading-screen head-track, and the game's own
    // camera when it owns the view -- is already folded into `m_TransformF` and is captured wholesale
    // rather than partially. Applied *before* `SetupRenderCamera` so the engine rebuilds the
    // view-projections (and fits the sun-shadow cascade) from the frozen centre, and the per-eye
    // offset below still runs on top of it; the per-eye parameters are frozen to match in
    // `crate::vr::begin_render_frame`. See `crate::vr::FreezeMode`.
    if is_render_camera && let Some(camera) = unsafe { camera.as_mut() } {
        freeze_render_camera(camera);
    }

    let result = SETUP_RENDER_CAMERA.get().unwrap().call(camera, jitter);

    // Snapshot the center view-projection before the FSR-jitter and per-eye blocks below patch it.
    // This is the value the engine's own sim-side previous-VP snapshot holds (un-offset, unjittered
    // -- the engine jitter is disabled above whenever we patch), which the velocity pass reprojects
    // with; the FSR motion-vector correction needs it as its "what the engine encoded" matrix.
    if is_render_camera && let Some(camera) = unsafe { camera.as_ref() } {
        // Coordinate-frame verification (docs/mod/vr-runtime.md "Blocker 3"): log the pristine center
        // camera's basis + travel direction before the per-eye offset below mutates m_TransformF.
        crate::debug::coord_frame::log_render_camera_frame(camera);

        STEREO_STATE.lock().vp_history.cur_center =
            Some(glam::Mat4::from(camera.m_ViewProjectionF));

        // Snapshot the un-offset world transform for the HUD panel pose (see `StereoState::center_transform`).
        STEREO_STATE.lock().center_transform = Some(camera.m_TransformF);
        crate::hud::pointer::publish_camera(
            glam::Mat4::from(camera.m_TransformF),
            pointer_cockpit,
            pointer_scale,
        );
    }

    // FSR is a temporal reconstructor: it needs the camera jittered by its sequence, with the same
    // offset fed to the dispatch. Apply it to the render camera's projections after SetupRenderCamera
    // has built them (reverse-Z done), then rebuild the view-projections from the jittered proj.
    if is_render_camera && fsr_enabled {
        unsafe {
            if let Some(camera) = camera.as_mut()
                && let Some(ge) = GraphicsEngine::get()
                && let Some(mc) = ge.m_MainColorBuffer.as_ref()
            {
                let (w, h) = (u32::from(mc.m_Width), u32::from(mc.m_Height));
                apply_jitter_to_projection(&mut camera.m_Projection, w, h);
                apply_jitter_to_projection(&mut camera.m_ProjectionF, w, h);
                // Publish the UV-space shift this jitter applies to every projected position, for
                // the motion-vector jitter cancellation (the velocity pass measures curUV under the
                // jittered projection, so every vector carries this shift as a constant offset).
                let jitter_uv = crate::fsr::current_camera_jitter_ndc(w, h)
                    .map_or((0.0, 0.0), |(x, y)| (0.5 * x, -0.5 * y));
                STEREO_STATE.lock().vp_history.cur_jitter_uv = jitter_uv;
                camera.m_ViewProjection = camera.m_View * camera.m_Projection;
                camera.m_ViewProjectionF = camera.m_View * camera.m_ProjectionF;
            }
        }
    }

    // Per-eye parallax: shift the camera world position (`m_TransformF` translation, the
    // CameraPosition the camera-relative scene render subtracts) per eye. In VR the offset is the
    // TRUE per-eye delta from `locate_views` (a full 3D vector); on flatscreen stereo it is the
    // synthetic +/- half-IPD along the camera right axis. Either way, re-derive m_View from the moved
    // m_TransformF and rebuild the view-projections, so the offset reaches the full-m_ViewProjection
    // shaders (transparents/sky/water), not just the camera-relative opaque path.
    if is_render_camera && stereo_active {
        if crate::stereo::single_pass::collapse_active() {
            // Collapsed single-pass keeps the render camera centered: both eyes are produced from the
            // per-eye `cb13` in the patched vertex shaders, so applying a per-eye world offset here
            // would double the parallax. Zero the shadow-anchor delta to match the centered camera
            // (the sun-cascade correction expects whatever offset this path would otherwise add).
            STEREO_STATE.lock().shadow_anchor_delta = glam::Vec3::ZERO;
        } else if let Some(vr) = vr_eye {
            unsafe {
                if let Some(camera) = camera.as_mut() {
                    // Fallback convention: write the already-reverse-Z'd off-axis projection now
                    // (after SetupRenderCamera, so the engine does not re-reverse it); the VP rebuild
                    // below picks it up. TAA jitter is not applied on this path.
                    if vr.convention == crate::vr::ProjectionConvention::ManualReverseZ {
                        camera.m_Projection = vr.projection_reverse_z;
                        camera.m_ProjectionF = vr.projection_reverse_z;
                    }
                    let delta = vr.world_offset;
                    let mut transform = glam::Mat4::from(camera.m_TransformF);
                    transform.w_axis += delta.extend(0.0);
                    // Rotate the camera basis to this eye's orientation (display canting) about the
                    // now-offset eye position. m_TransformF is a column-vector world transform (its
                    // columns are the basis vectors), so a head-local rotation composes on the right
                    // and leaves the translation column -- the eye position just written -- intact.
                    // Identity for parallel-panel HMDs; corrects the Valve Index's ~5°/eye cant,
                    // without which the two eyes are rotationally mismatched and will not fuse.
                    let transform = transform * glam::Mat4::from_quat(vr.orientation_delta);
                    camera.m_TransformF = Matrix4::from(transform);
                    // Re-derive m_View = inverse(m_TransformF); the engine's data reads straight into
                    // glam's column-major matrix (see the Matrix4 glam bridge), so the inverse is
                    // written straight back.
                    let view = transform.inverse();
                    camera.m_View = Matrix4::from(view);
                    camera.m_ViewProjection = camera.m_View * camera.m_Projection;
                    camera.m_ViewProjectionF = camera.m_View * camera.m_ProjectionF;
                    // Publish this eye's world offset for the sun-shadow cascade correction (see
                    // SetGlobalShaderConstants hook / stereo::StereoState::shadow_anchor_delta).
                    STEREO_STATE.lock().shadow_anchor_delta = delta;
                }
            }
        } else if stereo_cameras {
            unsafe {
                if let Some(camera) = camera.as_mut() {
                    let eye1 = crate::stereo::draw_index() == 1;
                    let half_ipd = ipd * 0.5;
                    // Eye 0 is the LEFT eye (shift -right), eye 1 the RIGHT (shift +right), so view 0
                    // == left (OpenXR convention). The reverse assignment fuses the debug pair
                    // cross-eyed when the "parallel" toggle is off.
                    let offset = if eye1 { half_ipd } else { -half_ipd };
                    let mut transform = glam::Mat4::from(camera.m_TransformF);
                    let delta = offset * transform.x_axis.truncate();
                    transform.w_axis += delta.extend(0.0);
                    camera.m_TransformF = Matrix4::from(transform);
                    // Publish this eye's world offset for the sun-shadow cascade correction (see
                    // SetGlobalShaderConstants hook / stereo::StereoState::shadow_anchor_delta).
                    STEREO_STATE.lock().shadow_anchor_delta = delta;

                    // m_View == inverse(m_TransformF), so the +offset*right shift of the camera world
                    // position is a -offset shift of the view translation-X (data[12]).
                    // SetupRenderCamera has already applied reverse-Z + jitter to the projections, so
                    // rebuild m_ViewProjection / m_ViewProjectionF from them. `Matrix4`'s `*` is the
                    // engine's row-major `Multiply4x4` convention, so the operand order is the same
                    // one the engine uses.
                    camera.m_View.data[12] -= offset;
                    camera.m_ViewProjection = camera.m_View * camera.m_Projection;
                    camera.m_ViewProjectionF = camera.m_View * camera.m_ProjectionF;
                }
            }
        }
    }

    // Snapshot the final view-projection this dispatch rasterizes with (jitter and eye offset
    // applied). The FSR motion-vector correction inverts it to reconstruct each pixel's position and
    // re-anchor the velocity reprojection at this eye's own previous pose.
    if is_render_camera && let Some(camera) = unsafe { camera.as_ref() } {
        let mut stereo = STEREO_STATE.lock();
        let index = stereo.draw_index;
        stereo.vp_history.cur_eye[index] = Some(glam::Mat4::from(camera.m_ViewProjectionF));
        stereo.vp_history.cur_eye_view[index] = Some(glam::Mat4::from(camera.m_View));
        stereo.vp_history.cur_eye_projection_f[index] =
            Some(glam::Mat4::from(camera.m_ProjectionF));
    }

    result
}

/// Pin the scene render camera's world transform (and the view derived from it) to the pose the
/// [`FreezeMode::FullCamera`](crate::vr::FreezeMode::FullCamera) diagnostic holds, capturing the
/// engine's current one the first frame the mode is on. Inert in every other mode: it reads the
/// config and returns, leaving the camera exactly as the engine built it.
///
/// The held pose is hand-editable in world space from the Camera tab, so the view can be stepped by
/// an exact translation or rotation and returned to -- which is what turns "this content slides as
/// the camera moves" into a measurement. `m_View` is re-derived here rather than captured, so an edit
/// reaches both matrices consistently.
fn freeze_render_camera(camera: &mut Camera) {
    if Config::lock_query(|c| c.vr.freeze_mode) != crate::vr::FreezeMode::FullCamera {
        return;
    }
    let world =
        crate::vr::pose_control::full_camera_transform(|| glam::Mat4::from(camera.m_TransformF));
    camera.m_TransformF = Matrix4::from(world);
    camera.m_View = Matrix4::from(world.inverse());
}

/// The camera pipeline within a frame (see `docs/engine/rendering/rendering.md` §2.2): `CameraTree::
/// UpdateRenderContexts` fills the control contexts, `PushRenderContext` copies the next render
/// context's transform into the active camera's `m_TransformT0` and `m_TransformT1` (running it
/// through a rotation jitter filter that snaps sub-epsilon deltas), and `Camera::UpdateRender`
/// lerps T0 → T1 by `dtf` into `m_TransformF` and derives `m_View = inverse(m_TransformF)`.
///
/// Both mod writes therefore happen *before* the original call: post-call writes land after the
/// Lerp and view derivation, do nothing for the current frame, and are clobbered by the next
/// frame's `PushRenderContext`. Writing pre-call also bypasses the jitter filter (which otherwise
/// quantizes small headpose rotations into visible steps), and giving T0 the previous-tick pose
/// and T1 the current one hands the engine's own dtf Lerp the pair it needs to smooth the
/// tick-rate headpose across rendered frames.
#[detour(address = jc3gi::camera::camera::Camera::UpdateRender_ADDRESS)]
fn camera_update_render(camera: *mut Camera, dt: f32, dtf: f32) {
    unsafe {
        if let Some(local_character) = Character::GetLocalPlayerCharacter().as_mut()
            && let Some(camera) = camera.as_mut()
            && let Some(cm) = CameraManager::get()
            && cm.m_ActiveCamera == camera
        {
            LAST_DTF.store(dtf.to_bits(), Ordering::Relaxed);

            // Publish the engine's own near/far for the active camera so the reconstruction override
            // can recognize the main-view depth passes by their planes. The engine sets a runtime far
            // that differs from the `Camera` constructor default, so keying that override on a
            // hardcoded plane silently misses every main pass (see `main_camera_planes`).
            *MAIN_CAMERA_PLANES.lock() = Some((camera.m_Near, camera.m_Far));

            let camera_settings = Config::lock_query(|c| c.camera);
            if !camera_settings.enabled {
                authored::reset(authored::Refusal::Disabled);
                *LAST_CAMERA_WORLD.lock() = None;
                CAMERA_UPDATE_RENDER.get().unwrap().call(camera, dt, dtf);
                return;
            }

            // Outside gameplay (loading screens, fast-travel teleports, the frontend) the engine resets
            // its own camera to the teleport destination and drives world streaming from it, so the mod
            // must leave this sim camera entirely alone: pinning it to Rico's head bone -- or to any
            // frozen pose -- feeds the streaming/LOD system the wrong position and the loading screen
            // never completes (issue #27). Stop absolute placement here and touch nothing. The player's
            // head-tracking for the *view* is applied to the render camera copy in `setup_render_camera`
            // (which the streaming system does not read); absolute placement resumes and the
            // auto-recenter re-bases onto Rico's updated head once gameplay returns.
            if !in_gameplay() {
                authored::reset(authored::Refusal::OutsideGameplay);
                *LAST_CAMERA_WORLD.lock() = None;
                CAMERA_UPDATE_RENDER.get().unwrap().call(camera, dt, dtf);
                return;
            }

            // The headpose path needs a valid anchor; until one exists (loading screens), fall
            // back to the translation-only bone-derived placement below.
            let headpose_active = is_active() && anchor().is_some();

            if !headpose_active {
                authored::reset(authored::Refusal::NoTrackedHead);
                *LAST_CAMERA_WORLD.lock() = None;
            }
            if headpose_active
                && let Some([previous, current]) = authored::selected(local_character)
            {
                camera.m_TransformT0 = Matrix4::from(previous);
                camera.m_TransformT1 = Matrix4::from(current);
                *LAST_CAMERA_WORLD.lock() = Some(camera.m_TransformT1);
                height::record(
                    glam::Mat4::from(local_character.m_WorldMatrixT1),
                    current.w_axis.truncate(),
                );
            } else if headpose_active {
                // Both position and orientation come from the tick-spaced pose pair, so the
                // engine's dtf Lerp smooths the whole camera placement — the bone reads
                // (`GetSafeBoneMatrix`) only carry the finalized sim-rate pose, and placing T0/T1
                // from them stepped the camera at the tick rate even though the mesh itself
                // interpolates via the skinning-palette pose pair.
                let cur = query();
                let prev = query_prev();
                let character_t1_matrix = glam::Mat4::from(local_character.m_WorldMatrixT1);
                let position = camera_position(
                    &cur,
                    character_t1_matrix,
                    follow_turn(true),
                    &camera_settings,
                );
                write_camera_transform(&mut camera.m_TransformT1, cur.orientation, position);
                height::record(character_t1_matrix, position);
                // Republish the transform for the sim-phase readers (see the GetCameraMatrix
                // hook below).
                *LAST_CAMERA_WORLD.lock() = Some(camera.m_TransformT1);

                if camera_settings.always_use_t1 {
                    camera.m_TransformT0 = camera.m_TransformT1;
                } else {
                    let character_t0_matrix = glam::Mat4::from(local_character.m_WorldMatrixT0);
                    write_camera_transform(
                        &mut camera.m_TransformT0,
                        prev.orientation,
                        camera_position(
                            &prev,
                            character_t0_matrix,
                            follow_turn(false),
                            &camera_settings,
                        ),
                    );
                }
            } else {
                let head_matrix = head_matrix(local_character);
                let (left_eye_matrix, right_eye_matrix) = eye_matrices(local_character);
                let character_t1_matrix = glam::Mat4::from(local_character.m_WorldMatrixT1);
                let head_position = calculate_head_position(
                    character_t1_matrix,
                    head_matrix,
                    left_eye_matrix,
                    right_eye_matrix,
                    camera_settings.use_eye_matrices,
                    &camera_settings,
                );
                camera.m_TransformT1.data[12] = head_position.x;
                camera.m_TransformT1.data[13] = head_position.y;
                camera.m_TransformT1.data[14] = head_position.z;

                if camera_settings.always_use_t1 {
                    camera.m_TransformT0 = camera.m_TransformT1;
                } else {
                    let character_t0_matrix = glam::Mat4::from(local_character.m_WorldMatrixT0);
                    let head_position_t0 = calculate_head_position(
                        character_t0_matrix,
                        head_matrix,
                        left_eye_matrix,
                        right_eye_matrix,
                        camera_settings.use_eye_matrices,
                        &camera_settings,
                    );
                    camera.m_TransformT0.data[12] = head_position_t0.x;
                    camera.m_TransformT0.data[13] = head_position_t0.y;
                    camera.m_TransformT0.data[14] = head_position_t0.z;
                }
            }

            if authored::take_normal_reset() {
                camera.m_TransformT0 = camera.m_TransformT1;
            }

            // Capture this gameplay frame's viewpoint as the upright loading base (issue #27): its
            // world position and yaw only, dropping pitch and roll so the base stays comfortable.
            // Refreshed every gameplay frame and read (frozen) while the game parks its own camera
            // outside gameplay, giving the loading-screen head-tracking a stable, well-formed base to
            // look around and the floating panel a fixed world anchor.
            let (_, gameplay_rotation, gameplay_position) =
                glam::Mat4::from(camera.m_TransformT1).to_scale_rotation_translation();
            *LOADING_BASE.lock() = Some((gameplay_position, yaw_only(gameplay_rotation)));
        }
    }

    CAMERA_UPDATE_RENDER.get().unwrap().call(camera, dt, dtf);
}

/// The camera position for a headpose: the eye point hung off the published head position through
/// the [`EyeRig`], plus the body-frame offset. The arm's base is the *measured* neck-to-eye-midpoint
/// arm from the animated eye bones with `use_eye_matrices` on, with `head_offset` a correction on top
/// of it; with it off, `head_offset` is the whole arm. Both the arm and the body-frame offset use the
/// character matrix matching the pose's side of the T0/T1 pair.
///
/// How the arm turns with the head depends on the source (see [`NeckModel`]): the sim pivots it
/// about the neck by the head orientation, while the VR source holds it at rest in the body frame
/// because the HMD's tracked translation already carries the player's own neck pivot.
///
/// `view_turn` is the turn from the character's body frame to the view frame on the pose's side of
/// the pair ([`crate::headpose::xr::follow_turn`]): the identity unless the body has turned under
/// the view.
fn camera_position(
    pose: &HeadPose,
    character_matrix: glam::Mat4,
    view_turn: glam::Quat,
    camera_settings: &config::CameraConfig,
) -> glam::Vec3 {
    let (_, character_rotation, _) = character_matrix.to_scale_rotation_translation();
    let measured_arm = if camera_settings.use_eye_matrices {
        eye_arm()
    } else {
        glam::Vec3::ZERO
    };
    let rig = EyeRig {
        neck_delta: neck_delta(),
        arm: measured_arm + camera_settings.head_offset,
        body_offset: camera_settings.body_offset,
        view_turn,
    };
    eye_position(
        pose,
        character_rotation,
        &rig,
        NeckModel::for_source(source()),
    )
}

/// The geometry the camera's eye point hangs from the published head position by.
#[derive(Copy, Clone, Debug)]
struct EyeRig {
    /// The world-space vector from the head anchor to the animated neck bone, the arm's pivot.
    neck_delta: glam::Vec3,
    /// The neck-to-eyes arm in the body frame.
    arm: glam::Vec3,
    /// A further offset in the body frame, applied after the arm.
    body_offset: glam::Vec3,
    /// The turn from the body frame to the view frame. The tracked neck model holds the arm at rest
    /// in the view frame: the HMD's translation already carries the player's eyes round with a
    /// turn of their torso, so the arm must not turn again with a body that turns under the view
    /// to follow it.
    view_turn: glam::Quat,
}

/// How the neck-to-eyes arm follows the head's orientation.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum NeckModel {
    /// The arm swings about the neck by the head orientation, so pitching the head carries the
    /// eyes forward over the chest. The sim source's published position is the head anchor alone
    /// and has no neck kinematics of its own, so this supplies them.
    Synthetic,
    /// The arm stays at its rest pose in the body frame whatever the head orientation. The VR
    /// source's published position already moves with the HMD's tracked translation, which
    /// includes the player's real neck pivot; swinging the arm as well would apply the pivot twice
    /// (looking down would drop the view into the chest).
    Tracked,
}

impl NeckModel {
    fn for_source(source: Source) -> Self {
        match source {
            Source::Sim => NeckModel::Synthetic,
            Source::Vr => NeckModel::Tracked,
        }
    }
}

/// The eye point for `pose`: the head position, plus the neck delta, plus the arm rotated per
/// `neck`, plus the body-frame offset. Under [`NeckModel::Tracked`] the head anchor to eye offset
/// is a pure function of the body frame, so the eye point moves exactly as the pose position does.
fn eye_position(
    pose: &HeadPose,
    character_rotation: glam::Quat,
    rig: &EyeRig,
    neck: NeckModel,
) -> glam::Vec3 {
    let arm_rotation = match neck {
        NeckModel::Synthetic => pose.orientation,
        NeckModel::Tracked => character_rotation * rig.view_turn,
    };
    pose.position + rig.neck_delta + arm_rotation * rig.arm + character_rotation * rig.body_offset
}

/// Write a full world transform (rotation + translation) into a camera transform slot.
fn write_camera_transform(target: &mut Matrix4, orientation: glam::Quat, position: glam::Vec3) {
    let world = glam::Mat4::from_rotation_translation(orientation, position);
    let m = world.to_cols_array();
    // Write rotation + translation (data[0..=14]); leave the projective row untouched.
    for (i, &v) in m.iter().enumerate().take(15) {
        target.data[i] = v;
    }
}

/// Whether the mod should present the VR loading view this frame: outside gameplay
/// (`GameState != E_GAME_RUN`), VR is the headpose source, and a gameplay frame has captured a
/// [`LOADING_BASE`] to present against. The single source of truth for the loading-presentation regime,
/// shared by the render-camera head-track ([`head_track_render_camera`], gated on this in
/// `setup_render_camera`) and the floating panel's world-lock in `crate::hud`, so the render camera and
/// the panel always agree on when it is active and hand off together.
pub fn vr_loading_view_active() -> bool {
    !in_gameplay() && source() == Source::Vr && LOADING_BASE.lock().is_some()
}

/// Head-track the *render* camera around the [`LOADING_BASE`] (the last gameplay viewpoint, upright)
/// while the game owns the sim camera outside gameplay (issue #27). Writes the render camera's world
/// transform (`m_TransformF`) and view (`m_View = inverse(world)`) from the HMD-composed pose; the
/// caller lets `SetupRenderCamera` build the view-projections from the new view, and the per-eye offset
/// applies on top.
///
/// This targets only the render camera copy, never the active/sim camera: outside gameplay the engine
/// parks its own camera at the teleport destination and streams the world from it, so writing the sim
/// camera would soft-lock the load. The render copy affects only what is displayed, so head-tracking it
/// lets the player look around a stable viewpoint without disturbing the teleport. The floating panel
/// world-locks to this same head-tracked render pose (see the panel's world-lock in `crate::hud`) so it
/// stays put in the world while the head looks about.
///
/// The caller gates on [`vr_loading_view_active`] (non-gameplay, VR source, base captured), so this only
/// resolves the frame's data: it no-ops on a VR frame that published no cockpit pose (a skipped render),
/// leaving the render copy as the engine left it.
fn head_track_render_camera(
    camera: &mut Camera,
    cockpit: Option<crate::headpose::xr::CockpitPose>,
    world_scale: f32,
) {
    let Some(cockpit) = cockpit else {
        return;
    };
    let Some((base_position, base_rotation)) = *LOADING_BASE.lock() else {
        return;
    };
    let base = glam::Mat4::from_rotation_translation(base_rotation, base_position);
    let world = compose_relative(base, &cockpit, world_scale);
    camera.m_TransformF = Matrix4::from(world);
    camera.m_View = Matrix4::from(world.inverse());
}

/// Compose a cockpit-frame HMD pose onto a base camera transform: rotate the head relative to the base
/// orientation and apply the room-scale head translation in the base frame. The identity cockpit pose
/// (looking straight ahead at the recenter neutral) reproduces the base transform exactly, so the
/// composition adds only the player's tracking delta.
fn compose_relative(
    base: glam::Mat4,
    cockpit: &crate::headpose::xr::CockpitPose,
    world_scale: f32,
) -> glam::Mat4 {
    let (_, base_rotation, base_position) = base.to_scale_rotation_translation();
    let position = base_position + base_rotation * (cockpit.position * world_scale);
    let orientation = base_rotation * cockpit.orientation;
    glam::Mat4::from_rotation_translation(orientation, position)
}

/// The upright loading base pose `(position, yaw-only rotation)`: the last gameplay viewpoint, held
/// frozen while the game parks its own camera outside gameplay so the render camera has a stable,
/// well-formed frame to head-track around during the load (issue #27). `None` until the first gameplay
/// frame captures one.
static LOADING_BASE: Mutex<Option<(glam::Vec3, glam::Quat)>> = Mutex::new(None);

/// Project a rotation onto yaw only (rotation about the world up axis), zeroing pitch and roll. A
/// degenerate (looking straight up or down) rotation falls back to identity yaw. Mirrors the recenter
/// baseline's yaw extraction ([`crate::vr`]).
fn yaw_only(rotation: glam::Quat) -> glam::Quat {
    let yaw = glam::Quat::from_xyzw(0.0, rotation.y, 0.0, rotation.w);
    if yaw.length_squared() > 1e-6 {
        yaw.normalize()
    } else {
        glam::Quat::IDENTITY
    }
}

/// The sim-phase camera matrix reader: `m_NextCameraContext`'s transform, which the game's
/// sim-phase camera update rewrites from its internal camera *after* the mod's render-phase
/// context patch. With the look input consumed by the headpose, that internal camera's yaw is
/// frozen, so every sim-side reader — the player aim control's raycasts and adjusted camera
/// matrix, the weapon aim-target queries, melee and grapple tasks — aimed at a fixed world
/// direction regardless of where the head looked. Overriding the getter's output hands them the
/// same transform the render camera uses.
///
/// Inside the grapple's scoped aim calls, the getter instead returns the grapple ray's camera
/// matrix ([`crate::controllers::aim::camera_matrix_override`]), so that the game's grapple
/// targeting casts from the off-hand.
#[detour(
    address = jc3gi::camera::game_camera_manager::GameCameraManager::GetCameraMatrix_ADDRESS
)]
fn game_camera_manager_get_camera_matrix(manager: *const GameCameraManager, matrix: *mut Matrix4) {
    GAME_CAMERA_MANAGER_GET_CAMERA_MATRIX
        .get()
        .unwrap()
        .call(manager, matrix);
    if let (Some(view), Some(matrix)) =
        (crate::controllers::aim::camera_matrix_override(), unsafe {
            matrix.as_mut()
        })
    {
        *matrix = view;
        return;
    }
    if !is_active() {
        return;
    }
    // Outside gameplay the render-phase hooks stop absolute placement and `LAST_CAMERA_WORLD` holds
    // the last gameplay pose, which no longer matches where the engine has moved its camera for the
    // teleport. Feeding that stale pose to the sim-phase readers would desync them from the engine's
    // own camera during the transition, so let the engine's value pass through (issue #27).
    if !in_gameplay() {
        return;
    }
    if let Some(data) = *LAST_CAMERA_WORLD.lock()
        && let Some(matrix) = unsafe { matrix.as_mut() }
    {
        *matrix = data;
    }
}

/// The last `dtf` seen by the active camera's `UpdateRender`, as bits (see [`last_dtf`]).
static LAST_DTF: AtomicU32 = AtomicU32::new(0);

/// The camera world transform last written to the active camera's `m_TransformT1` with the
/// headpose active, republished by [`game_camera_manager_get_camera_matrix`] so sim-phase readers
/// see the headpose camera. `None` until the camera hook first runs with the headpose active.
static LAST_CAMERA_WORLD: Mutex<Option<Matrix4>> = Mutex::new(None);

/// The active (main) camera's near/far clip planes (`m_Near`/`m_Far`), captured each frame from the
/// engine. The reconstruction override ([`crate::hooks::graphics_engine::reconstruction`]) keys on
/// these to recognize the main-view depth passes -- whose off-axis inverse it must substitute -- and
/// skip auxiliary cameras (reflections) with different planes. The engine writes a runtime far that
/// differs from the `Camera` constructor default (~40 km vs the constructor's 38.4 km), so gating that
/// override on a hardcoded config plane misses every main pass. `None` until the camera hook first runs.
static MAIN_CAMERA_PLANES: Mutex<Option<(f32, f32)>> = Mutex::new(None);

/// The active camera's near/far as of the last `Camera::UpdateRender`, or `None` before the first.
pub fn main_camera_planes() -> Option<(f32, f32)> {
    *MAIN_CAMERA_PLANES.lock()
}

/// The single source of truth for the VR near/far planes: the engine's live active-camera planes,
/// falling back to `fallback` (the configured VR planes) only until the first camera update. The
/// per-eye projections, the cull frustum, and the reconstruction gate all resolve through here, so the
/// eyes render, cull, and reconstruct against the same planes the engine is actually using.
pub fn main_camera_planes_or(fallback: (f32, f32)) -> (f32, f32) {
    main_camera_planes().unwrap_or(fallback)
}

#[detour(address = jc3gi::camera::camera_tree::CameraTree::UpdateRenderContexts_ADDRESS)]
fn camera_tree_update_render_contexts(
    tree: *mut c_void,
    camera_control_context: *mut CameraControlContext,
) {
    CAMERA_TREE_UPDATE_RENDER_CONTEXTS
        .get()
        .unwrap()
        .call(tree, camera_control_context);

    unsafe {
        *LAST_CAMERA_WORLD.lock() = None;
        let Some(local_character) = Character::GetLocalPlayerCharacter().as_mut() else {
            authored::reset(authored::Refusal::MissingChannel);
            return;
        };
        let Some(ccc) = camera_control_context.as_mut() else {
            authored::reset(authored::Refusal::MissingChannel);
            return;
        };

        let camera_settings = Config::lock_query(|c| c.camera);
        if !camera_settings.enabled {
            authored::reset(authored::Refusal::Disabled);
            return;
        }

        // Outside gameplay the engine owns the camera (issue #27): leave its render contexts alone so
        // its teleport can move the camera to the destination and stream the world from it. Absolute
        // placement stops entirely here; the player's head-tracking for the view is applied to the
        // render camera copy in `setup_render_camera`, which does not touch these contexts.
        if !in_gameplay() {
            authored::reset(authored::Refusal::OutsideGameplay);
            return;
        }

        let character_t0_matrix = glam::Mat4::from(if camera_settings.always_use_t1 {
            local_character.m_WorldMatrixT1
        } else {
            local_character.m_WorldMatrixT0
        });
        let character_t1_matrix = glam::Mat4::from(local_character.m_WorldMatrixT1);

        // The previous contexts get the previous-tick headpose placement and the next contexts
        // the current one, mirroring the T0/T1 pair in `camera_update_render`; without the
        // headpose (or before a valid anchor exists), the bone-derived positions apply.
        let headpose_active = is_active() && anchor().is_some();
        let (previous_position, next_position, previous_orientation, next_orientation) =
            if headpose_active {
                let cur = query();
                let prev = query_prev();
                (
                    camera_position(
                        &prev,
                        character_t0_matrix,
                        follow_turn(false),
                        &camera_settings,
                    ),
                    camera_position(
                        &cur,
                        character_t1_matrix,
                        follow_turn(true),
                        &camera_settings,
                    ),
                    Some(prev.orientation),
                    Some(cur.orientation),
                )
            } else {
                let head_matrix = head_matrix(local_character);
                let (left_eye_matrix, right_eye_matrix) = eye_matrices(local_character);
                (
                    calculate_head_position(
                        character_t0_matrix,
                        head_matrix,
                        left_eye_matrix,
                        right_eye_matrix,
                        camera_settings.use_eye_matrices,
                        &camera_settings,
                    ),
                    calculate_head_position(
                        character_t1_matrix,
                        head_matrix,
                        left_eye_matrix,
                        right_eye_matrix,
                        camera_settings.use_eye_matrices,
                        &camera_settings,
                    ),
                    None,
                    None,
                )
            };

        let selected = if headpose_active {
            let previous = query_prev();
            let current = query();
            let views = [
                authored::TrackedView {
                    world: glam::Mat4::from_rotation_translation(
                        previous.orientation,
                        previous_position,
                    ),
                    tracking_translation: previous.position
                        - anchor_prev().unwrap_or(previous.position),
                },
                authored::TrackedView {
                    world: glam::Mat4::from_rotation_translation(
                        current.orientation,
                        next_position,
                    ),
                    tracking_translation: current.position - anchor().unwrap_or(current.position),
                },
            ];
            authored::capture(local_character, views, camera_settings.always_use_t1)
        } else {
            authored::reset(authored::Refusal::NoTrackedHead);
            None
        };
        if let Some([previous, current]) = selected {
            patch_authored_context(
                &mut ccc.m_PreviousCameraContext,
                previous,
                camera_settings.blurs_enabled,
            );
            patch_authored_context(
                &mut ccc.m_PreviousRenderContext,
                previous,
                camera_settings.blurs_enabled,
            );
            patch_authored_context(
                &mut ccc.m_NextCameraContext,
                current,
                camera_settings.blurs_enabled,
            );
            patch_authored_context(
                &mut ccc.m_NextRenderContext,
                current,
                camera_settings.blurs_enabled,
            );
            *LAST_CAMERA_WORLD.lock() = Some(Matrix4::from(current));
            return;
        }
        if headpose_active {
            *LAST_CAMERA_WORLD.lock() = Some(Matrix4::from(glam::Mat4::from_rotation_translation(
                next_orientation.unwrap_or(glam::Quat::IDENTITY),
                next_position,
            )));
        } else {
            *LAST_CAMERA_WORLD.lock() = None;
        }

        let normal_reset = authored::normal_reset();
        let (previous_position, previous_orientation) = if normal_reset {
            (next_position, next_orientation)
        } else {
            (previous_position, previous_orientation)
        };
        patch_context(
            &mut ccc.m_PreviousCameraContext,
            previous_position,
            previous_orientation,
            camera_settings.blurs_enabled,
        );
        patch_context(
            &mut ccc.m_PreviousRenderContext,
            previous_position,
            previous_orientation,
            camera_settings.blurs_enabled,
        );
        patch_context(
            &mut ccc.m_NextCameraContext,
            next_position,
            next_orientation,
            camera_settings.blurs_enabled,
        );
        patch_context(
            &mut ccc.m_NextRenderContext,
            next_position,
            next_orientation,
            camera_settings.blurs_enabled,
        );
    }

    fn patch_authored_context(context: &mut CameraContext, world: glam::Mat4, blurs_enabled: bool) {
        context.m_CameraTransform = Matrix4::from(world);
        context.m_AlternateAimTransform = context.m_CameraTransform;
        context.m_ListenerTransform = context.m_CameraTransform;
        context.m_FOV = 90.0_f32.to_radians();
        if !blurs_enabled {
            context.m_MaxMotionBlur = 0.0;
            context.m_MotionBlurFactor = 0.0;
            context.m_RadialBlurFactor = 0.0;
        }
    }

    fn patch_context(
        context: &mut CameraContext,
        head_position: glam::Vec3,
        headpose_orientation: Option<glam::Quat>,
        blurs_enabled: bool,
    ) {
        if let Some(orientation) = headpose_orientation {
            let camera_world = glam::Mat4::from_rotation_translation(orientation, head_position);
            let m = camera_world.to_cols_array();
            for (i, &v) in m.iter().enumerate().take(15) {
                context.m_CameraTransform.data[i] = v;
            }
        } else {
            context.m_CameraTransform.data[12] = head_position.x;
            context.m_CameraTransform.data[13] = head_position.y;
            context.m_CameraTransform.data[14] = head_position.z;
        }
        context.m_AlternateAimTransform = context.m_CameraTransform;
        context.m_ListenerTransform = context.m_CameraTransform;
        context.m_FOV = 90.0_f32.to_radians();

        if !blurs_enabled {
            context.m_MaxMotionBlur = 0.0;
            context.m_MotionBlurFactor = 0.0_f32;
            context.m_RadialBlurFactor = 0.0;
        }
    }
}

fn head_matrix(character: &mut Character) -> glam::Mat4 {
    let mut head_matrix = Matrix4::default();
    unsafe {
        character.GetSafeBoneMatrix(SafeBoneIndex::HEAD, &mut head_matrix);
    }
    glam::Mat4::from(head_matrix)
}

fn eye_matrices(character: &mut Character) -> (glam::Mat4, glam::Mat4) {
    let mut left_eye_matrix = Matrix4::default();
    let mut right_eye_matrix = Matrix4::default();
    unsafe {
        if let Some(ac) = character.m_AnimatedModel.m_AnimationController.as_mut() {
            ac.GetBoneMatrix(
                ac.GetBoneIndex(hashlittle(b"fLeftEye") as u32),
                &mut left_eye_matrix,
            );
            ac.GetBoneMatrix(
                ac.GetBoneIndex(hashlittle(b"fRightEye") as u32),
                &mut right_eye_matrix,
            );
        }
    }
    (
        glam::Mat4::from(left_eye_matrix),
        glam::Mat4::from(right_eye_matrix),
    )
}

fn calculate_head_position(
    character_matrix: glam::Mat4,
    head_matrix: glam::Mat4,
    left_eye_matrix: glam::Mat4,
    right_eye_matrix: glam::Mat4,
    use_eye_matrices: bool,
    camera_settings: &config::CameraConfig,
) -> glam::Vec3 {
    let (_, character_rotation, _character_position) =
        character_matrix.to_scale_rotation_translation();

    if use_eye_matrices {
        let left_eye_worldspace_matrix = character_matrix * left_eye_matrix;
        let (_, _, left_eye_position) = left_eye_worldspace_matrix.to_scale_rotation_translation();

        let right_eye_worldspace_matrix = character_matrix * right_eye_matrix;
        let (_, _, right_eye_position) =
            right_eye_worldspace_matrix.to_scale_rotation_translation();

        (left_eye_position + right_eye_position) / 2.0
    } else {
        let head_worldspace_matrix = character_matrix * head_matrix;
        let (_, head_rotation, mut head_position) =
            head_worldspace_matrix.to_scale_rotation_translation();

        head_position += head_rotation * camera_settings.head_offset;
        head_position += character_rotation * camera_settings.body_offset;

        head_position
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::headpose::xr::CockpitPose;

    /// The identity cockpit pose (looking straight ahead at the recenter neutral, no room-scale
    /// translation) reproduces the engine base transform exactly, so the relative composition adds
    /// nothing on its own.
    #[test]
    fn identity_cockpit_reproduces_base() {
        let base = glam::Mat4::from_rotation_translation(
            glam::Quat::from_rotation_y(0.7),
            glam::Vec3::new(3.0, 1.0, -4.0),
        );
        let cockpit = CockpitPose {
            position: glam::Vec3::ZERO,
            orientation: glam::Quat::IDENTITY,
        };
        let composed = compose_relative(base, &cockpit, 1.0);
        assert!((composed - base).abs_diff_eq(glam::Mat4::ZERO, 1e-5));
    }

    /// The cockpit orientation rotates relative to the base orientation, and the room-scale head
    /// translation is applied in the base frame (a 90° base yaw carries the cockpit's forward lean
    /// into the base's facing).
    #[test]
    fn cockpit_delta_composes_in_base_frame() {
        let base_rotation = glam::Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let base = glam::Mat4::from_rotation_translation(base_rotation, glam::Vec3::ZERO);
        let cockpit = CockpitPose {
            position: glam::Vec3::new(0.0, 0.0, -1.0),
            orientation: glam::Quat::from_rotation_x(0.3),
        };
        let composed = compose_relative(base, &cockpit, 1.0);
        let (_, orientation, position) = composed.to_scale_rotation_translation();
        // +Y yaw of 90° maps the cockpit's forward -Z to -X.
        assert!((position - glam::Vec3::new(-1.0, 0.0, 0.0)).length() < 1e-5);
        assert!(orientation.abs_diff_eq(base_rotation * cockpit.orientation, 1e-5));
    }

    /// The world scale multiplies only the room-scale translation, not the orientation.
    #[test]
    fn world_scale_scales_translation_only() {
        let base = glam::Mat4::IDENTITY;
        let cockpit = CockpitPose {
            position: glam::Vec3::new(0.0, 0.0, -0.5),
            orientation: glam::Quat::IDENTITY,
        };
        let composed = compose_relative(base, &cockpit, 2.0);
        let (_, orientation, position) = composed.to_scale_rotation_translation();
        assert!((position - glam::Vec3::new(0.0, 0.0, -1.0)).length() < 1e-6);
        assert!(orientation.abs_diff_eq(glam::Quat::IDENTITY, 1e-6));
    }

    /// A rig shaped like a standing character: the neck a little below and behind the head anchor,
    /// the eyes forward of and above the neck.
    fn standing_rig() -> EyeRig {
        EyeRig {
            neck_delta: glam::Vec3::new(0.0, -0.08, 0.02),
            arm: glam::Vec3::new(0.0, 0.12, -0.1),
            body_offset: glam::Vec3::ZERO,
            view_turn: glam::Quat::IDENTITY,
        }
    }

    fn pose(position: glam::Vec3, orientation: glam::Quat) -> HeadPose {
        HeadPose {
            position,
            orientation,
        }
    }

    /// Under the VR source, looking down must not move the eye point: the tracked translation already
    /// carries the player's neck pivot, so the orientation alone contributes nothing.
    #[test]
    fn tracked_pitch_does_not_move_the_eye() {
        let body = glam::Quat::from_rotation_y(0.9);
        let rig = standing_rig();
        let position = glam::Vec3::new(5.0, 1.7, -2.0);
        let level = eye_position(&pose(position, body), body, &rig, NeckModel::Tracked);
        for pitch in [
            -std::f32::consts::FRAC_PI_4,
            -std::f32::consts::FRAC_PI_2,
            0.6,
        ] {
            let pitched = eye_position(
                &pose(position, body * glam::Quat::from_rotation_x(pitch)),
                body,
                &rig,
                NeckModel::Tracked,
            );
            assert!(
                pitched.distance(level) < 1e-6,
                "a pitch of {pitch} rad moved the tracked eye by {:?}",
                pitched - level
            );
        }
    }

    /// Under the VR source the eye point follows the tracked translation one to one: a pose that
    /// dropped and moved forward (as a real head does when looking down) moves the eye by exactly
    /// that much and no further.
    #[test]
    fn tracked_eye_follows_the_pose_translation_exactly() {
        let body = glam::Quat::from_rotation_y(-0.4);
        let rig = standing_rig();
        let rest = glam::Vec3::new(1.0, 1.7, 3.0);
        let tracked_delta = body * glam::Vec3::new(0.0, -0.06, -0.08);
        let looking_down = body * glam::Quat::from_rotation_x(-std::f32::consts::FRAC_PI_4);
        let neutral_eye = eye_position(&pose(rest, body), body, &rig, NeckModel::Tracked);
        let down_eye = eye_position(
            &pose(rest + tracked_delta, looking_down),
            body,
            &rig,
            NeckModel::Tracked,
        );
        assert!((down_eye - neutral_eye - tracked_delta).length() < 1e-6);
    }

    /// The tracked arm is the rest-pose anchor-to-eyes offset carried by the body frame, so it turns
    /// with the character's facing.
    #[test]
    fn tracked_arm_rides_the_body_frame() {
        let body = glam::Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let rig = standing_rig();
        let eye = eye_position(
            &pose(glam::Vec3::ZERO, glam::Quat::IDENTITY),
            body,
            &rig,
            NeckModel::Tracked,
        );
        assert!((eye - (rig.neck_delta + body * rig.arm)).length() < 1e-6);
    }

    /// A body turned under the view leaves the tracked arm where the view has it: the headset's
    /// translation already carries the player's eyes round with a torso turn.
    #[test]
    fn tracked_arm_stays_in_the_view_frame_when_the_body_turns_under_it() {
        let view = glam::Quat::from_rotation_y(0.3);
        let body = glam::Quat::from_rotation_y(1.4);
        let rig = EyeRig {
            view_turn: body.inverse() * view,
            ..standing_rig()
        };
        let eye = eye_position(
            &pose(glam::Vec3::ZERO, glam::Quat::IDENTITY),
            body,
            &rig,
            NeckModel::Tracked,
        );
        assert!((eye - (rig.neck_delta + view * rig.arm)).length() < 1e-5);
    }

    /// The flatscreen sim keeps its synthetic neck: pitching the head down swings the eyes forward
    /// and down about the neck, which its anchor-only position cannot otherwise express.
    #[test]
    fn synthetic_pitch_swings_the_eye_about_the_neck() {
        let rig = standing_rig();
        let position = glam::Vec3::new(0.0, 1.7, 0.0);
        let level = eye_position(
            &pose(position, glam::Quat::IDENTITY),
            glam::Quat::IDENTITY,
            &rig,
            NeckModel::Synthetic,
        );
        let down = eye_position(
            &pose(
                position,
                glam::Quat::from_rotation_x(-std::f32::consts::FRAC_PI_4),
            ),
            glam::Quat::IDENTITY,
            &rig,
            NeckModel::Synthetic,
        );
        assert!(down.y < level.y, "the eye drops when looking down");
        let neck = position + rig.neck_delta;
        assert!(
            (down.distance(neck) - level.distance(neck)).abs() < 1e-6,
            "the eye stays on the arm's sphere about the neck"
        );
    }
}
