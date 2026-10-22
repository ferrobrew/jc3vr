//! The signals a scenario can check: typed reads of the snapshots the mod's systems publish for
//! the debug UI and the status rows ([`crate::status`]), and of the status rows themselves.
//!
//! Every read takes the publishing module's leaf slot or atomics only, as the status rows
//! do, so the runner may read them from the game thread at the top of the frame.

use std::{fmt, time::Instant};

use crate::{
    controllers::{
        body::{
            procedural::{self, authority::Case},
            rig::root as rig_root,
        },
        frame::{self as controller_frame, Hand},
        grab::{latch::GripLatch, tick as grab_tick},
        hands::grip,
        locomotion,
        mode::{self, Mode, OnFootVariant},
        physical_hands::{
            self, climb,
            physics::{self, BodyPhase, guns},
            stunt::{self, solve::StuntKind},
            vault,
        },
        scheme, vehicles,
        weapons::{
            self, ads, c4, collide, grenade, grip_watch,
            hold::second,
            inventory, regions,
            reload::{self, loose},
        },
    },
    frame_timing,
    status::{self as status_page, Health},
    testing::replay,
};

/// A value a probe reads on one frame.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Bool(bool),
    Number(f64),
    /// The signal has no value on this frame: not measured yet, or nothing to measure.
    Missing,
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Bool(b) => write!(f, "{b}"),
            Value::Number(n) => write!(f, "{n}"),
            Value::Missing => write!(f, "missing"),
        }
    }
}

/// What kind of value a signal reads.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum SignalKind {
    /// A yes or no.
    Flag,
    /// A count since the payload loaded, which only grows; a scenario usually checks how far it
    /// grew during the replay.
    Counter,
    /// A measurement.
    Measure,
}

macro_rules! signals {
    ($($variant:ident => $name:literal, $kind:ident, $doc:literal;)*) => {
        /// A published signal a scenario can check.
        #[derive(Debug, Copy, Clone, PartialEq, Eq)]
        pub(crate) enum Signal {
            $(#[doc = $doc] $variant,)*
        }

        impl Signal {
            /// Every signal, in the order the documentation lists them.
            pub(crate) const ALL: &[Signal] = &[$(Signal::$variant,)*];

            /// The name a scenario file uses.
            pub(crate) const fn name(self) -> &'static str {
                match self {
                    $(Signal::$variant => $name,)*
                }
            }

            pub(crate) const fn kind(self) -> SignalKind {
                match self {
                    $(Signal::$variant => SignalKind::$kind,)*
                }
            }

            /// What the signal reads.
            pub(crate) const fn description(self) -> &'static str {
                match self {
                    $(Signal::$variant => $doc,)*
                }
            }
        }
    };
}

signals! {
    ReplayDriving => "replay.driving", Flag, "A replay is publishing its frames.";
    VrRendering => "vr.rendering", Flag, "A VR frame is rendering to the headset.";
    Recenters => "vr.recenters", Counter, "Recenters of the cockpit baseline.";
    FrameTimeP95Ms => "frame.p95_ms", Measure, "The 95th percentile of the recent frame intervals, milliseconds.";
    ControllerFrameFresh => "controllers.frame_fresh", Flag, "A controller frame is published and fresh.";
    ControllerScheme => "controllers.scheme", Flag, "The controller scheme is selected rather than the gamepad.";
    OnFoot => "controllers.on_foot", Flag, "The mode detector has the player on foot on the ground.";
    Swimming => "controllers.swimming", Flag, "The mode detector has the player swimming.";
    InVehicle => "controllers.in_vehicle", Flag, "The mode detector has the player in a vehicle seat.";
    ControlCues => "vehicles.control_cues", Counter, "Haptic stops, detents, and gates the held sticks and levers gave.";
    Afterburner => "vehicles.afterburner", Flag, "The plane's throttle is through its afterburner gate.";
    AfterburnerLights => "vehicles.afterburner_lights", Counter, "Times the throttle went through its afterburner gate.";
    PassengerAiming => "vehicles.passenger_aiming", Flag, "The seated passenger's body turns toward the gun hand's aim.";
    PassengerAimStarts => "vehicles.passenger_aim_starts", Counter, "Times the seated body started following the gun hand.";
    MountedHandsOn => "vehicles.mounted_hands_on", Flag, "A hand holds the mounted gun's handle.";
    MountedGrips => "vehicles.mounted_grips", Counter, "Grabs and releases of the mounted gun's handles.";
    MountedKicks => "vehicles.mounted_kicks", Counter, "Shots of the mounted gun that kicked its handles.";
    ButtonPresses => "vehicles.button_presses", Counter, "Cockpit buttons pressed and twist grips twisted by hand.";
    HornByHand => "vehicles.horn", Flag, "A hand presses the horn on the wheel's hub.";
    NitrousByHand => "vehicles.nitrous", Flag, "A hand presses the nitrous button or twists a twist grip.";
    Airborne => "controllers.airborne", Flag, "The mode detector has the player falling, in the wingsuit, or under the parachute.";
    HandBodiesRegistered => "hand_bodies.registered", Flag, "The hand bodies' pre-simulation callback is registered.";
    HandBodiesOnGameThread => "hand_bodies.on_game_thread", Flag, "The callback has fired, on the thread that registered it.";
    HandBodiesFires => "hand_bodies.fires", Counter, "Times the callback fired.";
    LeftBodyInWorld => "hand_bodies.left_in_world", Flag, "The left hand's body is acting in the world.";
    RightBodyInWorld => "hand_bodies.right_in_world", Flag, "The right hand's body is acting in the world.";
    HandBodiesCreated => "hand_bodies.created", Counter, "Hand bodies made.";
    HandBodiesContacts => "hand_bodies.contacts", Counter, "Contacts the hand bodies started.";
    HandBodiesDroppedContacts => "hand_bodies.dropped_contacts", Counter, "Contacts dropped because the queue was full.";
    HandBlows => "hand_bodies.blows", Counter, "Blows the hands dealt to characters.";
    HandBodiesError => "hand_bodies.error", Flag, "The hand bodies hold an error.";
    GunBodiesMade => "guns.made", Counter, "Held guns' bodies made.";
    GunHeld => "guns.held", Flag, "A held gun's body is joined to its hand.";
    GunBraced => "guns.braced", Flag, "A held gun's body is joined to the bracing hand too.";
    GunShots => "guns.shots", Counter, "Shots whose kick a gun body took.";
    GunBodyContacts => "guns.contacts", Counter, "Contacts the held guns' bodies started.";
    GunReseats => "guns.reseats", Counter, "Gun bodies put back on their seat in the hand.";
    GunError => "guns.error", Flag, "The held guns hold an error.";
    Climbing => "climb.climbing", Flag, "A hand grips the world.";
    Mantling => "climb.mantling", Flag, "The mantle is carrying the body over a ledge.";
    ClimbGrips => "climb.grips", Counter, "Grips of the world.";
    ClimbMovingGrips => "climb.moving_grips", Counter, "Grips of moving bodies.";
    ClimbReleases => "climb.releases", Counter, "Grips let go.";
    ClimbMantles => "climb.mantles", Counter, "Mantles completed.";
    ClimbDrops => "climb.drops", Counter, "Grips ended against the player's wish.";
    ClimbError => "climb.error", Flag, "The climbing holds an error.";
    StuntHanging => "stunt.hanging", Flag, "The game holds the player hanging from a helicopter.";
    StuntStanding => "stunt.standing", Flag, "The game holds the player standing in a vehicle's stunt area.";
    StuntHangEntries => "stunt.hang_entries", Counter, "Helicopter hangs the grips entered.";
    StuntRoofEntries => "stunt.roof_entries", Counter, "Stunt areas the mantles entered.";
    StuntReleases => "stunt.releases", Counter, "Hangs let go by opening the grips.";
    StuntRefusals => "stunt.refusals", Counter, "Stunt position entries the game refused.";
    PushUps => "push_up.pushes", Counter, "Pushes up with the hands pressed down.";
    PushingUp => "push_up.lifting", Flag, "The hands lift the body.";
    ArticulatedGrips => "articulated.grips", Counter, "Articulated objects a hand took hold of.";
    GripGrabs => "grab.taken", Counter, "Targets the grips took.";
    GripLeftHolds => "grab.left_holds", Flag, "The left grip holds what it took.";
    GripRightHolds => "grab.right_holds", Flag, "The right grip holds what it took.";
    GripLeftFist => "grab.left_fist", Flag, "The left grip is closed on nothing.";
    GripRightFist => "grab.right_fist", Flag, "The right grip is closed on nothing.";
    DoorGrips => "doors.grips", Counter, "Grips taken on a latched vehicle door.";
    DoorUnlatches => "doors.unlatches", Counter, "Vehicle door latches a hand undid.";
    DoorOnLatch => "doors.on_latch", Flag, "A hand grips a latched vehicle door.";
    CharacterGrips => "characters.grips", Counter, "Grips taken on characters.";
    Hijacks => "characters.hijacks", Counter, "Seated characters a hand pulled out through an open door.";
    CharacterYanks => "characters.yanks", Counter, "Characters on foot a hand yanked.";
    CharacterRegrips => "characters.regrips", Counter, "Fallen characters' ragdolls a hand took hold of after a yank.";
    CharacterKnockdowns => "characters.knockdowns", Counter, "Yanks and shoves that knocked a character down.";
    ProceduralRigError => "procedural.rig_error", Flag, "The procedural body could not rig the skeleton.";
    PassThroughErrorM => "procedural.pass_through_error_m", Measure, "The pass-through's largest read-back error, metres.";
    PassThroughErrorDeg => "procedural.pass_through_error_deg", Measure, "The pass-through's largest read-back error, degrees.";
    ProceduralOnFoot => "procedural.on_foot", Flag, "The hand-over table's case is on foot.";
    StanceMeasured => "procedural.stance_measured", Flag, "The neutral stance has been measured.";
    ProceduralSeated => "procedural.seated", Flag, "The torso is posed on the seat's hips in a vehicle.";
    ProceduralWingArms => "procedural.wing_arms", Flag, "Rico's arms follow the player's in the wingsuit, both hands queued.";
    ProceduralParachute => "procedural.parachute", Flag, "The torso is posed on the harness's hips under the parachute, the legs hanging.";
    ProceduralFlare => "procedural.parachute_flare", Measure, "How far the hanging legs swing forward for the parachute's brake, as a share of the full flare, under the parachute.";
    ProceduralSwimming => "procedural.swimming", Flag, "The torso is posed on the swim's hips in the water.";
    ProceduralRoof => "procedural.roof", Flag, "The torso is posed on the stunt idle's hips on a vehicle's roof.";
    ProceduralNeckErrorM => "procedural.neck_error_m", Measure, "How far the solved neck is from the point under the player's head, metres, while the procedural body poses the torso.";
    Wielded => "weapons.wielded", Flag, "A weapon is wielded.";
    WeaponInLeftHand => "weapons.in_left_hand", Flag, "The wielded weapon's first gun rides the left hand.";
    WeaponInRightHand => "weapons.in_right_hand", Flag, "The wielded weapon's first gun rides the right hand.";
    TwoHanded => "weapons.two_handed", Flag, "The wielded weapon is held in both hands' reach.";
    SecondInLeftHand => "weapons.second_in_left_hand", Flag, "A second weapon is held in the left hand.";
    SecondInRightHand => "weapons.second_in_right_hand", Flag, "A second weapon is held in the right hand.";
    SecondCarried => "weapons.second_carried", Flag, "The second weapon is one the VR inventory carries beyond the slots.";
    SecondDrops => "weapons.second_drops", Counter, "Second weapons let fall from the hand.";
    SecondPromotions => "weapons.second_promotions", Counter, "Second weapons that became the wielded weapon.";
    WeaponGrabs => "weapons.grabs", Counter, "Grabs the weapon handling saw.";
    GrenadePins => "grenade.pins_pulled", Counter, "Pins pulled from grenades taken from the pouch.";
    GrenadesThrown => "grenade.thrown", Counter, "Grenades thrown from the pouch.";
    GrenadesCaught => "grenade.caught", Counter, "Live grenades caught.";
    GrenadesThrownOn => "grenade.thrown_on", Counter, "Caught grenades thrown on.";
    C4Thrown => "c4.thrown", Counter, "Charges thrown or pressed on from the pouch.";
    C4Stuck => "c4.stuck", Counter, "Charges pressed onto a surface by hand.";
    MagazinesDropped => "magazines.dropped", Counter, "Magazine props dropped into the world and noted.";
    MagazinesPickedUp => "magazines.picked_up", Counter, "Magazines picked up from the world.";
    MagazinesSeated => "magazines.seated", Counter, "Magazines from the world seated in a gun.";
    ActionsWorked => "actions.worked", Counter, "Actions worked by hand: a slide, charging handle, bolt, or pump cycled.";
    ActionsLockedBack => "actions.locked_back", Counter, "Slides caught at the rear on an empty magazine.";
    ActionHeld => "actions.held", Flag, "A hand holds a gun's action with the physical action on.";
    ActionTravel => "actions.travel", Measure, "How far back the action of the gun in hand furthest back is, as a share of its travel, with the physical action on.";
    AdsSighted => "ads.sighted", Counter, "Times a long gun's sights came onto the eye.";
    AdsShouldered => "ads.shouldered", Counter, "Times a long gun's stock came into the shoulder.";
    AdsWelded => "ads.welded", Counter, "Times a scope came onto the eye.";
    GunContacts => "collide.contacts", Counter, "Times a held long gun came against a wall.";
    GunTouching => "collide.touching", Flag, "A held long gun is against a wall.";
    RegionCarries => "regions.carries", Counter, "Long guns taken by a part other than the grip.";
    RegionBraces => "regions.braces", Counter, "Long guns braced at the point the second hand closed on.";
    RegionGripsTaken => "regions.grips_taken", Counter, "Grips taken from a carrying hand or slid along to.";
    RegionLetGo => "regions.let_go", Counter, "Long guns left carried in the bracing hand when the holding hand let go.";
    RegionCarrying => "regions.carrying", Flag, "A hand carries the wielded gun by a part other than its grip.";
    InventoryCarried => "inventory.carried", Measure, "Weapons the inventory carries beyond the slots.";
    InventoryDraws => "inventory.draws", Counter, "Carried weapons drawn off their sockets into a hand.";
    InventoryStows => "inventory.stows", Counter, "Held weapons put onto a free socket.";
    InventoryRestored => "inventory.restored", Counter, "Carried weapons respawned onto their sockets after a reset or a load.";
    InventorySidecarWrites => "inventory.sidecar_writes", Counter, "Save sidecars written.";
    InventoryStashed => "inventory.stashed", Flag, "The carried weapons are stashed while a mission holds the player's weapons.";
    InventoryWithOthers => "inventory.with_others", Measure, "Weapons the player let go of that another character holds.";
    InventoryKept => "inventory.kept", Measure, "Weapons lying in the world the inventory keeps from the game's despawn.";
    InventoryPins => "inventory.pins", Counter, "Weapons in the world the inventory started keeping.";
    InventoryResets => "inventory.resets", Counter, "Spawn system resets the inventory saw.";
    GripFlips => "grip.flips", Counter, "Snaps of a gun on the wrist.";
    GripPins => "grip.pins", Counter, "Attach bones pinned.";
    StickSpeed => "locomotion.stick_speed_mps", Measure, "The speed the stick drives the body at, metres per second.";
    RigGapM => "rig.gap_m", Measure, "How far the ground point under the player's upright eyes is from the root, metres, while a VR frame renders.";
    RigResidualM => "rig.residual_m", Measure, "How far that point is outside the lean the chase leaves alone, metres: what the chase still has to close.";
    RigLedgerProjected => "rig.ledger_projected", Counter, "Completed steps whose chase was credited by projection, because something stopped or deflected the root.";
    RigPushbackM => "rig.pushback_m", Measure, "How far the push-back has slid the view back in its current episode, metres, while a VR frame renders.";
    RigObstructed => "rig.obstructed", Flag, "The ledger finds the player's chase obstructed: something stops the body.";
    RigPushbacks => "rig.pushbacks", Counter, "Push-back episodes: times the view started being held at a wall.";
    LegsMoving => "legs.moving", Flag, "The procedural body's feet walk rather than step standing.";
    LegsTouchdownLeadM => "legs.touchdown_lead_m", Measure, "How far ahead of its hip the last walking foot landed, metres, while the procedural body poses the legs.";
}

impl Signal {
    /// The signal named `name` in a scenario file.
    pub(crate) fn parse(name: &str) -> Option<Signal> {
        Signal::ALL.iter().copied().find(|s| s.name() == name)
    }

    /// Read the signal now.
    pub(crate) fn read(self) -> Value {
        let flag = Value::Bool;
        let count = |n: u64| Value::Number(n as f64);
        match self {
            Signal::ReplayDriving => flag(replay::is_driving()),
            Signal::VrRendering => flag(crate::vr::render_params(0).is_some()),
            Signal::Recenters => count(u64::from(crate::vr::recenter_count())),
            Signal::FrameTimeP95Ms => frame_timing::frame_percentiles()
                .map_or(Value::Missing, |p| Value::Number(f64::from(p.p95_ms))),
            Signal::ControllerFrameFresh => flag(controller_frame::latest().is_some()),
            Signal::ControllerScheme => flag(scheme::is_controllers()),
            Signal::OnFoot => flag(mode::current().mode == Mode::OnFoot(OnFootVariant::Ground)),
            Signal::Swimming => flag(mode::current().mode == Mode::OnFoot(OnFootVariant::Swimming)),
            Signal::InVehicle => flag(matches!(mode::current().mode, Mode::Vehicle(_))),
            Signal::ControlCues => count(vehicles::summary().cues),
            Signal::Afterburner => flag(vehicles::summary().outputs.afterburner),
            Signal::AfterburnerLights => count(vehicles::summary().afterburner_lights),
            Signal::PassengerAiming => flag(vehicles::passenger::status().aiming),
            Signal::PassengerAimStarts => count(vehicles::passenger::status().starts),
            Signal::MountedHandsOn => flag(vehicles::summary().mounted_holds.contains(&true)),
            Signal::MountedGrips => count(vehicles::summary().mounted_grip_events),
            Signal::MountedKicks => count(vehicles::mounted_recoil::shots()),
            Signal::ButtonPresses => count(vehicles::summary().button_presses),
            Signal::HornByHand => flag(vehicles::summary().outputs.horn),
            Signal::NitrousByHand => flag(vehicles::summary().outputs.nitrous),
            Signal::Airborne => flag(matches!(mode::current().mode, Mode::Airborne(_))),
            Signal::HandBodiesRegistered => flag(physics::callback_status().registered),
            Signal::HandBodiesOnGameThread => {
                let status = physics::callback_status();
                flag(status.firing_thread != 0 && status.firing_thread == status.game_thread)
            }
            Signal::HandBodiesFires => count(physics::callback_status().fires),
            Signal::LeftBodyInWorld => flag(physics::status().hands[0].phase == BodyPhase::Acting),
            Signal::RightBodyInWorld => flag(physics::status().hands[1].phase == BodyPhase::Acting),
            Signal::HandBodiesCreated => count(physics::status().created),
            Signal::HandBodiesContacts => count(physics::status().contacts),
            Signal::HandBodiesDroppedContacts => count(physics::status().dropped_contacts),
            Signal::HandBlows => count(physics::status().blows),
            Signal::HandBodiesError => flag(physics::status().last_error.is_some()),
            Signal::GunBodiesMade => count(guns::status().made),
            Signal::GunHeld => flag(
                guns::status()
                    .hands
                    .iter()
                    .flatten()
                    .any(|g| g.holder_joint.is_some()),
            ),
            Signal::GunBraced => flag(
                guns::status()
                    .hands
                    .iter()
                    .flatten()
                    .any(|g| g.brace_joint.is_some()),
            ),
            Signal::GunShots => count(guns::status().shots),
            Signal::GunBodyContacts => count(guns::status().contacts.iter().sum()),
            Signal::GunReseats => count(guns::status().reseats),
            Signal::GunError => flag(guns::status().last_error.is_some()),
            Signal::Climbing => flag(climb::status().climbing),
            Signal::Mantling => flag(climb::status().mantling),
            Signal::ClimbGrips => count(climb::status().grips),
            Signal::ClimbMovingGrips => count(climb::status().moving_grips),
            Signal::ClimbReleases => count(climb::status().releases),
            Signal::ClimbMantles => count(climb::status().mantles),
            Signal::ClimbDrops => count(climb::status().drops),
            Signal::ClimbError => flag(climb::status().last_error.is_some()),
            Signal::StuntHanging => flag(stunt::status().position == Some(StuntKind::Hanging)),
            Signal::StuntStanding => flag(stunt::status().position == Some(StuntKind::Standing)),
            Signal::StuntHangEntries => count(stunt::status().hang_entries),
            Signal::StuntRoofEntries => count(stunt::status().roof_entries),
            Signal::StuntReleases => count(stunt::status().releases),
            Signal::StuntRefusals => count(stunt::status().refusals),
            Signal::PushUps => count(vault::status().pushes),
            Signal::PushingUp => flag(vault::status().lifting),
            Signal::ArticulatedGrips => count(physical_hands::status().pickup.articulated_grips),
            Signal::GripGrabs => count(grab_tick::status().grabs),
            Signal::GripLeftHolds => grip_latch(Hand::Left, GripLatch::grabbing),
            Signal::GripRightHolds => grip_latch(Hand::Right, GripLatch::grabbing),
            Signal::GripLeftFist => grip_latch(Hand::Left, |l| matches!(l, GripLatch::Fist { .. })),
            Signal::GripRightFist => {
                grip_latch(Hand::Right, |l| matches!(l, GripLatch::Fist { .. }))
            }
            Signal::DoorGrips => count(physical_hands::status().pickup.door_grips),
            Signal::DoorUnlatches => count(physical_hands::status().pickup.unlatches),
            Signal::DoorOnLatch => flag(physical_hands::status().hands.iter().any(|h| h.latched)),
            Signal::CharacterGrips => count(physical_hands::status().pickup.character_grips),
            Signal::Hijacks => count(physical_hands::status().pickup.hijacks),
            Signal::CharacterYanks => count(physical_hands::status().pickup.yanks),
            Signal::CharacterRegrips => count(physical_hands::status().pickup.regrips),
            Signal::CharacterKnockdowns => count(physics::status().knockdowns),
            Signal::ProceduralRigError => flag(procedural::published_status().rig_error.is_some()),
            Signal::PassThroughErrorM => procedural::published_status()
                .round_trip
                .map_or(Value::Missing, |(m, _)| Value::Number(f64::from(m))),
            Signal::PassThroughErrorDeg => procedural::published_status()
                .round_trip
                .map_or(Value::Missing, |(_, deg)| Value::Number(f64::from(deg))),
            Signal::ProceduralOnFoot => flag(procedural::published_status().case == Case::OnFoot),
            Signal::StanceMeasured => flag(procedural::published_status().stance_measured),
            Signal::ProceduralSeated => {
                let status = procedural::published_status();
                flag(status.case == Case::Seated && status.on_animated_hips && status.weight > 0.0)
            }
            Signal::ProceduralWingArms => {
                let status = procedural::published_status();
                flag(status.case == Case::Wingsuit && status.wing_queued == [true, true])
            }
            Signal::ProceduralParachute => {
                let status = procedural::published_status();
                flag(
                    status.case == Case::Parachute
                        && status.on_animated_hips
                        && status.weight > 0.0,
                )
            }
            Signal::ProceduralFlare => {
                let status = procedural::published_status();
                if status.case == Case::Parachute {
                    Value::Number(f64::from(status.flare))
                } else {
                    Value::Missing
                }
            }
            Signal::ProceduralSwimming => {
                let status = procedural::published_status();
                flag(
                    status.case == Case::Swimming && status.on_animated_hips && status.weight > 0.0,
                )
            }
            Signal::ProceduralRoof => {
                let status = procedural::published_status();
                flag(
                    status.case == Case::StuntStanding
                        && status.on_animated_hips
                        && status.weight > 0.0,
                )
            }
            Signal::ProceduralNeckErrorM => {
                let status = procedural::published_status();
                if status.weight > 0.0 {
                    Value::Number(f64::from(status.neck_error_m))
                } else {
                    Value::Missing
                }
            }
            Signal::Wielded => flag(weapons::status().wielded.is_some()),
            Signal::WeaponInLeftHand => flag(wielded_holder() == Some(Hand::Left)),
            Signal::WeaponInRightHand => flag(wielded_holder() == Some(Hand::Right)),
            Signal::TwoHanded => flag(
                weapons::status()
                    .wielded
                    .is_some_and(|w| w.weapon.two_handed),
            ),
            Signal::SecondInLeftHand => flag(second::hand() == Some(Hand::Left)),
            Signal::SecondInRightHand => flag(second::hand() == Some(Hand::Right)),
            Signal::SecondCarried => flag(matches!(
                second::status().source,
                Some(second::Source::Carried(_))
            )),
            Signal::SecondDrops => count(second::status().drops),
            Signal::SecondPromotions => count(second::status().promotions),
            Signal::WeaponGrabs => count(weapons::status().grabs),
            Signal::GrenadePins => count(grenade::status().pins_pulled),
            Signal::GrenadesThrown => count(grenade::status().thrown),
            Signal::GrenadesCaught => count(grenade::status().catch.caught),
            Signal::GrenadesThrownOn => count(grenade::status().catch.thrown),
            Signal::C4Thrown => count(c4::status().thrown),
            Signal::C4Stuck => count(c4::status().stuck),
            Signal::MagazinesDropped => count(loose::counts().dropped),
            Signal::MagazinesPickedUp => count(loose::counts().picked_up),
            Signal::MagazinesSeated => count(loose::counts().seated),
            Signal::ActionsWorked => count(reload::status().counts.racks),
            Signal::ActionsLockedBack => count(reload::status().counts.lock_backs),
            Signal::ActionHeld => flag(
                reload::status()
                    .weapon
                    .is_some_and(|w| w.actions.iter().flatten().any(|t| t.grip.is_some())),
            ),
            Signal::ActionTravel => reload::status()
                .weapon
                .and_then(|w| {
                    w.actions
                        .into_iter()
                        .flatten()
                        .map(|t| t.position)
                        .reduce(f32::max)
                })
                .map_or(Value::Missing, |p| Value::Number(f64::from(p))),
            Signal::AdsSighted => count(ads::status().sighted),
            Signal::AdsShouldered => count(ads::status().shouldered),
            Signal::AdsWelded => count(ads::status().welded),
            Signal::GunContacts => count(collide::status().contacts),
            Signal::InventoryCarried => Value::Number(inventory::status().carried().count() as f64),
            Signal::InventoryDraws => count(inventory::status().draws),
            Signal::InventoryStows => count(inventory::status().stows),
            Signal::InventoryRestored => count(inventory::restore::status().restored),
            Signal::InventorySidecarWrites => count(inventory::restore::status().writes),
            Signal::InventoryStashed => flag(inventory::status().stashed),
            Signal::InventoryWithOthers => Value::Number(inventory::status().with_others() as f64),
            Signal::InventoryKept => Value::Number(inventory::status().world().0 as f64),
            Signal::InventoryPins => count(inventory::status().pins),
            Signal::InventoryResets => count(inventory::status().resets),
            Signal::GunTouching => flag(collide::status().touching),
            Signal::RegionCarries => count(regions::status().carries),
            Signal::RegionBraces => count(regions::status().braces),
            Signal::RegionGripsTaken => count(regions::status().grips_taken),
            Signal::RegionLetGo => count(regions::status().let_go),
            Signal::RegionCarrying => flag(
                regions::status()
                    .hands
                    .iter()
                    .flatten()
                    .any(|h| h.role == regions::HandRole::Carrying),
            ),
            Signal::GripFlips => count(grip_watch::status().flips.iter().sum()),
            Signal::GripPins => count(grip::status().pins),
            Signal::StickSpeed => locomotion::status()
                .velocity
                .map_or(Value::Missing, |v| Value::Number(f64::from(v.length()))),
            Signal::RigGapM => rig_measure(|f| f.gap_m()),
            Signal::RigResidualM => rig_measure(|f| f.residual_m()),
            Signal::RigLedgerProjected => count(rig_root::status().sim.ledger.projected),
            Signal::RigPushbackM => rig_measure(|f| f.debt_m),
            Signal::RigObstructed => {
                let frame = rig_root::status().frame;
                flag(frame.fresh(Instant::now()) && frame.pushback.obstructed)
            }
            Signal::RigPushbacks => count(rig_root::status().frame.pushbacks),
            Signal::LegsMoving => flag(procedural::published_status().legs_moving),
            Signal::LegsTouchdownLeadM => {
                let status = procedural::published_status();
                match status.touchdown_lead_m {
                    Some(lead) if status.legs_weight > 0.0 => Value::Number(f64::from(lead)),
                    _ => Value::Missing,
                }
            }
        }
    }
}

/// A measure of the rig's latest frame, missing while no VR frame renders.
fn rig_measure(read: impl Fn(&rig_root::FrameReadout) -> f32) -> Value {
    let frame = rig_root::status().frame;
    if frame.fresh(Instant::now()) {
        Value::Number(f64::from(read(&frame)))
    } else {
        Value::Missing
    }
}

/// A status row's line, by its label, for a scenario's status-row probe.
pub(crate) fn status_row(label: &str) -> Option<status_page::Line> {
    status_page::read_row(label)
}

/// The status row labelled `label`, matched without regard to case, as the rows spell it.
pub(crate) fn status_row_label(label: &str) -> Option<&'static str> {
    status_page::row_labels().find(|row| row.eq_ignore_ascii_case(label.trim()))
}

/// The health word a scenario file names, in the page's own spelling or lowercase.
pub(crate) fn parse_health(word: &str) -> Option<Health> {
    [Health::Ok, Health::Off, Health::Waiting, Health::Failed]
        .into_iter()
        .find(|h| h.word().eq_ignore_ascii_case(word))
}

/// The hand the wielded weapon's first gun rides.
fn wielded_holder() -> Option<Hand> {
    weapons::status().wielded.and_then(|w| w.weapon.holder)
}

/// Whether `hand`'s grip passes `test`, while the grip model resolves the hands.
fn grip_latch(hand: Hand, test: impl Fn(GripLatch) -> bool) -> Value {
    let status = grab_tick::status();
    if status.live {
        Value::Bool(test(status.hands[hand.index()].latch))
    } else {
        Value::Missing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_signal_parses_back_from_its_name() {
        for &signal in Signal::ALL {
            assert_eq!(Signal::parse(signal.name()), Some(signal));
        }
        assert_eq!(Signal::parse("climb.climbin"), None);
    }

    #[test]
    fn signal_names_are_distinct() {
        let mut names: Vec<_> = Signal::ALL.iter().map(|s| s.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Signal::ALL.len());
    }

    #[test]
    fn status_rows_are_found_by_their_label_in_any_case() {
        assert_eq!(status_row_label("callback"), Some("CALLBACK"));
        assert_eq!(
            status_row_label("PASS-THROUGH ERROR"),
            Some("PASS-THROUGH ERROR")
        );
        assert_eq!(status_row_label("NO SUCH ROW"), None);
    }

    #[test]
    fn health_words_parse_in_either_case() {
        assert_eq!(parse_health("OK"), Some(Health::Ok));
        assert_eq!(parse_health("waiting"), Some(Health::Waiting));
        assert_eq!(parse_health("fine"), None);
    }
}
