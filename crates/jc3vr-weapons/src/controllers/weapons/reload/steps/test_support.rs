//! The reload steps' shared fixtures: an insertion state and tick with one gun in the dominant
//! hand, and the belt reload's offer on them. The steps' own tests use them, and so do the
//! bindings' summary tests in the gameplay crate, through the `test-support` feature.

use glam::{Mat4, Quat};
use jc3vr_body::controllers::{body::rig::frames::Frames, hands::pose::CockpitHand};
use jc3vr_view::controllers::cockpit::CockpitTransform;

use super::*;
use crate::controllers::weapons::{
    capabilities::{GunHand, GunOwner},
    reload::{
        BeltEligibility, ReloadStatus,
        eject::{DoubleTapDetector, TapDetector},
        extraction::Pull,
        knock::Knocks,
        plan::WellFrame,
        records::Records,
        tuning::RELOAD,
    },
};

pub(crate) fn state(stage: InsertStage) -> State {
    let status = ReloadStatus {
        insert_hands: Some((Hand::Left, Hand::Right)),
        insertion: insert::Insertion {
            stage,
            ..Default::default()
        },
        ..Default::default()
    };
    State {
        status,
        tick: None,
        pouches: [None; 2],
        speedloaders: [None; 2],
        cylinders: [None; 2],
        belt_rings: [None; 2],
        stocked_pouch: None,
        pouch_refusal: None,
        free: Some(Hand::Left),
        loose_seen: Vec::new(),
        frames: Some(Frames {
            cockpit: CockpitTransform {
                head_world: Vec3::ZERO,
                head_cockpit: Vec3::ZERO,
                body: Quat::IDENTITY,
                world_scale: 1.0,
            },
            character: Mat4::IDENTITY,
        }),
        records: Records::new(),
        measured: [None; 2],
        extraction: Pull::new(),
        pending_detach: None,
        pending_ejects: [None; 2],
        detach_generation: 0,
        extraction_blocks_belt: [false; 2],
        extraction_needs_neutral: [false; 2],
        insertion_source: InsertionSource::RetainedMagazine,
        ready_cue: insert::ReadyCue::new(),
        tap: TapDetector::new(),
        fire_taps: [DoubleTapDetector::new(); 2],
        flicks: [FlickDetector::new(); 2],
        belts: [BeltPush::new(); 2],
        last_grips: [None; 2],
        drawn: None,
        since_draw_s: 10.0,
        knocks: Knocks::new(),
    }
}

pub(crate) fn tick(seating: bool) -> Tick {
    let grip = Pose::IDENTITY;
    let top = insert::held_round(
        frames::held_line(grip, Hand::Left, RELOAD.held_palm_gap_m),
        frames::pointing(grip),
        RELOAD.held_top_m,
        Vec3::ZERO,
    )
    .unwrap();
    let depth = if seating {
        RELOAD.seat_depth_m * 1.1
    } else {
        -0.2
    };
    let well = Well {
        mouth: top.top - top.axis * depth,
        axis: top.axis,
    };
    Tick {
        sequence: Some(7),
        weapons: [
            Some(WeaponTick {
                address: 1,
                ammo_owned: false,
                key: RecordKey::Object(1),
                plan: ReloadPlan {
                    load: LoadKind::Magazine,
                    action: ActionKind::Slide,
                    well: WellFrame::Gun {
                        mouth: Vec3::ZERO,
                        axis: Vec3::Y,
                    },
                    handle: None,
                    travel_m: 0.06,
                    part_top_m: None,
                    part_axis: Vec3::Y,
                    spare_top_m: None,
                    round: None,
                    launcher: false,
                    spent_round_replaced: false,
                },
                cylinder: None,
                from_table: true,
                reading: engine::Reading {
                    class: 0,
                    name_hash: 1,
                    capacity: 30,
                    magazine: 0,
                    chambers: 0,
                    out: true,
                    exempt: false,
                    unlimited: false,
                },
                reserve: 70,
            }),
            None,
        ],
        dominant: Hand::Right,
        guns: [
            None,
            Some(GunTick {
                weapon: 0,
                index: 0,
                magazine: None,
                wrist: Pose::IDENTITY,
                frame: None,
                well: Some(well),
                handle: None,
            }),
        ],
        hands: [
            Some(CockpitHand {
                wrist: grip,
                grip,
                aim: grip,
            }),
            None,
        ],
        world_scale: 1.0,
    }
}

pub(super) const SEQUENCE: u64 = 7;

pub(super) fn fixture() -> (WeaponSettings, State, Tick, [Option<GunOwner>; 2]) {
    let config = WeaponSettings::default();
    let mut state = state(InsertStage::Idle);
    state.status.insert_hands = None;
    state.pouches = [Some(Vec3::ZERO); 2];
    let mut tick = tick(false);
    let hand = config.dominant;
    if hand != tick.dominant {
        tick.guns.swap(0, 1);
    }
    tick.dominant = hand;
    let guns = Hand::BOTH.map(|h| {
        (h == hand).then_some(GunOwner {
            weapon: 1,
            role: GunHand::Grip,
        })
    });
    (config, state, tick, guns)
}

pub(super) fn offer(
    state: &State,
    tick: &Tick,
    config: &WeaponSettings,
    guns: [Option<GunOwner>; 2],
) -> Option<BeltEligibility> {
    state
        .belt_finish(tick)
        .eligibility_with(SEQUENCE, config, guns, guns, |address| {
            let weapon = tick.weapon(address)?;
            Some((weapon.reading, weapon.reserve))
        })
}

pub(crate) fn eligible_belt_for_test(
    config: &WeaponSettings,
) -> (BeltEligibility, [Option<GunOwner>; 2]) {
    let (_, state, mut tick, _) = fixture();
    if tick.dominant != config.dominant {
        tick.guns.swap(0, 1);
    }
    tick.dominant = config.dominant;
    let guns = Hand::BOTH.map(|hand| {
        (hand == tick.dominant).then_some(GunOwner {
            weapon: 1,
            role: GunHand::Grip,
        })
    });
    (offer(&state, &tick, config, guns).unwrap(), guns)
}
