# Debug UI

The debug UI is an egui window drawn on the desktop and, in the headset, on the floating panel ([hud.md](hud.md#the-interactive-egui-debug-panel)). F8 or the controller chord summons the panel, F6 toggles the desktop overlay's input capture, and F4 toggles the clean view. It holds every setting the pause menu does not ([settings.md](settings.md#where-a-setting-is-exposed)) and the diagnostic readouts. The code is `crates/jc3vr-ui/src/ui/`.

## Navigation

The nine pages are Status, Body/tracking, Hands/interaction, Weapons, Movement/vehicles, Rendering, Input/bindings, Tools, and Experimental, each holding features (`ui/catalog.rs`). Each feature renders its controls, then a collapsible live readout that runs after the controls return, so it holds no configuration edit guard. `ui::dispatch` routes every feature with an exhaustive match, so a new feature does not compile without a renderer. Tools holds the session checklist, the captures ([render trace](rendering/render-trace.md)), the director ([trailer-tooling.md](trailer-tooling.md)), the [environment controls](#environment-controls), and the presets; Experimental holds the render RE window (`ui::diagnostics`).

## Status rows

The window opens on Status (`ui::overview`), which lists the status rows (`crates/jc3vr-ui/src/status/`) needing attention first. Each row reads a state word (`Ok`, `Off`, `Waiting`, or `Failed`), then a reason or key numbers. Each section module owns its rows as `status::Row` constants, whose formatter is the one place the status becomes text; a feature page shows its row as a headline through `ui::status_row::headlines`, so the two never disagree. `Log the summary` logs every row once as a line starting `status:`. A row may be read on any thread, including mid-frame while the game thread holds the VR runtime lock ([threading.md](threading.md)), so it reads only what its module publishes for any thread (atomics or a leaf lock), and a subsystem whose state sits behind a gate publishes a copy for the rows.

## Session checklist

The checklist (`ui::checklist`) lists the numbered lines under "Checks in the headset" in the newest `docs/sessions/<stamp>/next-session.md`, embedded by the UI crate's build script. Each check is marked Untested, Works, or Doesn't work, saved at once to `session_checklist.toml` keyed by stamp and number, and logged, so a recording's log shows when each was tried. A file that fails to parse blocks saves until it is repaired and reloaded.

## Input ownership

Opening the panel requests editing: the panel takes the controls, the game's input device is disabled, and gameplay stands down. `Allow gameplay input` makes it a read-only overlay instead; it is not saved, and reopening returns to editing. A transition clears egui's queued and remembered input, waits for the destination device's acknowledgement and then a neutral input sample, and consumes held inputs through their release, because filtering egui's events alone does not make handing the controls back safe. The policy is `jc3vr_input::capture` and the adapter `jc3vr_hud::egui_impl`. A panel left with neither editing nor gameplay owning the controls is logged with what the neutral test read. Window focus gates only the keyboard and text; the controller pointer works while the game window is in the background.

## Visualizations

Show visualizations opens a drawer of the overlay's view switches (the body views, the grab and collider views, the docked guns), each with how many primitives it drew at the last snapshot. Hide registered views masks their output until Restore, without writing the configuration.

## Environment controls

Tools > Environment (`crates/jc3vr-ui/src/ui/environment.rs`) sets the time of day and the weather for testing rendering under different lighting, and re-reads each value every frame so drift shows. The hour slider calls `WorldTime::SetTimeOfDay`. The weather buttons send the weather controller's named events, which is the way to pin a weather state; the severity slider calls `Weather::SetSeverity`, and the controller pulls the rain and snow sliders back unless an event holds the state. [engine/environment.md](../engine/environment.md) describes the game's side.

## Layout persistence

The workspace and detached feature views are saved to `ui_layout_v2.json` by stable catalog keys. One owned writer replaces the file atomically outside the layout and egui locks, keeping only the newest pending snapshot and retrying a failed write. An invalid layout shows a notice and stops automatic saving until it is recovered or reset.
