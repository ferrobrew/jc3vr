# Physical grapple

The grapple hand works the line: a yank zips, the pull's speed drives the reel, a yank during the reel boosts it, pulling back with tethers attached tightens them, the arm pumps the swing while hanging, and the hand feels the line's tension. Every part sits on top of the bindings, so the buttons keep working and the hand adds to them. The code is `grapple::physical` in `jc3vr-grapple`, with the hang swing in `jc3vr_body::controllers::hands::anchor`; the engine side is [grapple-pipeline.md](../../engine/gameplay/grapple-pipeline.md), "The reel" and "The tether squeeze". The lock on the reel's line is [grapple-lock.md](grapple-lock.md), and the snatch [grapple-snatch.md](grapple-snatch.md).

None of it has a switch: it acts under the controller scheme. The tuning is `crates/jc3vr-grapple/tuning/grapple.toml` ([tuning.md](../tuning.md)): `button` for the gestures, `grapple_yank` and `yank` for the zip's yank, and `physical` for the pull, reel, retract, and swing.

## The grapple button

The grapple is the off-hand's upper face button, not the grip, which takes what the hand reaches ([grip.md](grip.md)). With nothing attached it is the gamepad's grapple button: a tap fires, and a hold on one target released at a second tethers them. With tethers or a line attached, `controllers::grapple_button` reads gestures before the game sees them:

- a tap fires, once no second tap follows within `button.double_tap_s` (at once on a line with no tethers);
- a double tap releases every tether and fires nothing;
- a hold with a grapple target under the aim is the game's new tether;
- a hold with nothing new under the aim tightens the tethers, ramping up over `button.tighten_ramp_s`, and pulling the hand back tightens harder ([The tether retract](#the-tether-retract));
- a yank while held, with a line attached, zips as a tap would.

During the reel and its wind-up the button is the lock's ([grapple-lock.md](grapple-lock.md#the-button)). The hand's pulls act only while the button is held, so a hand moved back in play never zips or tightens. The L trigger still retracts tethers while the left hand holds nothing that uses the trigger. Both fires of a tether come from the grapple hand's aim ([hands-and-aiming.md](hands-and-aiming.md#grapple-aim)), and the aim sees weapons, so the button works on a gun too ([grapple-snatch.md](grapple-snatch.md#the-grapple-button-at-a-gun)).

### The yank

The zip's yank is measured along the grapple hand's aim at the tick it armed, in the cockpit frame (`controllers::yank::LineYank`): a drop of at least `grapple_yank.min_distance_m` in reach along that line at `grapple_yank.min_speed_mps`. Measuring along the aim rather than toward the head lets a seated player's hand, which pulls back past its side, zip. A pull carrying the hand out from the head across the ground by more than `yank.outward_share` is not a yank, because spreading the arms to open the wingsuit drops reach along an aim across the body as fast as a yank does; measuring across the ground keeps a yank down from a raised hand valid.

Every armed gesture on the line (the reel's and retract's pull, the lock's take-in, and the snatch's yank) latches its line the same way (`controllers::yank::ArmedLine`), with its own detector. The snatch's yank is armed only with no line out, so it never meets the zip's.

## The pull

`grapple::physical::pull::LinePull` arms when the reel starts or the tether retract arms, takes the aim of the first tracked tick as its line, and samples the grip's reach along it every input tick. It reads the pull's speed, peak-held and decaying over `physical.pull.speed_hold_s` so a pull at a reel's start counts through a reel longer than the arm, and the pull's depth, how far the hand is back from a reference that jumps out with the hand and creeps toward a hand held still. An untracked hand forgets the pull.

## The reel

While the reel or its wind-up runs, the input tick publishes a drive: a scale from the pull's speed, from `physical.reel.min_scale` to `physical.reel.max_scale`, and whether a boost runs. A detour on [`NStateTask_ReelIn::UpdateReelVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::NStateTask_ReelIn##func-UpdateReelVelocity) hands the local player's update a scaled copy of the [`GrappleTuning`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrappleTuning) (`grapple::physical::reel`); the shared resource is never written, and other characters' reels are untouched. A drive older than `physical.reel.drive_max_age_ms` is ignored, so the reel falls back to the game's when the tick stops. With `min_scale` at one, an unpulled reel runs at the game's speed.

A yank during the reel boosts it for `physical.reel.boost_s` of game time, so slow motion stretches it ([slow-motion.md](slow-motion.md)): the copy takes the retract's top speed and acceleration, which is what holding the retract does for the game's boost. It applies only when [`GrapplingHook::GetRetractAcceleratesReelIn`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-GetRetractAcceleratesReelIn) says the boost is unlocked, and it does not touch the retract input, so it never squeezes tethers.

## The tether retract

The retract arms while the button is held with dual tethers attached ([`GrapplingHook::GetNumDualTethers`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-GetNumDualTethers)), no line of the grapple's own (the retract input would pull a light object on it), nothing carrying the character, and Rico on the ground or falling (the retract input boosts the wingsuit and parachute, whose steering uses the hands). The pull's depth becomes the retract value (`grapple::physical::tether`): nothing inside a deadzone, then the cube root of the pull's fraction, since the game squeezes by the cube of the value. It goes into the bindings' analog `RETRACT_GRAPPLE`, merged with the trigger and the button's tightening, the largest winning.

## The hang swing

The hang task's detour pushes a hanging body against the grapple hand's displacement from where it took the line. The pump (`grapple::physical::swing`) adds a push against the hand's smoothed velocity relative to the head, scaled to the game's own full-stick swing impulse ([`GrapplingHangTask_SmallImpulseSwingMagnitude`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHangTask_SmallImpulseSwingMagnitude)), so pulling back on the forward swing feeds it. Hold and pump together are capped at `physical.swing.max_scale` times that impulse per tick, applied at the hips.

## The grapple in a vehicle seat

The base game takes no grapple fire in a seat; it ships a vehicle grapple that tethers what the hook hits to the vehicle but never calls it ([grapple-pipeline.md, "The grapple in a vehicle seat"](../../engine/gameplay/grapple-pipeline.md#the-grapple-in-a-vehicle-seat)). The mod runs its steps from the input tick (`grapple::seat`, with `engine.rs` for the game calls; tuning in the `seat` section).

One test, `jc3vr_body::controllers::facts::seat::stands_in` (riding per [`Character::IsInRidingInVehicleState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsInRidingInVehicleState) where the game refuses the fire), decides the button, the aim, and whether a flying hook is still seated, so the button never fires where the aim casts nothing. A seat's animation state clears the hook's fire input and the game's targeting stops refreshing the grapple slot, so in a seat the aim casts the grapple hand's ray itself, out to [`GrappleTuning::m_GrappleDistance`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrappleTuning##field-m_GrappleDistance); wherever else the game refuses the fire, the readout is marked not current so the target never freezes.

The button fires the game's custom fire, [`GrapplingHook::FireHookCustom`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-FireHookCustom), at once with nothing attached, and with tethers attached behaves as on foot (tap, double tap, hold). The wire's near end moves from Rico's gauntlet to the vehicle at the grapple hand's grip and follows the hand while the button is held. Once the hook holds and the button is up, the wire becomes one of the hook's tethers ([`GrapplingHookWire::PushGrapplingHookWire`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHookWire##func-PushGrapplingHookWire)) and from there is the game's own. A long hold released at a second target ties the two instead. A hook still flying after `seat.flight_s`, or out when Rico leaves the seat or the controllers, is called back ([`GrapplingHook::CancelActiveWire`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-CancelActiveWire)); a pause of the controllers keeps it, and the flight time counts only ticks that run. The tether plays `Event::LineLock`.

## Feedback and threading

The fire, attach, and yank play `Event::GrappleFire`, `Event::GrappleAttach`, and `Event::Yank`, and a retract `Event::TetherRetract`. Two levels are summed on the grapple hand: `Sustain::GrappleTension` from [`GrapplingHook::GetHighestWireTension`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-GetHighestWireTension) while the game squeezes, and `Sustain::GrappleReel` while the hand drives the reel ([haptics.md](haptics.md)).

The input tick's game stage ([bindings.md](bindings.md#injection-timing)) steps the pull and publishes the drive; the reel's detour reads it from a leaf lock on the game thread, and the readout has its own leaf lock so the status rows never wait ([threading.md](../threading.md)). The Status page's `Grapple` section shows the hook's phase, the attached target, the reel scale, the tethers, `Line lock`, `Seat grapple`, the hang swing, `Snatch`, and the line haptics. The log, under the `grapple` target, notes attaches, reels, boosts, retracts, and each seat grapple's fire, tether, miss, and refusal.

## Unverified in the headset

- That the reel's velocity update reads nothing of the tuning beyond the bound fields in every path.
- That the retract's analog value from the bindings drives the squeeze as the trigger's does.
- Whether a hand pulled hard in the zip's wind-up gives an unwanted boost, and how hard the pump should push.
