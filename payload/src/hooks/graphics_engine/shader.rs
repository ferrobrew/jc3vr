//! Detour on `Graphics::CreateFragmentProgram` to neutralise the per-eye sun-shadow PCF rotation hash.
//!
//! The opaque sun-shadow resolve rotates its 38-tap Poisson PCF disk by
//! `frac(sin(dot(SV_Position, k)) * 43758.5)` -- a hash of the screen pixel. In stereo the same world
//! point lands on a different pixel in each eye, so the two eyes average a different tap set: the
//! shadow shimmers/grains differently between the eyes (and the alpha-tested foliage with it). The
//! world-space shadow *lookup* uses the interpolated world position and is identical per eye, so the
//! fix is to make the rotation eye-invariant -- zero the two seed constants in that `dp2`, so every
//! pixel (and both eyes) uses the same unrotated 38-tap PCF. With 38 taps the look change is
//! negligible. `12.9898` occurs only in this instruction (159 shaders per bundle), so the patch is
//! exactly targeted.
//!
//! The patch is applied to the DXBC in-flight, before `CreatePixelShader` (which copies the bytecode,
//! so a patched copy only needs to outlive the call). Editing the bytecode invalidates the DXBC
//! container checksum, and the D3D stack under Proton rejects a blob whose stored hash no longer
//! matches -- so the patched copy's checksum is recomputed (`dxbc_stereo::refresh_checksum`) before
//! the call.
//! It therefore affects only shaders created after
//! the hook installs: with launch-time injection that is every shader; with mid-session injection,
//! trigger a shader reload (e.g. change the shadow-quality graphics setting) so the shadow shaders are
//! recreated through the hook. [`patched_count`] (shown in the debug UI) makes it clear whether the
//! hook is catching anything.

use std::{
    ffi::c_void,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use detours_macro::detour;
use dxbc_stereo::{AmbientOcclusionSampling, LocalLights, refresh_checksum};
use jc3gi::graphics_engine::{
    draw::{
        CreateDomainProgramParams, CreateFragmentProgramParams, CreateHullProgramParams,
        CreateVertexProgramParams, HDomainProgram_t, HFragmentProgram_t, HHullProgram_t,
    },
    graphics_engine::GraphicsEngine,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    hooks::graphics_engine::ss_decal,
    stereo::{
        single_pass::{
            VsTransform, ambient_occlusion, ao_volumes, eye_camera, is_terrain_shadow_pass,
            post_filters, screen_space_reflection, set_patch_pending, terrain_active,
            water_pixel_variants,
        },
        switches::ShaderBuild,
    },
};

/// The 16-byte `dp2` immediate `l(12.9898, 78.233, 0, 0)` -- the screen-pixel PCF rotation seed. The
/// first eight bytes are the two multiplier constants; zeroing them makes the dot product (and thus
/// the rotation angle) a constant.
const SEED: [u8; 16] = [
    0x39, 0xd6, 0x4f, 0x41, // 12.9898
    0x4c, 0x77, 0x9c, 0x42, // 78.233
    0x00, 0x00, 0x00, 0x00, // 0.0
    0x00, 0x00, 0x00, 0x00, // 0.0
];

static PATCHED: AtomicUsize = AtomicUsize::new(0);
/// The number of screen-space decal permutations whose depth fetch has been biased since injection --
/// twelve per bundle load. Read on eject to decide whether the pristine shaders need restoring.
static SSDECAL_BIASED: AtomicUsize = AtomicUsize::new(0);
/// How many composite permutations have had their screen overlays mapped per eye this session.
static OVERLAYS_LOCALIZED: AtomicUsize = AtomicUsize::new(0);
/// How many composite permutations have had their bloom lookups kept per eye this session.
static BLOOM_LOCALIZED: AtomicUsize = AtomicUsize::new(0);
static RELOAD_REQUESTED: AtomicBool = AtomicBool::new(false);
/// The state the live shader set was created under by the last reload, or `None` before the first
/// reload, after a failed one, or when the configuration changed while the shaders were being
/// re-created (so some may have been created under each state).
static BUILT_WITH: Mutex<Option<ShaderBuild>> = Mutex::new(None);
/// Whether single-pass was active when the live shader set was last created by a reload. Off at
/// injection: a game injected after it loaded its shaders holds the pristine set, and one injected
/// before creates them before any VR session runs, while single-pass is inert.
static CREATED_SINGLE_PASS: AtomicBool = AtomicBool::new(false);

/// Read a shader program's engine debug name (a null-terminated C string in
/// `Create*ProgramParams.m_Name`), or `None` if the pointer is null. Used to log which tessellation
/// shaders the terrain hooks transform, to catch structural over-capture of non-terrain families.
fn program_name(m_name: *const u8) -> Option<String> {
    (!m_name.is_null()).then(|| {
        unsafe { std::ffi::CStr::from_ptr(m_name.cast::<std::ffi::c_char>()) }
            .to_string_lossy()
            .into_owned()
    })
}

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&CREATE_FRAGMENT_PROGRAM_BINDER)
        .with_static_binder(&CREATE_VERTEX_PROGRAM_BINDER)
        .with_static_binder(&CREATE_HULL_PROGRAM_BINDER)
        .with_static_binder(&CREATE_DOMAIN_PROGRAM_BINDER)
}

/// Detour on `Graphics::CreateVertexProgram` for single-pass stereo: census the vertex-shader rewrite
/// and, when single-pass is active, substitute the patched bytecode.
///
/// When single-pass (or its census-only dry-run) is enabled, run [`dxbc_stereo::patch_vertex_shader`]
/// on the incoming DXBC and tally the outcome ([`crate::stereo::single_pass::record_patch_outcome`]),
/// so the debug UI reports how the rewriter fares against the game's real shader set. Decide which of
/// the single-pass transforms the shader is to get ([`crate::stereo::single_pass::decide_vs_transform`])
/// -- that decision reads the shader's engine name, which exists nowhere else -- and remember it
/// against the bytecode for the D3D-level re-create path. When single-pass is *active* (master on,
/// dry-run off, capability present), also point the params at the patched copy for the
/// (bytecode-copying) `CreateVertexShader` call -- the copy is kept alive in `saved` and the caller's
/// pointer restored afterwards, exactly as the fragment hook does. The patched shader reads
/// its position from `cb13` (see [`crate::hooks::graphics_engine::single_pass`]); in dry-run the copy
/// is discarded and rendering is unchanged. Only sees shaders created after it installs; use the
/// shader-reload button for shaders loaded before injection.
#[detour(address = jc3gi::graphics_engine::draw::CreateVertexProgram_ADDRESS)]
fn create_vertex_program(
    device: *mut c_void,
    params: *mut CreateVertexProgramParams,
) -> *mut c_void {
    // The patched blob is *larger* than the original (added SFI0 chunk, cb13 declaration, prologue,
    // signature entries), so unlike the in-place fragment patch this must repoint `m_Size` as well as
    // `m_Code` -- otherwise the engine hands the D3D stack a truncated container whose chunk table
    // runs past the declared length. Both are restored after the (bytecode-copying) call.
    let mut saved: Option<(*const u8, u64, Vec<u8>)> = None;
    // Hoisted out of the census block so it can be handed to `set_patch_pending` below, which records
    // it against the `ID3D11VertexShader` the engine is about to create -- the only join between a
    // shader's engine name and the pointer the draw-time paths see.
    let mut name: Option<String> = None;
    // Whether the substitute is a pinned-eye rewrite, which the `CreateVertexShader` detour tags
    // rather than recording as patched.
    let mut pinned = false;
    // Skip the census once eject begins, matching the fragment hook. The eject restore bounces the
    // bundle -- two `LoadShaderBundle` calls, so every vertex shader passes through here twice -- on
    // the game thread, which blocks in `shutdown_from_game` throughout. Running the full rewrite ~900
    // times there is a multi-second freeze on every F5, on a path already racing teardown, and it buys
    // nothing: the substitution is inert during shutdown, so the rewrite's only product is a tally
    // nobody will read.
    let census = !crate::is_shutting_down()
        && Config::lock_query(|c| {
            c.stereo.single_pass.enabled || c.stereo.single_pass.patch_dryrun
        });
    if census
        && let Some(p) = unsafe { params.as_mut() }
        && !p.m_Code.is_null()
        && p.m_Size >= 4
    {
        let code = unsafe { std::slice::from_raw_parts(p.m_Code, p.m_Size as usize) };
        // The shader's engine name (a null-terminated C string) drives the reprojection allowlist and
        // the baked-cb block families' decline.
        name = program_name(p.m_Name);
        // A family whose draws a baked-view-projection block intercept re-issues per eye must stay
        // pristine: the remap would claim it on a `cb0[4]` reference that is not its position path,
        // leaving both eyes on the collapsed centre view *and* standing the intercept down. It is
        // counted as having no per-eye reference, which is what it has -- no per-eye *clip* reference.
        let owned = crate::stereo::single_pass::baked_cb_block_owns_vs(name.as_deref(), code);
        let outcome = if owned {
            Err(dxbc_stereo::DxbcError::NoPerEyeReferences)
        } else {
            crate::stereo::single_pass::remap_vertex_shader(code)
        };
        crate::stereo::single_pass::record_patch_outcome(&outcome, name.as_deref());
        // Which transform this shader gets is decided here, because here is the only place its engine
        // name exists. The decision is then remembered against the bytecode, so the `CreateVertexShader`
        // detour re-applies *this* decision to a shader re-created through a path that skips this hook,
        // rather than deriving a `cb0` remap it has no name to override.
        //
        // Decided (and remembered) whether or not single-pass is active: the dry-run census is the only
        // pass over the shader set that some shaders get, and turning single-pass on afterwards creates
        // no new programs to decide from.
        let transform = if owned {
            VsTransform::None
        } else {
            crate::stereo::single_pass::decide_vs_transform(name.as_deref(), code, &outcome)
        };
        crate::stereo::single_pass::remember_vs_transform(code, transform);
        pinned = transform == VsTransform::Pinned;
        // The bark and foliage blocks' single draw binds a reprojected variant of each eye-pass
        // permutation, attached when the shader is created. Built whenever single-pass is live, so
        // the switch applies without a reload.
        if crate::stereo::single_pass::active()
            && crate::stereo::single_pass::vegetation_once::variants::remember(
                name.as_deref(),
                code,
            )
        {
            crate::stereo::single_pass::ensure_viewport_detours();
        }
        let substitute = if crate::stereo::single_pass::active() {
            crate::stereo::single_pass::apply_vs_transform(
                transform,
                code,
                outcome.ok(),
                name.as_deref(),
            )
        } else {
            None
        };
        if let Some(blob) = substitute {
            let len = blob.len() as u64;
            saved = Some((p.m_Code, p.m_Size, blob));
            p.m_Code = saved.as_ref().expect("just set").2.as_ptr();
            p.m_Size = len;
        }
    }

    // Flag the patched creation so the CreateVertexShader detour records the resulting shader for the
    // draw-time patched/unpatched gating. Install the COM detours *first*, before the trampoline
    // creates the D3D shader: otherwise a shader created before the detours' lazy first-frame install
    // (e.g. a character shader loaded at level start) is patched at the blob level but never recorded,
    // so `BOUND_VS_PATCHED` stays false and its draw is never doubled -- it renders in one eye.
    if saved.is_some() {
        crate::stereo::single_pass::ensure_viewport_detours();
        if pinned {
            crate::stereo::single_pass::set_pin_pending(true);
        } else {
            set_patch_pending(true, name.as_deref());
        }
    }
    let result = CREATE_VERTEX_PROGRAM.get().unwrap().call(device, params);
    set_patch_pending(false, None);
    crate::stereo::single_pass::set_pin_pending(false);

    if let Some((original_code, original_size, _copy)) = saved
        && let Some(p) = unsafe { params.as_mut() }
    {
        p.m_Code = original_code;
        p.m_Size = original_size;
    }
    result
}

#[detour(address = jc3gi::graphics_engine::draw::CreateFragmentProgram_ADDRESS)]
fn create_fragment_program(
    device: *mut c_void,
    params: *mut CreateFragmentProgramParams,
) -> *mut HFragmentProgram_t {
    let identity = if crate::is_shutting_down() {
        None
    } else {
        unsafe { params.as_ref() }.and_then(|p| unsafe {
            crate::stereo::single_pass::shader_identity::begin(p.m_Code, p.m_Size)
        })
    };
    // When enabled and a target site is present, point the params at a patched copy of the bytecode
    // for the duration of the (bytecode-copying) CreatePixelShader call, then restore the caller's
    // pointer. `saved` keeps the patched copy alive across the call.
    let mut saved: Option<(*const u8, u64, Vec<u8>)> = None;
    let patch_pcf = Config::lock_query(|c| c.stereo.patch_shadow_pcf_hash);
    // The screen-space decal permutations' depth-UV bias, the one rewrite here that *grows* the blob.
    let bias_ssdecal = ss_decal::shader_rewrite_enabled();
    // The final composite's vignette and lens dirt, mapped over one eye instead of the whole
    // double-wide target. Baked into the bytecode, so it is only right while the collapse is live;
    // the debug UI requests a reload when the switch changes.
    let localize_overlays = crate::stereo::single_pass::collapse_active()
        && Config::lock_query(|c| c.stereo.single_pass.composite_overlays_per_eye);
    // The ambient-occlusion generation pass computed for each eye's half. Baked into the bytecode
    // like the overlays, and decided for every fragment program so the SSAO block knows whether the
    // generation shader it will run carries it.
    // The composite's bloom lookups, kept to each eye's half. Baked into the bytecode like the
    // overlays.
    let localize_bloom = crate::stereo::single_pass::collapse_active()
        && Config::lock_query(|c| c.stereo.single_pass.glare_per_eye);
    let localize_ambient_occlusion = crate::stereo::single_pass::collapse_active()
        && Config::lock_query(|c| c.stereo.single_pass.ssao_per_eye);
    // The SSAO passes' sampling computed in the shader, in every mode: under the collapse as part of
    // the rewrite above, elsewhere as a rewrite of its own.
    let exact_sampling = Config::lock_query(|c| c.stereo.single_pass.ssao_exact_sampling);
    let ambient_occlusion_sampling = if exact_sampling {
        AmbientOcclusionSampling::Exact
    } else {
        AmbientOcclusionSampling::DoubleWide
    };
    // The ambient-occlusion volumes mapped through each eye's camera. Baked into the bytecode like
    // the ambient occlusion; its vertex shader's reprojection is decided by name with the same switch.
    let localize_ao_volumes = crate::stereo::single_pass::collapse_active()
        && Config::lock_query(|c| c.stereo.single_pass.ao_volumes_per_eye);
    // The screen-space reflection ray march traced for each eye's half, and its blur kept to it.
    // Baked into the bytecode like the ambient occlusion, and decided for every fragment program so
    // the reflection block knows whether the ray march it will run carries it.
    let localize_reflections = crate::stereo::single_pass::collapse_active()
        && Config::lock_query(|c| c.stereo.single_pass.ssr_per_eye);
    // Every pixel shader that reads the camera position picks its eye's copy per pixel. Applied last,
    // over whatever the other rewrites produced, since it composes with all of them.
    let localize_camera = crate::stereo::single_pass::collapse_active()
        && Config::lock_query(|c| c.stereo.single_pass.pixel_camera_per_eye);
    let local_lights = if Config::lock_query(|c| c.stereo.single_pass.local_lights_per_eye) {
        LocalLights::PerEye
    } else {
        LocalLights::Unchanged
    };
    // Every screen-door pattern hashed from the pixel position is laid out from its eye's origin.
    // Applied after the camera rewrite, whose buffer carries the eye boundary it reads.
    let localize_screen_door = crate::stereo::single_pass::collapse_active()
        && Config::lock_query(|c| c.stereo.single_pass.screen_door_per_eye);
    // The tree impostors hash theirs from their own projected position; the same buffer's boundary
    // lays it out per eye.
    let localize_projected_screen_door = crate::stereo::single_pass::collapse_active()
        && Config::lock_query(|c| c.stereo.single_pass.tree_impostor_screen_door_per_eye);
    // The velocity pass's motion vectors are written in each eye's screen space, from rows of the
    // same buffer. Applied after the screen-door rewrites, whose buffer declaration it reads.
    let localize_velocity = crate::stereo::single_pass::collapse_active()
        && Config::lock_query(|c| c.stereo.single_pass.velocity_per_eye);
    let ssao_far_early_out = Config::lock_query(|c| c.stereo.single_pass.ssao_far_early_out);
    let mut rewritten_screen_pass = None;
    if !crate::is_shutting_down()
        && let Some(p) = unsafe { params.as_ref() }
        && !p.m_Code.is_null()
        && let Ok(size) = usize::try_from(p.m_Size)
    {
        let code = unsafe { std::slice::from_raw_parts(p.m_Code, size) };
        rewritten_screen_pass = ambient_occlusion::rewrite_fragment_program(
            code,
            localize_ambient_occlusion,
            ambient_occlusion_sampling,
        )
        .or_else(|| {
            (exact_sampling && !localize_ambient_occlusion)
                .then(|| ambient_occlusion::snap_fragment_program(code))
                .flatten()
        })
        .or_else(|| screen_space_reflection::rewrite_fragment_program(code, localize_reflections))
        .or_else(|| ao_volumes::rewrite_fragment_program(code, localize_ao_volumes));
        // The SSAO generation's far quads skip the taps, over whichever rewrite above it carries.
        if ssao_far_early_out
            && let Some(skipped) = ambient_occlusion::skip_far_fragment_program(
                rewritten_screen_pass.as_deref().unwrap_or(code),
            )
        {
            rewritten_screen_pass = Some(skipped);
        }
    }
    // Skip patching once eject begins, so the shader restore's bounce re-creates pristine fragment
    // shaders (matching the vertex side) and the clean game keeps none of the mod's edits.
    if let Some(blob) = rewritten_screen_pass
        && let Some(p) = unsafe { params.as_mut() }
    {
        let len = blob.len() as u64;
        saved = Some((p.m_Code, p.m_Size, blob));
        p.m_Code = saved.as_ref().expect("just set").2.as_ptr();
        p.m_Size = len;
    } else if !crate::is_shutting_down()
        && (patch_pcf || bias_ssdecal || localize_overlays || localize_bloom)
        && let Some(p) = unsafe { params.as_mut() }
    {
        let size = p.m_Size as usize;
        if !p.m_Code.is_null() && size >= SEED.len() {
            let code = unsafe { std::slice::from_raw_parts(p.m_Code, size) };
            let mut copy = code.to_vec();
            let pcf = if patch_pcf { zero_seeds(&mut copy) } else { 0 };
            if pcf > 0 {
                // A raw byte-patch leaves the DXBC container checksum stale; D3D consumers that
                // validate it (the translation layers under Proton do) reject the blob, so the
                // shadow shaders fail to create and the scene renders broken. Refresh the checksum
                // so the patched bytecode is a valid container.
                refresh_checksum(&mut copy);
                PATCHED.fetch_add(pcf, Ordering::Relaxed);
            }
            // Structural, not name-gated: the rewrite declines every shader without the decals'
            // projective-UV depth fetch, and reassembles (and re-checksums) the container itself.
            let mut biased = false;
            if bias_ssdecal && let Ok(blob) = dxbc_stereo::bias_ssdecal_depth_uv(&copy) {
                copy = blob;
                biased = true;
                // `CreateFragmentProgramParams` carries no debug name (unlike the vertex/hull/domain
                // forms), so the running count is the only handle on which permutations were caught.
                let count = SSDECAL_BIASED.fetch_add(1, Ordering::Relaxed) + 1;
                tracing::debug!(
                    target: "single_pass",
                    "ssdecal: depth-UV biased ({count} permutations so far)",
                );
            }
            // Structural as well: the rewrite declines every shader that binds neither overlay.
            let mut localized = false;
            if localize_overlays && let Ok(blob) = dxbc_stereo::localize_screen_overlays(&copy) {
                copy = blob;
                localized = true;
                let count = OVERLAYS_LOCALIZED.fetch_add(1, Ordering::Relaxed) + 1;
                tracing::debug!(
                    target: "single_pass",
                    "composite: screen overlays mapped per eye ({count} permutations so far)",
                );
            }
            if localize_bloom && let Ok(blob) = dxbc_stereo::localize_composite_bloom(&copy) {
                copy = blob;
                localized = true;
                let count = BLOOM_LOCALIZED.fetch_add(1, Ordering::Relaxed) + 1;
                tracing::debug!(
                    target: "single_pass",
                    "composite: bloom lookups kept per eye ({count} permutations so far)",
                );
            }
            if pcf > 0 || biased || localized {
                // Unlike the in-place byte patches, the decal rewrite splices an instruction in, so
                // `m_Size` has to move with `m_Code` or the engine hands D3D a container whose chunk
                // table runs past the declared length. Both are restored after the copying call.
                let len = copy.len() as u64;
                saved = Some((p.m_Code, p.m_Size, copy));
                p.m_Code = saved.as_ref().expect("just set").2.as_ptr();
                p.m_Size = len;
            }
        }
    }

    // The water's per-eye variants are built from the engine's own bytecode for every shader the
    // other rewrites left alone. The camera and screen-door rewrites do not count: the water's
    // per-eye draws stage each eye's camera rows into `cb0` themselves, which the engine's bytecode
    // reads, and the only pattern the water hashes from its pixel position is the shadow rotation
    // `patch_shadow_pcf_hash` zeroes.
    let substituted_before_camera = saved.is_some();
    let mut rewrote_velocity = false;
    if !crate::is_shutting_down()
        && let Some(p) = unsafe { params.as_mut() }
        && !p.m_Code.is_null()
        && let Ok(size) = usize::try_from(p.m_Size)
    {
        let current = unsafe { std::slice::from_raw_parts(p.m_Code, size) };
        let camera = eye_camera::rewrite_fragment_program(current, localize_camera, local_lights);
        let screen_door = eye_camera::rewrite_screen_door(
            camera.as_deref().unwrap_or(current),
            localize_screen_door,
        );
        let projected_screen_door = eye_camera::rewrite_projected_screen_door(
            screen_door
                .as_deref()
                .or(camera.as_deref())
                .unwrap_or(current),
            localize_projected_screen_door,
        );
        let earlier = projected_screen_door.or(screen_door).or(camera);
        let velocity =
            eye_camera::rewrite_velocity(earlier.as_deref().unwrap_or(current), localize_velocity);
        rewrote_velocity = velocity.is_some();
        if let Some(blob) = velocity.or(earlier) {
            let len = blob.len() as u64;
            let (original_code, original_size) = saved
                .take()
                .map_or((p.m_Code, p.m_Size), |(code, size, _)| (code, size));
            saved = Some((original_code, original_size, blob));
            p.m_Code = saved.as_ref().expect("just set").2.as_ptr();
            p.m_Size = len;
        }
    }

    let result = CREATE_FRAGMENT_PROGRAM.get().unwrap().call(device, params);
    if rewrote_velocity
        && unsafe { result.as_ref() }
            .and_then(|program| program.m_Program.as_ref())
            .is_none()
    {
        tracing::warn!(
            target: "single_pass",
            "velocity: the engine could not create a velocity pixel shader from its rewrite",
        );
    }
    drop(identity);
    let engine_code = saved
        .as_ref()
        .map(|&(code, size, _)| (code, size))
        .or_else(|| unsafe { params.as_ref() }.map(|p| (p.m_Code, p.m_Size)));
    if !substituted_before_camera
        && !crate::is_shutting_down()
        && let Some((code, size)) = engine_code
        && !code.is_null()
        && let Ok(size) = usize::try_from(size)
        && let Some(shader) = unsafe { result.as_ref() }.and_then(|p| p.m_Program.as_ref())
        && let Err(error) = water_pixel_variants::register(shader, unsafe {
            std::slice::from_raw_parts(code, size)
        })
    {
        tracing::warn!("{error:#}");
    }

    // The glare chain's filters get an eye-half variant the draw detour binds while the chain runs.
    // Built whenever single-pass is on, so the switch applies without a reload.
    if !crate::is_shutting_down()
        && Config::lock_query(|c| c.stereo.single_pass.enabled)
        && let Some(p) = unsafe { params.as_ref() }
        && !p.m_Code.is_null()
        && let Ok(size) = usize::try_from(p.m_Size)
        && let Some(shader) = unsafe { result.as_ref() }.and_then(|p| p.m_Program.as_ref())
        && let Err(error) = post_filters::register(shader, unsafe {
            std::slice::from_raw_parts(p.m_Code, size)
        })
    {
        tracing::warn!("{error:#}");
    }

    if let Some((original_code, original_size, _copy)) = saved
        && let Some(p) = unsafe { params.as_mut() }
    {
        p.m_Code = original_code;
        p.m_Size = original_size;
    }
    result
}

/// Detour on `Graphics::CreateHullProgram` for the single-pass terrain path: when terrain single-pass
/// is active, forward the eye index through the tessellation control-point phase by widening the hull's
/// `TEXCOORD3` lane to `.xyz` ([`dxbc_stereo::forward_eye_hull_shader`]). Gated structurally -- only
/// hulls that carry the free lane transform; the rest return an error and are left untouched -- since
/// the hull is created without a paired-VS identity to name-gate on. The patched blob replaces the
/// bytecode for the (copying) `CreateHullShader` call, then the caller's pointer is restored.
#[detour(address = jc3gi::graphics_engine::draw::CreateHullProgram_ADDRESS)]
fn create_hull_program(
    device: *mut c_void,
    params: *mut CreateHullProgramParams,
) -> *mut HHullProgram_t {
    let mut saved: Option<(*const u8, u64, Vec<u8>)> = None;
    if terrain_active()
        && let Some(p) = unsafe { params.as_mut() }
        && !p.m_Code.is_null()
        && p.m_Size >= 4
    {
        let name = program_name(p.m_Name);
        // Skip shadow-pass terrain: it renders from the light's view, so eye-forwarding its lane feeds
        // a shadow-atlas draw an eye it must not have.
        if !is_terrain_shadow_pass(name.as_deref()) {
            let code = unsafe { std::slice::from_raw_parts(p.m_Code, p.m_Size as usize) };
            if let Ok(blob) = dxbc_stereo::forward_eye_hull_shader(code) {
                let len = blob.len() as u64;
                tracing::info!(target: "single_pass", stage = "hull", name = ?name, "terrain: eye-lane forwarded");
                saved = Some((p.m_Code, p.m_Size, blob));
                p.m_Code = saved.as_ref().expect("just set").2.as_ptr();
                p.m_Size = len;
                crate::stereo::single_pass::record_hull_forwarded();
            }
        }
    }

    let result = CREATE_HULL_PROGRAM.get().unwrap().call(device, params);

    if crate::stereo::single_pass::shader_identity::requested()
        && let Some(program) =
            unsafe { result.as_ref() }.and_then(|holder| holder.m_Program.as_ref())
        && let Some(params) = unsafe { params.as_ref() }
        && !params.m_Code.is_null()
        && (4..=16 * 1024 * 1024).contains(&params.m_Size)
    {
        crate::screenshot::harness::patch_draw::tag(program, unsafe {
            std::slice::from_raw_parts(params.m_Code, params.m_Size as usize)
        });
    }
    if let Some((original_code, original_size, _copy)) = saved
        && let Some(p) = unsafe { params.as_mut() }
    {
        p.m_Code = original_code;
        p.m_Size = original_size;
    }
    result
}

/// Detour on `Graphics::CreateDomainProgram` for the single-pass terrain path: when terrain single-pass
/// is active, reproject the domain shader's clip by the per-eye `M_eye` and route to the eye's viewport
/// ([`dxbc_stereo::reproject_domain_shader`]), reading the eye off the `TEXCOORD3.z` lane the vertex and
/// hull stages carried. Gated structurally like the hull hook. A terrain domain shader with no free
/// lane takes the pinned-eye reprojection instead ([`dxbc_stereo::reproject_pinned_eye`], under
/// [`crate::stereo::single_pass::should_pin_terrain`]), and its D3D object is tagged so the per-eye
/// re-issue knows to pin `cb13` for its draws. The domain shader reads `cb13`, which is bound to the
/// domain stage in [`crate::stereo::single_pass`]'s per-view upload.
#[detour(address = jc3gi::graphics_engine::draw::CreateDomainProgram_ADDRESS)]
fn create_domain_program(
    device: *mut c_void,
    params: *mut CreateDomainProgramParams,
) -> *mut HDomainProgram_t {
    let mut saved: Option<(*const u8, u64, Vec<u8>)> = None;
    let mut pinned = false;
    if terrain_active()
        && let Some(p) = unsafe { params.as_mut() }
        && !p.m_Code.is_null()
        && p.m_Size >= 4
    {
        let name = program_name(p.m_Name);
        // Skip shadow-pass terrain: reprojecting a shadow-atlas draw by the eye M_eye (and routing it to
        // a per-eye viewport that does not exist in the shadow pass) corrupts the shadow map.
        if !is_terrain_shadow_pass(name.as_deref()) {
            let code = unsafe { std::slice::from_raw_parts(p.m_Code, p.m_Size as usize) };
            // A terrain domain shader with no free eye lane is drawn once per eye instead, and takes
            // the pinned-eye reprojection that reads whichever eye that re-issue pins `cb13` to.
            // With the eye-row switch on, either path reads the eye's camera rows rather than
            // reprojecting the centre's clip position.
            let eye_rows = crate::stereo::single_pass::terrain_eye_rows();
            let lane = if eye_rows {
                dxbc_stereo::rebase_domain_shader(code)
            } else {
                dxbc_stereo::reproject_domain_shader(code)
            };
            let rewrite = match lane {
                Ok(blob) => Some((blob, "lane")),
                Err(dxbc_stereo::DxbcError::NoEyeLane)
                    if crate::stereo::single_pass::should_pin_terrain(name.as_deref()) =>
                {
                    let pinned = if eye_rows {
                        dxbc_stereo::rebase_pinned_domain_shader(code)
                    } else {
                        dxbc_stereo::reproject_pinned_eye(code)
                    };
                    pinned.ok().map(|blob| (blob, "pinned"))
                }
                Err(_) => None,
            };
            if let Some((blob, path)) = rewrite {
                let len = blob.len() as u64;
                tracing::info!(target: "single_pass", stage = "domain", name = ?name, path, eye_rows, "terrain: reprojected");
                pinned = path == "pinned";
                saved = Some((p.m_Code, p.m_Size, blob));
                p.m_Code = saved.as_ref().expect("just set").2.as_ptr();
                p.m_Size = len;
                crate::stereo::single_pass::record_domain_reprojected();
            }
        }
    }

    let result = CREATE_DOMAIN_PROGRAM.get().unwrap().call(device, params);

    if pinned
        && let Some(program) =
            unsafe { result.as_ref() }.and_then(|holder| holder.m_Program.as_ref())
    {
        crate::stereo::single_pass::tag_pinned_eye(program);
    }

    if crate::stereo::single_pass::shader_identity::requested()
        && let Some(program) =
            unsafe { result.as_ref() }.and_then(|holder| holder.m_Program.as_ref())
        && let Some(params) = unsafe { params.as_ref() }
        && !params.m_Code.is_null()
        && (4..=16 * 1024 * 1024).contains(&params.m_Size)
    {
        crate::screenshot::harness::patch_draw::tag(program, unsafe {
            std::slice::from_raw_parts(params.m_Code, params.m_Size as usize)
        });
    }
    if let Some((original_code, original_size, _copy)) = saved
        && let Some(p) = unsafe { params.as_mut() }
    {
        p.m_Code = original_code;
        p.m_Size = original_size;
    }
    result
}

/// Zero the two seed constants (the first eight bytes of each `l(12.9898, 78.233, 0, 0)` immediate),
/// collapsing the PCF disk rotation to a constant angle. Returns the number of sites patched.
fn zero_seeds(code: &mut [u8]) -> usize {
    let mut count = 0;
    let mut i = 0;
    while i + SEED.len() <= code.len() {
        if code[i..i + SEED.len()] == SEED {
            code[i..i + 8].fill(0);
            count += 1;
            i += SEED.len();
        } else {
            i += 1;
        }
    }
    count
}

/// The number of PCF-seed sites patched since injection. Surfaced in the debug UI so it is obvious
/// whether the hook is catching shadow shaders -- `0` means none were (re)created after it installed,
/// so a shader reload is needed.
pub fn patched_count() -> usize {
    PATCHED.load(Ordering::Relaxed)
}

/// Request a shader reload (from the debug UI). Performed at the next [`process_reload_request`] on the
/// game thread, so the patch can be applied to shaders that were created before the hook installed
/// (the usual case -- injection is after the game has loaded its shaders).
pub fn request_reload() {
    RELOAD_REQUESTED.store(true, Ordering::Relaxed);
}

/// Request a shader reload when single-pass has turned on or off since the live shader set was
/// created. The shader-creation hooks apply single-pass's rewrites, so the set has to be re-created
/// whenever [`crate::stereo::single_pass::active`] changes: when the configuration turns it on or
/// off, and when a VR session starts or stops under the default configuration, including the first
/// session after injection. Called once per frame on the game thread, after the single-pass
/// configuration snapshot is refreshed and before [`process_reload_request`], so the reload lands
/// in the frame whose snapshot asked for it.
pub fn reconcile_single_pass() {
    if crate::is_shutting_down() {
        return;
    }
    let active = crate::stereo::single_pass::active();
    if CREATED_SINGLE_PASS.load(Ordering::Relaxed) != active
        && !RELOAD_REQUESTED.load(Ordering::Relaxed)
    {
        tracing::info!(
            active,
            "shader reload: single-pass activity changed; re-creating the shaders under it"
        );
        request_reload();
    }
}

/// If a reload was requested, force the engine to re-create every shader: read the active bundle name,
/// then `LoadShaderBundle` the *other* quality variant and back. Each call re-creates all shader
/// holders through [`create_fragment_program`], so the PCF patch lands. Call once per frame on the game
/// thread (no draw in flight).
pub fn process_reload_request() -> Option<Result<(), ReloadError>> {
    if !RELOAD_REQUESTED.swap(false, Ordering::Relaxed) {
        return None;
    }
    Some(reload_now())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReloadError {
    NoGraphicsEngine,
    InvalidBundleLength,
    InvalidBundleEncoding,
    UnknownBundle,
    LoadVariantFailed,
    RestoreBundleFailed,
}

impl std::fmt::Display for ReloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let reason = match self {
            Self::NoGraphicsEngine => "the graphics engine is unavailable",
            Self::InvalidBundleLength => "the shader bundle name has an invalid length",
            Self::InvalidBundleEncoding => "the shader bundle name is not UTF-8",
            Self::UnknownBundle => "the shader bundle has no known quality variant",
            Self::LoadVariantFailed => "the engine did not load the alternate shader bundle",
            Self::RestoreBundleFailed => "the engine did not restore the original shader bundle",
        };
        write!(f, "shader reload: {reason}")
    }
}

impl std::error::Error for ReloadError {}

/// The state the live shader set was created under, if a completed reload established one. The
/// reload is synchronous -- `LoadShaderBundle` re-creates every shader holder before it returns --
/// so a value equal to [`ShaderBuild::current`] means the live bytecode carries exactly the
/// creation-time rewrites the configuration asks for.
pub(crate) fn built_with() -> Option<ShaderBuild> {
    *BUILT_WITH.lock()
}

pub(crate) fn reload_now() -> Result<(), ReloadError> {
    RELOAD_REQUESTED.store(false, Ordering::Relaxed);
    // Recorded before the attempt, so a reload that fails is not retried on every frame.
    CREATED_SINGLE_PASS.store(crate::stereo::single_pass::active(), Ordering::Relaxed);
    *BUILT_WITH.lock() = None;
    let result = bounce_shader_bundle();
    if let Err(error) = &result {
        tracing::warn!("{error}");
    }
    result
}

/// On eject: if the mod patched any shaders that the game still holds -- single-pass's substituted
/// (`cb13`-reading) vertex shaders, or the fragment-side PCF edits -- re-create the
/// originals. `LoadShaderBundle` re-runs the create hooks, and with `crate::is_shutting_down()` set
/// both the vertex substitution and the fragment patches are inert, so the game returns to fully
/// pristine shaders instead of keeping the mod's edits after it is gone. Game thread, during
/// shutdown, before the hooks are uninstalled.
pub fn restore_original_shaders_on_eject() {
    // Every substitution the mod makes has to be represented here, or a session that made only that
    // kind of substitution ejects without a bounce. The tessellation pair is the case that was
    // missing: a terrain-only session leaves a domain shader reading a `cb13` nothing binds any more.
    let (hull_forwarded, domain_reprojected) = crate::stereo::single_pass::terrain_counts();
    let patched_anything = crate::stereo::single_pass::has_patched_shaders()
        || patched_count() > 0
        || SSDECAL_BIASED.load(Ordering::Relaxed) > 0
        || OVERLAYS_LOCALIZED.load(Ordering::Relaxed) > 0
        || hull_forwarded > 0
        || domain_reprojected > 0;
    if !patched_anything {
        return;
    }
    crate::stereo::single_pass::warn_if_shaders_hold_patched_bytecode();
    tracing::info!("shader restore: re-creating the pristine shaders on eject");
    let _ = reload_now();
}

/// Re-create every shader by bouncing the active bundle to its other quality variant and back, so all
/// shader holders re-run through the create hooks. Game thread only, no draw in flight (drains first).
fn bounce_shader_bundle() -> Result<(), ReloadError> {
    // SAFETY: runs on the game thread; the engine singleton is live and its `m_CurrentBundleName` is a
    // stable `std::string`. `LoadShaderBundle` is what the settings path calls; we drain the draw
    // first so no GPU work references the shaders being replaced.
    unsafe {
        let ge = GraphicsEngine::get().ok_or(ReloadError::NoGraphicsEngine)?;
        let size = ge.m_CurrentBundleName.size;
        if size == 0 || size > 64 {
            return Err(ReloadError::InvalidBundleLength);
        }
        let current = ge.m_CurrentBundleName.as_bytes().to_vec();
        let current_name =
            std::str::from_utf8(&current).map_err(|_| ReloadError::InvalidBundleEncoding)?;
        let other = toggle_bundle(current_name).ok_or(ReloadError::UnknownBundle)?;

        let mut away = other.as_bytes().to_vec();
        away.push(0);
        let mut back = current.clone();
        back.push(0);

        ge.WaitForCPUDrawToFinish();
        let away_loaded = ge.LoadShaderBundle(away.as_ptr());
        // A reload bounces the bundle (away, then back), re-creating every shader through the hooks
        // twice. Reset the single-pass census after the throwaway `away` pass so the reported numbers
        // reflect exactly one clean pass over the real (`back`) shader set; also clear the patched-VS
        // set so it repopulates with the `back` set's live shader pointers.
        crate::stereo::single_pass::shader_identity::reset();
        crate::stereo::single_pass::reset_census();
        crate::stereo::single_pass::reset_patched_vs();
        let build = ShaderBuild::current();
        if !ge.LoadShaderBundle(back.as_ptr()) {
            return Err(ReloadError::RestoreBundleFailed);
        }
        if !away_loaded {
            return Err(ReloadError::LoadVariantFailed);
        }
        // The debug UI can change the configuration from another thread while the bundle loads;
        // a set created across that change matches neither state, so it is recorded as unknown.
        if ShaderBuild::current() == build {
            *BUILT_WITH.lock() = Some(build);
        }
        tracing::info!(
            "shader reload: '{current_name}' (bounced via '{other}'); {} PCF sites patched total",
            patched_count(),
        );
        // A no-op unless the const census switch is on; then the `back` pass has re-created every
        // shader through the census hook, so the name census is complete -- dump it.
        crate::stereo::single_pass::dump_vs_name_census();
    }
    Ok(())
}

/// The opposite shadow-quality variant of a shader bundle, used to force a reload by switching away and
/// back. Bundles come in `*LowShadows` / non-`*LowShadows` pairs (plus the Intel `ConstMath` variants);
/// toggling the suffix keeps the math variant correct.
fn toggle_bundle(name: &str) -> Option<&'static str> {
    Some(match name {
        "Shaders" => "ShadersLowShadows",
        "ShadersLowShadows" => "Shaders",
        "ShadersConstMath" => "ShadersConstMathLowShadows",
        "ShadersConstMathLowShadows" => "ShadersConstMath",
        _ => return None,
    })
}
