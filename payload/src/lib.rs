use std::{ffi::c_void, sync::OnceLock};

use jc3vr_core::{
    cpu_placement, director::clean_view, game_tick::GameTick, is_shutting_down, lifecycle, logging,
    module, session, thread_role,
};
use jc3vr_hud::{egui_impl::EguiState, hooks::ui::cancel_pointer_on_game_thread, hud};
use jc3vr_physical::controllers::physical_hands;
use jc3vr_render::hooks::graphics_engine::shader::restore_original_shaders_on_eject;
use jc3vr_tooling::{director, testing};
use jc3vr_ui::ui;
use jc3vr_view::{hooks::graphics_engine::capture, recorder::tracking, vr};
use windows::Win32::{
    Foundation::HMODULE,
    System::{LibraryLoader::DisableThreadLibraryCalls, SystemServices::DLL_PROCESS_ATTACH},
};

mod allocator;
mod composition;
mod config;
mod crash;
mod eject_gpu;
mod hooks;
mod teardown;
mod tuning;

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub extern "system" fn DllMain(module: HMODULE, reason: u32, _unk: *mut c_void) -> bool {
    if reason == DLL_PROCESS_ATTACH {
        unsafe {
            DisableThreadLibraryCalls(module).ok();
            module::set(module);
        };
    }
    true
}

/// Starts the payload, reading the tuning packaged beside the DLL.
///
/// When the startup cannot continue (a tuning file is missing or invalid), the payload logs why and
/// unloads itself from this thread, so the injector finds it gone once the call returns.
#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub extern "system" fn run(_: *mut c_void) {
    start(tuning::Source::Packaged);
}

/// Starts the payload as [`run`] does, reading the tuning from the source tree it was built from,
/// so that an edit to the repository's files applies live. The development launchers start the
/// payload with this export.
#[unsafe(no_mangle)]
pub extern "system" fn run_source_tuning(_: *mut c_void) {
    start(tuning::Source::SourceTree);
}

/// Asks the game thread to eject the payload, for an injector that cannot signal it any other way.
///
/// The request only records the intent: teardown happens on the game thread, and the module stays
/// mapped until that thread reaches the end of its shutdown.
#[unsafe(no_mangle)]
#[inline(never)]
pub extern "system" fn eject(_: *mut c_void) {
    lifecycle::request_shutdown();
}

fn start(source: tuning::Source) {
    lifecycle::set_build_stamp(BUILD_STAMP);
    if !initialize_startup(source) {
        module::exit();
    }
}

/// Called when the DLL is loaded. Returns `false` when the startup stopped before installing
/// anything, in which case the caller unloads the payload.
fn initialize_startup(source: tuning::Source) -> bool {
    // Resolve this run's output directory first, so the log, crash dumps, and every other artifact
    // land under the same timestamped session folder.
    session::init();

    std::panic::set_hook(Box::new(|info| {
        // Log the location before touching the payload: for formatted panics, `payload()` runs the
        // format arguments' Display impls, and one reading dead game memory faults, which
        // kills the process before anything is logged. The location is plain static
        // data and always safe.
        let location = info.location().map(|l| l.to_string());
        tracing::error!(
            panic.location = location.as_deref(),
            "A panic occurred; formatting the payload (a crash before the next record means the \
             panic message itself faulted)",
        );

        let payload = info.payload();

        let payload = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str));

        let backtrace = std::backtrace::Backtrace::capture();

        tracing::error!(
            panic.payload = payload,
            panic.location = location,
            panic.backtrace = tracing::field::display(backtrace),
            "A panic occurred",
        );
    }));

    logging::install();
    tracing::info!(build = lifecycle::build_stamp(), "jc3vr startup");
    session::log_location();
    // First, while nothing has started that the eject would have to stop.
    if let Err(error) = teardown::register() {
        tracing::error!("startup: {error}; the payload unloads itself");
        return false;
    }
    // Before anything that reads the tuning, and before anything that would need undoing: a
    // payload without its tuning stops here and unloads.
    if let Err(error) = tuning::init(source) {
        tracing::error!("{error}");
        tracing::error!("startup: stopped; the payload unloads itself");
        return false;
    }
    crash::install();
    // Before the hooks: VR initialization, on the game thread's first frame, reads the configuration
    // (the dominant hand and the input layout among it), so it must already hold the saved settings.
    config::init();
    tracing::info!("startup: config init complete");
    // A registration refused or missing stops the startup before any hook is installed.
    if let Err(error) = composition::install() {
        tracing::error!("startup: {error}");
        tracing::error!("startup: stopped; the payload unloads itself");
        undo_startup_before_hooks();
        return false;
    }
    tracing::info!("startup: hook installation returned");
    true
}

/// Undo what `initialize_startup` started before the composition refused to install the hooks: the
/// tuning watcher's and the settings writers' threads, and the crash handler's process-wide
/// registration, each of which would outlive the unmapped image. A thread that does not stop pins
/// the image instead.
fn undo_startup_before_hooks() {
    let tuning_stopped = tuning::shutdown();
    let config_stopped = config::shutdown();
    if !(tuning_stopped && config_stopped) {
        module::pin();
    }
    crash::uninstall();
}

/// Called to undo `initialize_startup` and eject, on its own thread once the game thread has run
/// the cleanups.
fn shutdown_startup() {
    // Every worker thread must have exited before `module::exit` unmaps the image (see
    // `lifecycle::stop_workers`). One that will not stop pins the module instead: leaking this DLL
    // for the rest of the game's run is strictly better than a live thread in an unmapped image.
    if !lifecycle::stop_workers().is_empty() {
        module::pin();
    }

    // The cleanups requested work the still-installed hooks carry out on the engine's next frames:
    // the native-resolution restore in the `Draw` prologue, and the per-frame restore of the flags
    // they cleared (the HUD redirect among them).
    lifecycle::settle_frames("the cleanups' requests");

    tracing::info!("Uninstalling hooks");
    if !jc3vr_core::hooks::uninstall() {
        // Cleanup has already committed. Retention here is fatal containment, not live deferral.
        module::pin();
        tracing::error!("shutdown: hook removal is incomplete; ejection is blocked");
        return;
    }

    // A detour entered before the removal may still be running on a frame thread; the frames
    // passing see it return.
    lifecycle::settle_frames("the removed detours");
    eject_gpu::log_video_memory("hooks removed");

    // The VEH registration must not outlive the DLL: ntdll would keep calling into the unmapped
    // image on the game's next routine first-chance exception.
    crash::uninstall();

    tracing::info!("Ejecting");
    module::exit();
}

/// Called when we're on the game thread for the first time
fn initialize_from_game() -> anyhow::Result<()> {
    static INITIALIZED: OnceLock<bool> = OnceLock::new();
    if INITIALIZED.get().is_some() {
        return Ok(());
    }
    INITIALIZED.set(true).unwrap();

    eject_gpu::install();
    EguiState::install()?;
    capture::install();
    hud::install();
    vr::install();
    tracing::info!("Initialized in game thread");

    Ok(())
}

/// Called to undo `initialize_from_game`; called once shutdown is triggered
fn shutdown_from_game() {
    eject_gpu::log_video_memory("eject begins");
    // Let an in-flight frame tail submit before anything below takes the egui state lock: the VR
    // teardown in the cleanups locks the runtime state under it, and the tail holds that lock while
    // it waits on a draw task that takes the egui state lock. See `docs/mod/threading.md`.
    vr::wait_for_frame_tail();
    cancel_pointer_on_game_thread();
    // Re-create the original vertex shaders before anything is torn down: single-pass substitutes
    // patched (cb13-reading) shaders that the game holds, so without this the clean game would keep
    // rendering with them after eject. `SHUTTING_DOWN` is already set, so the reload's create hooks
    // are inert and produce the unpatched originals. A no-op unless single-pass patched any shaders.
    restore_original_shaders_on_eject();

    // Unconditionally, not only when the debug UI came up: the cleanups undo engine-wide state --
    // the double-wide render resolution, the HUD redirect -- that a session sets whether or not egui
    // was ever shown.
    lifecycle::run_cleanups(
        EguiState::get()
            .as_mut()
            .map(|state| &mut state.egui_renderer as &mut dyn std::any::Any),
    );
    EguiState::uninstall();
}

/// When this payload was compiled, embedded by `build.rs`. Announced at startup (log, in-headset
/// banner, and the debug window): an uninject can leave the module resident (a hung shutdown
/// thread keeps the DLL mapped), and a failed re-inject then silently reactivates the stale code —
/// the stamp is how a stale payload is caught at a glance. `run` records it in
/// [`lifecycle::set_build_stamp`], whose cell is the stamp's home for every module beneath this one.
pub const BUILD_STAMP: &str = env!("jc3vr_build_stamp");

/// Run only after lifecycle preparation commits on the game pre-update entry.
fn commit_shutdown() {
    tracing::info!("Shutdown preparation committed; tearing down");
    shutdown_from_game();
    std::thread::spawn(shutdown_startup);
}

fn contain_fatal_failure() {
    let first_failure = lifecycle::mark_fatal();
    module::pin();
    if first_failure {
        tracing::error!(
            "payload failure: safe gameplay continuation is unverified; cleanup and ejection are \
             blocked, and the game must be restarted"
        );
    } else if is_shutting_down() {
        tracing::error!("shutdown failure: teardown is incomplete; the game must be restarted");
    }
}

/// Whether any surface will draw this frame's egui output: the VR floating panel, the desktop
/// mirror's overlay, or, outside a VR frame, the flat overlay `graphics_flip` renders onto the
/// back buffer under the same conditions it checks.
fn debug_ui_visible(panel_active: bool) -> bool {
    panel_active
        || hud::mirror_overlay::is_active()
        || (vr::render_params(0).is_none() && !clean_view::hides_desktop())
}

fn update() {
    if is_shutting_down() || lifecycle::is_fatal() {
        return;
    }
    let panic = std::panic::catch_unwind(|| {
        if let Err(error) = initialize_from_game() {
            tracing::error!(
                ?error,
                "game-thread initialization failed; ejection is not proven safe"
            );
            contain_fatal_failure();
            return;
        }
        // Close the previous real frame's puffin scopes and open the next; the frame scope itself
        // is opened by `game_update` right after this returns. Once per real frame, main thread.
        #[cfg(feature = "profiler")]
        jc3vr_core::profiler::new_frame();

        if lifecycle::advance_shutdown() {
            commit_shutdown();
            return;
        }

        // First, so the whole frame reads one version of each tuning file.
        tuning::tick();

        // The directing tools' reconciliation pass (time scale, god mode, heat, queued teleports and
        // spawns) runs before the debug UI so a hotkey and a panel click in the same frame land in
        // one pass, and before the game's own update consumes the state they set.
        director::update();

        // The test tools, after the director's pass, which services a visual stage's teleport.
        tracking::frame_tick();
        testing::update();
        let large_cache_cores = vr::VR_CONFIG.lock().large_cache_cores;
        cpu_placement::update(large_cache_cores);

        // Ahead of the game's update, whose physics step fires the hand bodies' callback, so that
        // registering it never races the engine's walk over the callbacks.
        // SAFETY: the top of the game's update is a serial point: the update has not started, so
        // no engine thread mutates the world's game objects.
        unsafe { GameTick::enter(physical_hands::physics::frame_tick) };

        // The per-eye captures feed the VR blit, the desktop mirror, the recorder, and the screenshots.
        // Driven here rather than from inside the egui closure below: they are what VR submits, and
        // preparing them only as a side effect of the debug UI meant an `EguiState` that failed to
        // install left VR with nothing to present and no indication why.
        capture::EGUI_DEBUG_RENDER_STATE.lock().ensure_eye_targets();

        {
            let mut egui_state_guard = EguiState::get();
            if let Some(egui_state) = egui_state_guard.as_mut() {
                // The debug UI runs holding the egui state lock, which the draw task also takes, so
                // nothing in it may block on the VR runtime lock (see `jc3vr_core::thread_role`).
                let _debug_ui_role = thread_role::enter(thread_role::ThreadRole::GameDebugUi);

                // Drive the egui UI as the VR floating panel when a session is running and the panel is
                // enabled (issue #24): size the layout to the panel texture and re-source the pointer from
                // the desktop mouse or a controller onto the panel surface. Off, this is `None` and the
                // flat overlay path is unchanged.
                let panel_size = hud::egui_panel::active_size();
                egui_state.set_panel_mode(panel_size);
                match panel_size {
                    Some(size) => {
                        egui_state.push_events(hud::pointer::egui_panel_events(size));
                    }
                    None => hud::pointer::reset_egui_pointer(),
                }

                let visible = debug_ui_visible(panel_size.is_some());
                egui_state.run(|ctx, renderer| {
                    hud::pointer::apply_panel_input_options(ctx, panel_size.is_some());
                    // The UI-side half of capture upkeep: the Previews tab's own textures, and the egui
                    // registrations for surfaces the render thread created without a renderer. The eye
                    // captures themselves are prepared above, off this path. The HUD preview's retired
                    // registrations are released first, outside the capture lock.
                    hud::HUD_STATE.lock().drain_preview_retirements(renderer);
                    capture::EGUI_DEBUG_RENDER_STATE
                        .lock()
                        .prepare_if_necessary(renderer);
                    // Laying out and tessellating the debug window is the bulk of this pass, and
                    // nothing would draw its output while every surface that shows it is off.
                    if !visible {
                        return;
                    }
                    ui::startup_banner(ctx);
                    ui::restart_banner(ctx);
                    // The dock area fills whatever space the window gives it, so the window carries an
                    // explicit default size instead of auto-sizing to content.
                    egui::Window::new("Debug")
                        .default_pos(egui::pos2(0.0, 0.0))
                        .default_size(egui::vec2(760.0, 560.0))
                        .resizable(true)
                        .show(ctx, |ui| ui::egui_debug_window(ui, renderer));
                });
            }
        }

        // After the debug UI, whose controls are the most frequent edits, and outside its lock.
        config::tick();
    });
    if let Err(e) = panic {
        let panic_msg = e
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()));
        tracing::error!(
            ?panic_msg,
            "panic in update; safe continuation and ejection are unverified"
        );
        contain_fatal_failure();
    }
}
