# Floating HUD in VR

The flat game draws its HUD straight onto the final image ([engine/rendering/ui-rendering.md](../engine/rendering/ui-rendering.md)), which in VR would pin it to the screen plane at infinity. The mod renders the HUD into its own texture and draws it as a damped quad in front of the head; world-anchored markers are reprojected, and parts that describe one world object get their own depths. The code is `crates/jc3vr-hud/src/hud/`, with the UI detours in `crates/jc3vr-hud/src/hooks/ui/`; the settings are `hud` (`hud::config`) and the tuning is `crates/jc3vr-hud/tuning/hud.toml`.

## Presentation

The panel is drawn into each eye's scene at [`RenderEngine::PostDraw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-PostDraw), not as an OpenXR quad layer, so it shows in the desktop preview and shares the scene's depth, at the cost of slightly softer text.

## Redirecting the HUD into the mod's texture

The UI manager renders Scaleform through [`UIManager::m_RenderBuffer`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##field-m_RenderBuffer), which [`UIManager::InitPlatformRT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##func-InitPlatformRT) builds over the engine's surface at startup and on every device or resolution reset. [`RenderTargetData::UpdateData`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::RenderTargetData##func-UpdateData) swaps the buffer's views and refcounts them itself, so it is safe to call from outside.

The redirect (`hud/binding.rs`) creates the HUD texture, sizes the movie's render rectangle, cached viewport, and movie viewport to it, re-runs `InitPlatformRT`, and calls `UpdateData` to point the buffer at the texture. It installs lazily, so a late injection works, and re-applies after a reset. Its maintenance runs before the main [`UIManager::StartRender`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##vfunc-StartRender) queues its worker, under the UI-deferred lock and then the graphics context lock. Turning `hud.redirect` off, or unloading, re-runs `InitPlatformRT` so the UI never renders into a freed texture.

Off the engine's working surface, the HUD drops out of the scene composite with no further patch. Before `PostDraw` samples the texture it calls [`UIManager::SyncRender`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##vfunc-SyncRender) and [`UIManager::Submit`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##vfunc-Submit) outside both locks, and only while the redirect is applied. The tracked world therefore stays visible behind full-screen UI.

## Comfort: lazy follow

The panel's position is the live head position; its orientation eases toward the head's with a critically damped slerp (halflife `panel.follow_rotation_halflife_s`), roll included. A rigid lock amplifies tremor and a world lock slides out of view; the damped follow has no deadzone, so the panel never rests off the gaze.

`HudMode` (from `current_mode`) picks the aspect: the gameplay HUD at `panel.hud_aspect`, or full-screen UI (movies, loading screens, menus) at `MOVIE_ASPECT`. An open in-game pause menu or map latches the panel in place (`menu_world_lock`) so the head can look around it. In a vehicle the gameplay panel eases to `panel.depth_shift.near_distance_m` (`hud/depth.rs`).

## Stereo depth

The panel's world-space corners are computed once per frame on eye 0 and both eyes project them through their own view-projection, giving correct disparity at a finite depth; computing corners per eye would cancel the world transform and collapse the panel to infinity. The panel keeps a constant apparent size, so `hud.distance` changes depth without resizing it; `hud.panel_scale` sets the size (`hud::panel_height`).

## World-to-screen: split the HUD

The static HUD lags with the panel, which is correct, but world-anchored markers mean a direction, so they are reprojected. [`UIManager::Get2DInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##func-Get2DInfo) takes the view-projection as a parameter and forwards it to [`Convert3DCoords`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##func-Convert3DCoords) ([ui-rendering.md](../engine/rendering/ui-rendering.md#points-of-interest)); its detour substitutes the panel's matrix and camera transform (`hud::compute_panel_vp`), and sets [`UIManager::m_CachedViewportRatio`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##field-m_CachedViewportRatio) to the panel's aspect for the call only. The grapple reticle projects through [`UIManager::Convert3DCoordsDefault`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##func-Convert3DCoordsDefault), called only from [`HUDUI::UpdateGrappleReticle`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::hud_ui::HUDUI##func-UpdateGrappleReticle); a separate detour reprojects it and records the aim depth.

On-screen point-of-interest markers leave the panel as billboards at their world points, keeping their angular size. On-screen is judged against the headset's field of view, not the panel's, so markers outside it stay edge arrows on the panel. Each sits on the nearest visible surface along its line of sight, so it never reads as farther than a wall it shows through.

## HUD depth

The atlas (`hud.atlas.enabled`, the pause menu's `HUD ON THE HANDS`) is the only depth path: Scaleform has already composited overlapping clips into the one texture, so the atlas moves the parts that describe one world object into cells of their own before the capture and draws each on its own quad. The mechanics are in [ui-decoupling.md](rendering/ui-decoupling.md#the-atlas-as-built).

The rule: an element tied to one world object gets depth (at the object, on the hands or body, or at the aim depth); a general element stays on the panel; a mixed group is split per child.

- World markers: a cell each, at the surface they label.
- Ammunition: the mod's own counter over each gun in a hand (`hud::anchored::hand_ammo`), red while the magazine is empty or out; the game's readout is not drawn, so with the atlas or redirect off there is none.
- Under the controller scheme: the tether readout above the off-hand's forearm, the [weapon wheel](weapon-selector.md)'s cross at the steering hand, and a health bar on the off-hand's inner wrist over the game's heat and grenade readouts (the game's health vignette is suppressed).
- The reticles and hit marker: one cell at the aim depth. Under the controller scheme the hand-aimed reticles are hidden in favour of the world-space gun dot or laser, so the cell shows for the gamepad and for head-aimed driver and mounted weapons.
- The grab hint ([grip.md](input/grip.md#the-grab-hint)) on the panel's plane below its centre, since its object is on the body, out of view; the markers on let-go weapons at their world points ([two-handed-inventory.md](input/two-handed-inventory.md#markers-on-the-weapons-let-go-of)).
- Everything else stays on the panel: centre notices, directional damage indicators (they carry only a direction), objectives, timers, notifications, the chaos feed, edge arrows, subtitles, and the skip prompt.

The full-screen overlay clips (drowning tint, damage flash, low-health paint, sniper scope) are held invisible ahead of every capture (`hud::scaleform`).

The aim depth (`hud/aim.rs`) comes from the first default-matrix projection of each game frame only, so later projections that frame (the wire's attach point, the grip-radius sample) are ignored. It is smoothed (`aim.halflife_s`) and eases to `aim.rest_distance_m` after `aim.stale_after_s` without a target. Earlier approaches are in [the post-mortem](../issues/08-14-hud-overlays-and-depth.md#constraints-established).

## The interactive egui debug panel

While an OpenXR session runs, the [debug UI](debug-ui.md) renders into a floating panel of its own (`hud/egui_panel/`), with its own target, placement, and corners. egui is tessellated once per frame into the panel texture, which both eyes and the desktop mirror draw. `hud.egui_panel.enabled` is off at every launch and not saved; F8 or the controller chord (left stick click held, right B pressed) toggles it ([debug-ui.md](debug-ui.md#input-ownership)).

### Where the debug panel lives

The panel's pose is held in the cockpit frame, the tracked space the head and controllers are reported in (`hud/egui_panel/slot.rs`), so it rides along with Rico like a window in a runtime's dashboard. It reaches the world through `cockpit::CockpitTransform` with the render-time head (`pointer::rendered_camera`), so it does not swim at speed ([render-time-anchoring.md](rendering/render-time-anchoring.md)) and agrees with the controller rays.

It is first placed in front of the head at `egui_panel.summon_distance_m`, facing it without roll, and comes back where it was left unless that is now behind the head, in which case it is placed in front again; `Recall panel` and a recenter do the same.

A hand grabs it by gripping with its aim ray on the panel (`crates/jc3vr-gameplay/src/controllers/bindings/pointer/egui_grab.rs`); the grip counts only after a released sample, one hand holds at a time, and the held panel follows the hand rigidly. That hand's stick pushes and pulls it along the ray (`egui_panel.push_rate_per_s`, within `egui_panel.min_distance_m` and `egui_panel.max_push_depth_m`).

### Pointing with the controllers

Either controller points at the panel (`pointer/egui_controller.rs`): the input tick intersects each aim ray with the corners the panel published. The pointing hand's trigger is the primary button, its upper face button the secondary, and its stick the wheel. The pointer passes between hands on a trigger pull, never mid-press. A press clicks only after a released sample and only if it begins on the panel.

Every controller pointer (the panel's, the game UI's, and the map's) has an aim hold (`pointer/aim_hold.rs`): a light pull past `pointer.aim_hold.pin_press` pins the cursor, and only a full pull past `pointer.aim_hold.click_press` presses at the pinned point. A held press stays a click until it leaves `pointer.click_slop`, so tremor never turns it into a drag.

The desktop mouse and the controller are arbitrated by activity (a resting controller's jitter under `egui_panel.controller_move_uv` does not count), and ownership changes only while no button is held. While the panel is enabled the bindings idle and the game-UI pointer is cancelled, so the trigger neither fires a weapon nor clicks a menu behind it.

## The clean view

The clean view (`director.clean_view.on`, `crates/jc3vr-core/src/clean_view.rs`) hides the mod's overlays without changing their settings. At the `Desktop` scope it hides the flat egui overlay from the desktop window; at `Everything` it also hides the floating HUD, the debug panel, and the in-world overlays. F4 toggles it. Neither value is saved: it starts on at `Desktop`, so the desktop overlay starts hidden and a launch never hides the HUD.

## Controller pointing in the game UI

While a menu action set is active, the game's Scaleform UI takes the dominant controller's aim ray as its mouse (`crates/jc3vr-gameplay/src/controllers/bindings/pointer/`, `hud/pointer/`, `hooks/ui/mouse.rs`), following `hud.redirect`. The trigger arms only after a released sample, so a trigger held across a mode change or focus loss never clicks. While the pointer shows, its trigger is the pointer's alone; the other hand's trigger still turns pages.

One router picks the source for each delivery through [`UIManager::SendMouseEvents`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##func-SendMouseEvents). Every cancellation releases a held button off the movie, which Scaleform treats as a release outside, so a cancelled press does nothing. [`UIManager::PreUpdate`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##func-PreUpdate) is the frame's input boundary: its detour prepares one controller sample for the map and the delivery, committed only when each consumer runs. [`UIManager::m_IsUsingGamepad`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##field-m_IsUsingGamepad) stays set under the controller scheme.

### The native map

The map ([`CommMapUI`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::comm_map_ui::CommMapUI)) reads the mouse only in its mouse branch, chosen by `m_IsUsingGamepad` ([engine/gameplay/input.md](../engine/gameplay/input.md#the-maps-cursor-and-selection)). While the controller drives it, the mod (`crates/jc3vr-hud/src/controllers/map/`) clears that flag around [`CommMapUI::OnManageInput`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::comm_map_ui::CommMapUI##func-OnManageInput) and [`CommMapUI::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::comm_map_ui::CommMapUI##func-Update), restoring one saved value, and answers [`InputActionMap::GetActionEffector`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::InputActionMap##func-GetActionEffector) with mouse and zoom effectors it owns, writing no physical slot. The cursor comes from the controller through the [`UIManager::GetMovieSpaceMouseCursor`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##func-GetMovieSpaceMouseCursor) detour.

The trigger is the primary button. The map turns a press into a drag after a few stage pixels, which tremor exceeds, so its cursor is held at the press point within `map.click_slop_uv`. A (`GUI_OK`) is reported as a right click, because the mouse branch would otherwise open the context menu at the last mouse selection rather than the cursor. `map_zoom` steps the zoom; the controller pans by dragging.

Unchecked in the headset: menu behaviour in gamepad mode, movie replacement, the map route end to end, and the map slop against real trigger pulls.
