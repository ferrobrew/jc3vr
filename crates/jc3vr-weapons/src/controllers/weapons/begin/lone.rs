//! The begin half's lone-gun stage: the handle and the foregrip of the gun held in one hand while
//! the other is free, where the free hand touches it, and the handover to the free hand.

use glam::Vec3;
use jc3vr_body::controllers::{
    grab::{Candidate, GrabVolume},
    hands::{ActiveHands, CockpitHand},
};
use jc3vr_core::game_tick::GameTick;
use jc3vr_input::frame::{Hand, Pose};

use crate::controllers::weapons::{
    begin::{Begin, weighted_hands},
    foregrip,
    handling_tuning::HANDLING,
    hold,
    hold::tuning::HOLD,
    regions,
    regions::{GunRegion, GunShape},
    state::OffTouch,
};

/// The lone gun's targets: on a long gun, the free hand's touch along it becomes its handle (a
/// handover) or its foregrip, and a bracing hand slides along it; without a measured shape, the
/// foregrip is offered where the geometry puts it. Then the broad handover of the lone gun to the
/// free hand.
pub(super) fn lone_gun(_game: &GameTick<'_>, b: &mut Begin) {
    let active = &b.active;
    let (lone, braced, hold) = (b.lone, b.braced, b.hold);
    // A hand holding something of its own braces nothing and takes nothing from the other hand, so
    // only the lone gun's free hand does.
    b.foregrip_hand = braced.and_then(|w| w.holder).map(Hand::other);
    let foregrip_hand = b.foregrip_hand;
    let handover_allowed =
        b.context.modes.transfer && lone.is_some_and(|w| hold::owned_holder(w.address).is_some());
    let on_gun = match (braced, b.braced_geometry, b.shape) {
        (Some(w), Some(g), Some(shape)) => w.holder.and_then(|holder| {
            Some(GunOnHand {
                hand: weighted_hands(active)[holder.index()]?,
                geometry: g,
                shape,
            })
        }),
        _ => None,
    };
    let holder = braced.and_then(|w| w.holder);
    b.context.touch = None;
    b.context.slide = None;
    match on_gun {
        Some(gun) => {
            let touch = foregrip_hand
                .filter(|_| hold.is_none())
                .and_then(|off| off_touch(&gun, off, active));
            b.context.touch = touch.map(|(touch, _)| touch);
            match (touch, holder) {
                (Some((touch, centre)), Some(holder)) if touch.hit.region == GunRegion::Grip => {
                    if handover_allowed {
                        b.targets.push(
                            b.place.on_hand(
                                &Candidate {
                                    volume: GrabVolume::Handle,
                                    centre,
                                    radius: gun_volume_radius(touch.hit.distance)
                                        .min(HOLD.get().hold.handover_max_m),
                                    hand: Some(touch.hand),
                                },
                                holder,
                            ),
                        );
                    }
                }
                (Some((touch, centre)), Some(holder)) => {
                    b.targets.push(
                        b.place.on_hand(
                            &Candidate {
                                volume: GrabVolume::Foregrip,
                                centre,
                                radius: gun_volume_radius(touch.hit.distance)
                                    .min(HOLD.get().hold.handover_max_m),
                                hand: Some(touch.hand),
                            },
                            holder,
                        ),
                    );
                }
                _ => {}
            }
            if let Some(h) = hold.filter(|h| braced.is_some_and(|w| w.address == h.weapon)) {
                // A pump the reload has out sets where the hand is along the barrel.
                if let Some((point, region)) = brace_slide(&gun, h.holder.other(), active)
                    && !foregrip::pumping(h.weapon)
                {
                    b.context.slide = Some((point, region));
                }
            }
        }
        None => {
            if hold.is_none()
                && let (Some(g), Some(w), Some(off)) = (b.braced_geometry, braced, foregrip_hand)
                && let Some(holder) = w.holder
                && let Some(holding_hand) = weighted_hands(active)[holder.index()]
            {
                b.targets.push(b.place.on_hand(
                    &Candidate {
                        volume: GrabVolume::Foregrip,
                        centre: foregrip::foregrip_point(&holding_hand, &g),
                        radius: HANDLING.get().foregrip.radius_m,
                        hand: Some(off),
                    },
                    holder,
                ));
            }
        }
    }
    if handover_allowed
        && hold.is_none()
        && let Some(w) = lone
        && let Some(holder) = hold::owned_holder(w.address)
        && let Some(current) = weighted_hands(active)[holder.index()]
    {
        // SAFETY: The owned held weapon is read on the game thread before the world update.
        let grip = unsafe {
            hold::take::measured_handle_point(w.address, holder, &current, active.world_scale)
        }
        .unwrap_or(current.grip.position);
        if let Some(candidate) =
            hold::world::broad_handover_candidate(hold::world::HandoverPolicy {
                allowed: handover_allowed,
                engaged_foregrip: hold.is_some(),
                guns: b.ticked.guns,
                offer: hold::world::Handover {
                    holder,
                    grip,
                    radius: HOLD.get().hold.handle_radius_m,
                    free: b.free,
                    points: active.hands.map(|hand| hand.map(|hand| hand.grip.position)),
                    contact: b.context.touch.as_ref().map(|touch| touch.hit.region),
                },
            })
        {
            b.targets.push(b.place.on_hand(&candidate, holder));
        }
    }
}

/// The radius of a volume on a gun centred where a hand `distance` away closes on it: the hand is
/// just inside it, so any volume the hand is well inside of wins over the gun (the action's, while
/// the chamber waits on it).
pub(super) fn gun_volume_radius(distance: f32) -> f32 {
    let tuning = &HOLD.get().begin;
    (distance.max(0.0) + tuning.gun_volume_margin_m) / tuning.gun_volume_depth
}

/// The lone long gun on its holding hand, as the regions place it.
struct GunOnHand {
    /// The holding hand as the gun is shown on it.
    hand: CockpitHand,
    geometry: foregrip::GripGeometry,
    shape: GunShape,
}

impl GunOnHand {
    /// The gun's own frame in the cockpit frame, with the position in cockpit metres.
    fn pose(&self) -> Pose {
        self.hand.wrist.compose(self.geometry.gun)
    }
}

/// Where `hand` (the hand without the gun) touches the gun, and the centre of its volume in the
/// cockpit frame.
fn off_touch(gun: &GunOnHand, hand: Hand, active: &ActiveHands) -> Option<(OffTouch, Vec3)> {
    let point = active.hands[hand.index()]?.grip.position;
    let scale = active.world_scale;
    let pose = gun.pose();
    let hit = regions::classify(
        &gun.shape,
        regions::into_gun(pose, point, scale),
        HOLD.get().regions.grip_radius_m,
        HOLD.get().regions.reach_m,
    )?;
    let touch = OffTouch {
        hand,
        hit,
        anchor_in_wrist: regions::out_of_gun(gun.geometry.gun, hit.anchor, scale),
    };
    Some((touch, regions::out_of_gun(pose, hit.anchor, scale)))
}

/// Where the bracing hand `hand` has slid to along the handguard or the barrel, from where its
/// controller is: the point in the holding wrist's frame, and the region.
fn brace_slide(gun: &GunOnHand, hand: Hand, active: &ActiveHands) -> Option<(Vec3, GunRegion)> {
    let point = active.hands[hand.index()]?.grip.position;
    let hit = regions::classify(
        &gun.shape,
        regions::into_gun(gun.pose(), point, active.world_scale),
        HOLD.get().regions.grip_radius_m,
        HOLD.get().regions.reach_m,
    )?;
    hit.region.slides().then(|| {
        (
            regions::out_of_gun(gun.geometry.gun, hit.anchor, active.world_scale),
            hit.region,
        )
    })
}
