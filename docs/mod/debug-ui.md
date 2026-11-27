# Debug UI

The debug UI is an egui window drawn on the desktop and, in the headset, on the floating panel. F8 or the controller panel shortcut summons the panel, F6 toggles the desktop overlay's input capture, and F4 toggles the clean view, which at its desktop scope hides the desktop overlay ([hud.md](hud.md)). It holds every setting the pause menu does not ([settings.md](settings.md#where-a-setting-is-exposed)) and the diagnostic readouts.

## Navigation

The window opens on Status. The nine pages are Status, Body/tracking, Hands/interaction, Weapons, Movement/vehicles, Rendering, Input/bindings, Tools, and Experimental, each holding features (`crates/jc3vr-ui/src/ui/catalog.rs`). A wide window keeps the page list and the feature list beside the selected feature; a narrow one uses selectors.

The Status page's one feature (`ui::overview`) lists the status rows ([status.md](status.md)) that need attention first, failed and waiting, then the rest under a collapsible header. Its buttons open the performance, tracking capture, and video features, and log the status summary.

Each feature renders its controls followed by a collapsible Live status readout. Readouts run after the controls return, so they hold no configuration edit guard. `ui::dispatch` routes every catalog feature to its renderer with an exhaustive match, so a new feature does not compile until it has a route. The controller, game, and render modules hand out each feature's controls and readout as plain functions, which lets a renderer-free test check that every feature renders something.

Tools holds the session checklist, tracking capture, render capture ([render trace](rendering/render-trace.md)), the director ([trailer-tooling.md](trailer-tooling.md)), the environment ([environment.md](environment.md)), video, and the presets (`Controllers` and `Gamepad baseline`). Experimental holds the single-pass stereo and post-FX skip controls and the render RE window: a separate window (`ui::diagnostics`) with the render-block type registry toggles, the exposure A/B, and the terrain and shadow-cascade diagnostics, whose open state is not persisted.

## Session checklist

The session checklist (`ui::checklist`) lists the headset checks of the newest session's notes. The UI crate's build script embeds `docs/sessions/<stamp>/next-session.md` from the newest session directory, rebuilding when the notes change, and the checks are the numbered lines under its "Checks in the headset" heading. Each check is Untested, Works, or Doesn't work, with an optional note on a failure. A mark is saved at once to `session_checklist.toml` in the configuration directory, keyed by the session's stamp and the check's number, and logged (`checklist: a check was marked`), so a recording's log says when each check was tried.

A file that fails to read or parse blocks changes and saves, preserving the file, until an explicit reload after it is repaired. A failed save keeps the edits in memory and offers a retry rather than a reload.

## Input ownership

Opening the panel requests editing: the panel takes the controls, the game's input device is disabled, and gameplay stands down. The header's `Allow gameplay input` requests a read-only overlay instead: readouts stay live, but no pointer, trigger, grip drag, key, text, or scroll edits a control, and the body, the navigation, detached views, and layout actions are disabled. It is not saved, does not enable the motion controllers or the bindings, and does not override native menus or input recovery. The game is not paused or unpaused. Hiding and reopening the panel returns to editing.

The header reports the requested mode separately from the effective ownership, so a tracking, neutral-input, or device-acknowledgement wait shows as a blocker rather than as gameplay. The capture policy and its published state are in `jc3vr_input::capture`; the adapter is `jc3vr_hud::egui_impl`. A transition clears queued events and egui's remembered pointer and key state, ends a panel drag, and waits for the destination device's acknowledgement and then a neutral input sample. The input tick consumes the held inputs through their release during that wait: filtering egui's events alone does not make it safe to hand the controls back. Editing saves the game device's prior state and releasing restores it, including a device that was disabled, and never forces the device on while the window lacks focus.

A panel that stays up with neither editing nor gameplay owning the controls is logged (`debug panel: neither editing nor gameplay owns the controls`) after a short wait and periodically after, with what the neutral test read: each hand's control not at rest, why the bindings' input group is not neutral, the desktop keys, focus, and tracking. The same group proof re-arms the controller chords, so a wait that never ends also leaves the close chord dead, and the log says which input holds it.

Desktop window focus gates only what the window carries, the keyboard and text. The panel's pointer, grip drag, and scroll come from the controllers through the OpenXR session, so the in-world panel stays editable while the game window is in the background. The desktop sample is taken on the game thread, the window's thread, and the window's handle comes from the game's graphics params, so the sample reads the real foreground; when the game names no window the sample is unavailable rather than guessed, and the first unavailable sample logs which check failed.

## Visualizations

The Show visualizations button opens a drawer on the right without changing the selected feature. It edits the overlay's view switches in place: the body mirror, body rig, and body estimate (owned by Body fit), the grab view and collider view (Physical hands), and the docked guns (Inventory). The filter matches the view names and the owning features. Under each switch that is on, it shows how many primitives the overlay collected from that source at its last snapshot, after the hide mask and the cap, so it does not restate the gates that decide whether a producer draws. It takes an edit guard only to apply changed switches after rendering.

Hide registered views suppresses the drawn output of every one of those sources (with the body mirror's pouches), including switches turned on while hidden; Restore clears the mask. The collector filters before the primitive cap, from the next first-eye snapshot so both eyes keep the same geometry. Producers keep submitting and expiring their slots, other sources (the grapple highlight, the sight tuning) are left alone, and neither action writes the configuration: a hide lasts until Restore or teardown.

## Layout persistence

The workspace and detached feature views are saved to `ui_layout_v2.json` in the configuration directory, by stable catalog keys. Detach this view opens the selected feature in a dock window; dragging its tab back re-docks it. Recover detached views gathers them into the main dock, and Restore default layout returns to one workspace on Status.

While editing, the layout submits a serialized snapshot every sixty frames to one owned writer, which keeps one replacement in flight and only the newest pending snapshot, and replaces the file atomically outside the layout and egui locks. The saved snapshot advances only once a write is acknowledged, so a failed one retries at the next submission. Shutdown drains the writer, and one that does not finish keeps the payload mapped. An unknown feature key is dropped, and an invalid or unsupported layout shows a notice and stops automatic saving until it is recovered or reset.
