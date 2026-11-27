# Controls summary

The controller scheme hides the game's context button hints, because their gamepad glyphs do not describe the controller bindings. A panel on the back of the off-hand wrist shows instead the controls usable now, above a short fixed list of the mode's essentials. The model is `crates/jc3vr-gameplay/src/controllers/summary/`; the presentation is `crates/jc3vr-hud/src/controllers/summary/` (the grid, the glance, the page clock, and the game's hint glyphs) and `crates/jc3vr-hud/src/hud/summary_panel.rs`. The engine's hints and localisation are [button-hints-and-localisation.md](../../engine/button-hints-and-localisation.md). The panel and the hint suppression have no settings: both act whenever the controller scheme is in effect.

## Sources

The rows come from the binding tables ([bindings.md, "Tables and masks"](bindings.md#tables-and-masks)), described through the runtime's bound sources. The full reference (`describe_bound` over a `SourceTable`, `summary::published().summary`) keeps unbound rows and default fallbacks, and Input/bindings shows it. The wrist describes the same declarations from sources only: an unknown binding gets no guessed default button, a known source without a glyph keeps the runtime's localized name, and an unbound row never appears. Named sources compare by hand and resolved name rather than a table-local index, and `Published::wrist_revision` changes only when the wrist's content changes, so unrelated source churn does not restart its pages.

## Eligibility

A row shows only while it is usable. `summary::refresh` takes an `Eligibility` snapshot from the input owner after the weapon handling's finish and the neutral-transfer reconciliation, carrying the owner's final routing (which hand is ready, and which role each trigger has: per-hand gun fire, reload, docking, carrying, action, engine shooting permission), so the summary never rediscovers ownership. `eligibility::resolve` admits the ordinary actions of the current mode (steering, camera, movement, seat controls) and gates the contextual ones on their current permission: skipping a cutscene, handheld fire, reload, tether retraction and release, use, vehicle entry, the grenade throw, and explosive placement. Rows left out keep a typed reason in `Published::omitted`. A trigger's transfer suppression does not hide a usable face button, stick, or grip on the same hand.

The owning subsystems supply the rows for the mod's own mechanics, from their final post-step state, so a row describes a control that works now: the held gun's fire and its native reload and holster (independent permissions on the same button), the tether controls by the grapple button's phase, the yank, the selected vehicle controls, the flick eject, the belt reload while a reload can take a round, a round in hand (`InsertRound`), a held grenade's pin, the C4 detonation from a held charge or the detonator, the grip on the resolver's chosen target for the hand that would take it, a vehicle door's grip only for the selected, unheld, latched door, a parachute handle (to take while unheld, to steer and brake while held), and the flight gestures (deploy while falling, close and brake in the suit, from the gesture owner's revalidated availability). These rows describe the mod's mechanics and grant no game permission. The cutscene skip, the selected use and vehicle-entry prompts, and other interaction-target labels are not yet supplied, and are omitted rather than guessed.

## Essentials

Usable rows alone leave the wrist nearly empty in the moments a player looks for help: on foot with nothing attached it showed a few lines and no way to release tethers. So below each hand's usable rows the wrist lists the mode's essentials, the controls a player cannot find by trying, whether or not usable now (`summary::essentials`), a step dimmer; an essential the usable rows already show adds its control to that row instead.

| Mode | Essentials |
| --- | --- |
| On foot and swimming | fire the grapple, retract the tethers, release the tethers (double tap), jump, use, reload, detonate (the stick click held, only while a charge is placed) |
| Vehicles, every class and seat | the vehicle's weapons and the personal weapon on the face buttons, fire the grapple, release the tethers, stunt (tap) and exit (hold), which reads as the bail where the bail applies |
| Falling | fire the grapple, release the tethers, open the parachute, open the wingsuit, the ripcord |
| Wingsuit | fire the grapple, release the tethers, open the parachute, boost, the suit's two weapons, the ripcord |
| Parachute | fire the grapple, retract the tethers, release the tethers, open the wingsuit, the handles |

The rows come from the mode's own table with the runtime's sources (`describe_current` without routes), so they follow the player's rebinding and cannot list a control the tables do not inject. They are labelled by the same projection as the usable rows, so a mod gesture keeps its mod string.

## Context hints and labels

The input tick reads the HUD's hint queue through [`HUDUI::m_ButtonHints`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::hud_ui::HUDUI##field-m_ButtonHints) under a non-waiting lock, keeping the previous read when the lock is busy. The wrist merges a current hint into the matching usable row; a hint cannot revive a missing binding or an ineligible row. Labels use the game's localisation keys through [`Locator::GetLocalizedStringRaw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::locator::Locator##func-GetLocalizedStringRaw), with an English fallback, and mod mechanics use mod strings. The use-item prompt, notifications, and tutorials keep their native gamepad glyphs. The weapon pickup panel is hidden under the controller scheme ([physical-weapons.md](physical-weapons.md)).

## Layout and pages

The grid keeps the physical left hand on the left and the right on the right whatever the dominant hand. Shared actions and view or menu actions take a second side-by-side pair only when present. Each cell has a four-row page budget at a fixed readable row height (`ROW_PX` in `hud/summary_panel.rs`); overflow makes more pages rather than smaller text, and one indivisible wrapped row may enlarge its page. `grid::layout` measures the same row jobs the painter draws. Each footer shows the page and the page count. A mode with nothing to show (the menus) says `No usable controls here`.

There is no next or back input for the pages: while the panel is shown they cycle, with an explicit countdown, every `summary.page_dwell_s` of the HUD tuning (`page_clock::PageClock`). Hidden time does not advance it, and a new glance or content revision resets it to the first page. No controller input is taken from gameplay for paging.

## Visibility

The panel has its own egui context, render target, and quad pass on the back of the off-hand wrist; eye zero advances visibility and paging, and both eyes draw the same world corners. It shows on a glance (the HUD tuning's `summary.glance` cone, dwell, hysteresis, and linger), for `summary.show_on_mode_change_ms` after a mode change, and while the summary action is held (touching the off-hand trackpad by default). A trackpad press cancels only a hold whose sole source on that hand is the trackpad touch: OpenXR reports an aggregate value rather than the contributing binding, so another accepted held route keeps the panel up. The health and heat wrist display is a separate surface ([hud.md](../hud.md)).

## Unverified

Unchecked in the headset: page overflow and dwell in busy moments, long localized headings and footers, the minimum text size, and the wrist placement.
