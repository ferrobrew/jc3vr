# Integrated runtime testing

The replacement paths run under the existing enabled controller, full-body, weapon-handling, and camera settings. Existing saved switches remain effective. New tracking recordings use lossless chunked version 4 by default; an explicitly saved compression-off setting still selects raw version 4.

## Runtime coverage

- Ordinary on-foot full-body updates use the constrained temporal fit at the body solve gate. The root movement executor consumes the same committed result once. The body-follow switch still disables inferred root movement. Tracked head and interaction-hand targets remain authoritative. Missing support observations leave inferred feet released, so foot sliding and gait appearance need testing.
- Free parachute-handle targets use native resting joint observations captured during the game's existing sync. Held steering keeps its grip-derived behavior. Missing or expired native observations temporarily withhold the free target instead of substituting shoulder geometry.
- Authored camera following admits exact takeoff, landing, recovery, floor-pickup, and hanging states. The numbered native channel is applied relative to entry before tracked head motion. Unsupported exits return to the normal tracked view. Exact-state animation restarts and outgoing native blend weights remain limitations.
- Magazine grip-and-pull, button ejection, flick ejection, and belt replacement execute detachment in the character's native serial post-physics pass. The callback revalidates the current weapon and spare identity. No extra magazine latch button is required.
- A grenade draw creates an inert model item without consuming ammunition. Each hand can hold a separate grenade. Hip/chest stow, hand transfer, and inert world drop retain its identity. Deliberate throwing release creates and launches the native explosive; incidental release leaves an inert item.
- A qualifying unlimited paired-pistol rack exposes its two native gun models as independent source members. Taking one does not adopt its neighbour. Staging uses native pickup initialization with equip disabled. An external same-tag mission give racing the callback remains ambiguous.
- Human collision hulls narrow horizontal bounds while retaining height, center, user data, and state-local contact policy. The debug physical-hands controls expose the runtime switch. The current player's existing settings and subsequently constructed settings are adopted. Distinct settings for already-existing NPCs are not exhaustively discovered.

## Collider restoration

Disable and shutdown request restoration through the native pre-simulation phase. Incomplete body inventory, pending bodies, invalid bounds, or overlap defer restoration and keep normal gameplay and hooks running. The current clearance check uses a conservative full enclosing region; ground overlap can prevent restoration even where an exact shape query might allow it. No teleport or forced expansion is used. After successful restoration, shutdown validates the copied preparation generation before teardown. An active native callback after hook removal keeps the image mapped.

The conservative restoration limit can prevent ejection during a test session. A process restart remains a practical recovery from a broken test run, but it is not counted as successful live restoration.

## Test order

1. Load an ordinary on-foot scene and confirm the constrained-fit status advances. Walk physically, bend, crouch, turn, and reach with one hand. Toggle body follow off and verify that inferred root movement stops.
2. Repeat takeoff, landing, recovery, floor pickup, and hanging. Verify that head motion remains independent and that entering a sequence a second time still follows it.
3. Open the canopy, acquire both resting handles, steer, release, and reacquire. Compare the target markers with visible native rest.
4. Extract a loaded magazine by grip and pull, abort a partial pull, reinsert it, and verify chamber and magazine ammunition. Repeat button, flick, and belt operations, including weapon changes while an operation is pending.
5. Draw safe grenades into both hands at zero stock, transfer and stow them, drop one without throwing, recover it, and deliberately throw the other. Confirm no stock refund or deduction and no fuse on the inert items.
6. At a qualifying paired rack, take each member in either order with the opposite hand occupied and the holsters full. Confirm the other member stays at the source. Leave and return, then repeat across a reset.
7. Exercise narrowed collision in standing, crouching, swimming, and wingsuit states. Request restoration in a clear area and beside an obstruction. A deferred request must leave gameplay responsive and must not partially restore bodies.
8. Capture full-rate tracking with joints while exercising these paths. Compare stored/uncompressed bytes, queue/drop counters, shutdown recovery, and replay. No real-capture compression performance result is established yet.

Compiler checks and pure tests do not establish the native callback schedule, model factories, live physics response, or headset comfort. The runtime paths are connected so these behaviors can be tested together.
