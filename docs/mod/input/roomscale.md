# Posture and roomscale

The player's body in the room: the body dimensions, the STAGE floor, the posture, the head height, crouching, teleports and recenters, the fades, and the seat transitions. The body following the head about the room is the body rig's ([rig.md](../body/rig.md)); this page is what surrounds it. The code is `crates/jc3vr-body/src/controllers/roomscale/`, `crates/jc3vr-body/src/controllers/body/`, and `crates/jc3vr-view/src/vr/stage.rs`.

None of this depends on the controller scheme, because a gamepad player standing in a room moves the head the same way. The head is the ground truth and the camera is never derived from the body ([head-and-body.md](../body/head-and-body.md)).

## Body dimensions

The player's dimensions are the settings file's `[body]` section ([settings.md](../settings.md)), written by hand or by the calibration on the pause menu's `VR: BODY` page ([pause-menu.md](../pause-menu.md#body-calibration)). Missing or impossible values take the generic adult's (`BodyDimensions::GENERIC_ADULT`). Rico's proportions are estimated from his height (`BodyDimensions::CHARACTER_ESTIMATE`), except his arms, which the hands measure ([hands-and-aiming.md, "Arm reach"](hands-and-aiming.md#arm-reach)).

The calibration measures the eye height and the T-pose span between the grip poses, and `body/calibration/derive.rs` derives the rest with the Drillis and Contini segment ratios. The player maps onto Rico by remapping: the hands scale by the ratio of the arms (`controllers.body.remap_hands`), and the eye height and vertical head travel by the ratio of the heights (`controllers.body.remap_head`).

## The STAGE floor

When the runtime offers STAGE, the session creates it beside LOCAL and locates the mid-eye pose against it each frame for the head's height above the floor. Everything else stays in LOCAL, so a runtime without STAGE loses only the floor height. A runtime may move the floor on a reference-space change (Monado has dropped it by a third of a metre), so shortly after a change a head-height step larger than a head moves in a frame is taken to be the floor's and removed (`FloorWatch`).

## Posture

The posture (`controllers.roomscale.posture`) is `Auto`, `Seated`, or `Standing` ([rig.md, "Posture"](../body/rig.md#posture)). Seated, the seated eye reference maps to Rico's standing eye height, the body follows the head only by the upper body, and a downward flick of the dominant stick toggles a crouch. Standing, the floor gives the true height, crouching is physical, and the body follows the head about the room.

OpenXR has no seated signal, so `Auto` (`roomscale/posture.rs`, `PostureClassifier`) classifies by the head's height. Until a height is located the player counts as seated, because a seated treatment never forces a crouch on a stander, while a standing one would on a sitter. Standing up takes a short hold above a high threshold and sitting down a long hold below a low one, so a crouch behind cover stays a crouch; a crouch held longer than that is the known failure, which the explicit postures avoid. The controllers' heights are not used, because a crouching player's hands sit as low as a seated one's.

## Head height

Standing, the true eye height is remapped onto Rico by the ratio of the heights, and the cockpit's vertical offset is the remapped height less Rico's standing eye height. The player's standing eye height is the one the body rig measures ([rig.md, "The pelvis solver"](../body/rig.md#the-pelvis-solver)), held near the calibration, because most players stand a few centimetres below their calibrated height once relaxed.

Seated, the head's travel from the seated eye reference (`roomscale/seated.rs`) scales by the height ratio. A calibrated height belongs to one chair, so the reference follows the player: a seated recenter captures it, and it drifts slowly toward the median of recent heights, so a short lean or duck stays one to one. The seated crouch drops the eyes only on foot, because in a seat, under a canopy, or swimming it would put the view into the footwell or Rico's torso.

## Crouching

The eyes' drop below Rico's standing eye height, after the remap, marks the character crouched past `crouch.enter_drop_m` and standing again above `crouch.exit_drop_m`. On the ground the drop is the crouch the body rig measures, so a look at the floor or a lean over a table does not count ([rig.md, "Posture"](../body/rig.md#posture)).

While crouched, the rig's root hook ([rig.md](../body/rig.md#who-moves-the-root)) requests [`ProxyState::CROUCHING`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::ProxyState##variant-CROUCHING) ([root-motion-and-proxy.md](../../engine/character/root-motion-and-proxy.md#capsule-dimensions-and-runtime-resize)) through [`Character::m_PendingProxyState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_PendingProxyState), only while the pending state is the default ([`Character::m_DefaultProxyState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_DefaultProxyState)) or already the crouch, so the game's own states win. The request is released once a ray up finds standing headroom.

## Teleports

A post-call detour on [`GameCameraManager::ResetCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::game_camera_manager::GameCameraManager##func-ResetCamera) counts a teleport, because the game resets the camera after every discontinuity it makes (a teleport, a respawn, a load). A move that bypasses the game world, such as a vehicle teleport, is caught by a per-tick displacement far beyond the proxy's speed. A teleport restarts the rig's bookkeeping ([rig.md, "Teleports and seat exits"](../body/rig.md#teleports-and-seat-exits)).

## Recenters

What a recenter re-bases is the rig's ([rig.md, "Recentering"](../body/rig.md#recentering)). A manual recenter (F7 or the stick-click chord, [bindings.md](bindings.md#chords)) also classifies the automatic posture and, seated, captures the seated eye reference. The seat transitions' re-bases (`vr::request_seat_recenter`) do neither, because they come on the game's schedule, not the player's.

## The fades

The head fade (`controllers.roomscale.head_fade.enabled`) darkens the view as the head goes into geometry: the rig's head probe ([rig.md, "Walls"](../body/rig.md#walls)) casts from where the head last was in the open to where it is, with the player aim's filter so it skips Rico's own bodies, and the depth past the hit sets the darkness. Seated in a vehicle it is held clear, because the head leans into cockpit glass and trim.

The head fade, the rig's push-back, and the seat transitions each set a darkness, and the VR blit darkens each eye by the darkest as it copies into the swapchain. The engine's own screen fade is not used, because the game drives it and a second writer would fight it.

## Seat transitions

Under a head-anchored camera a vehicle entry, in which Rico crosses metres of world, is a forced movement ending at an arbitrary yaw. The seat phase is classified from the body rule system's state by name (`roomscale::seat`), because the game's vehicle predicates do not separate entry, locked, and exit. `controllers.roomscale.seat_transition` picks what each edge does:

- `Ride` (the default) lets the view ride the animations and, when the seat locks, eases only the cockpit's yaw by the head's heading against the body ([rig.md](../body/rig.md#the-body-turning-under-the-view)), so a look off the body does not become a look off the vehicle's axis.
- `Instant` darkens the entry, re-bases fully at the lock, and re-bases again once out.
- `Fade` does the same, darkens the exit too, and holds the dark briefly so the re-based view settles first.

The lock needs a re-base because the seated head bone sits lower than the standing one. A shuffle between seats does not re-base. Body/tracking > Posture shows all of this page's state.
