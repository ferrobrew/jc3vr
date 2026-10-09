//! How the weapon handling's candidates become grip targets, and where their markers go.

use crate::{
    controllers::{
        config::ControllersConfig,
        frame::Hand,
        grab::{Candidate, GrabVolume, GripTarget, HandPoint, Mark, MarkAt, Shown, Tier},
        hands::ActiveHands,
        vehicles::wheel::CockpitTransform,
        weapons::{foregrip, volumes::Anchor},
    },
    headpose,
};

/// How the weapon handling's candidates become grip targets: which point of the hand reaches the
/// body's sockets and pouches, and where the markers go.
pub(super) struct Place {
    /// The hand's point for the body's sockets and pouches: the player's own grip on the player's
    /// body, the drawn one on the character's skeleton.
    pub(super) by: HandPoint,
    pub(super) xf: CockpitTransform,
}

impl Place {
    pub(super) fn new(active: &ActiveHands, anchor: &Anchor) -> Self {
        Self {
            by: match anchor {
                Anchor::Body { .. } => HandPoint::Player,
                Anchor::Skeleton(_) => HandPoint::Drawn,
            },
            xf: CockpitTransform {
                head_world: headpose::query().position,
                head_cockpit: active.head.position,
                body: headpose::xr::body_rotation(),
                world_scale: active.world_scale,
            },
        }
    }

    /// A socket or a pouch on the body, always marked.
    pub(super) fn body(&self, c: &Candidate) -> GripTarget {
        c.target(
            self.by,
            Mark::ring(
                MarkAt::Cockpit {
                    point: c.centre,
                    on_body: self.by == HandPoint::Player,
                },
                mark_size(c.radius),
            )
            .showing(Shown::Always),
        )
    }

    /// A pouch, and its reach margin: a hand that misses it by a little still takes from it when
    /// nothing else is in reach, because a hand feeling for a pouch on the body lands a little off
    /// it more often than not.
    pub(super) fn pouch(&self, c: &Candidate, margin: f32) -> [GripTarget; 2] {
        let pouch = self.body(c);
        let around = Candidate {
            radius: c.radius + margin.max(0.0),
            ..*c
        }
        .target(self.by, pouch.mark.showing(Shown::Never))
        .ranked(Tier::Fallback);
        [pouch, around]
    }

    /// Where a hand touched a gun docked on the body.
    pub(super) fn touch(&self, c: &Candidate) -> GripTarget {
        c.target(
            self.by,
            Mark::ring(
                MarkAt::Cockpit {
                    point: c.centre,
                    on_body: self.by == HandPoint::Player,
                },
                mark_size(c.radius),
            ),
        )
    }

    /// A socket on the character's skeleton for a carried weapon, always marked.
    pub(super) fn socket(&self, c: &Candidate) -> GripTarget {
        c.target(
            HandPoint::Drawn,
            Mark::ring(
                MarkAt::Cockpit {
                    point: c.centre,
                    on_body: false,
                },
                mark_size(c.radius),
            )
            .showing(Shown::Always),
        )
    }

    /// A part of the gun in `holder`'s hand.
    pub(super) fn on_hand(&self, c: &Candidate, holder: Hand) -> GripTarget {
        c.target(
            HandPoint::Drawn,
            Mark::ring(
                MarkAt::OnHand {
                    hand: holder,
                    point: c.centre,
                },
                mark_size(c.radius),
            ),
        )
    }

    /// Something lying in the world.
    pub(super) fn world(&self, c: &Candidate) -> GripTarget {
        c.target(
            HandPoint::Drawn,
            Mark::ring(
                MarkAt::World(self.xf.point_to_world(c.centre)),
                WORLD_MARK_M,
            ),
        )
    }
}

/// The braced foregrip, held by the bracing hand, marked where it holds the gun.
pub(super) fn held_foregrip(
    hold: &foregrip::Hold,
    active: &ActiveHands,
    config: &ControllersConfig,
) -> Option<GripTarget> {
    let mut posed = active.hands;
    foregrip::apply(&mut posed, &config.weapons.foregrip);
    let brace = hold.holder.other();
    let point = posed[brace.index()]?.grip.position;
    let target = Candidate {
        volume: GrabVolume::Foregrip,
        centre: point,
        radius: config.weapons.foregrip.radius_m,
        hand: Some(brace),
    }
    .target(
        HandPoint::Drawn,
        Mark::ring(
            MarkAt::OnHand {
                hand: hold.holder,
                point,
            },
            FOREGRIP_MARK_M,
        ),
    );
    Some(target.held(brace))
}

/// A marker's size for a target reached within `radius`: the reach's diameter, kept within a size
/// that marks the spot without covering what is around it.
pub(super) fn mark_size(radius: f32) -> f32 {
    (radius * 2.0).clamp(MIN_MARK_M, MAX_MARK_M)
}

const MIN_MARK_M: f32 = 0.03;
const MAX_MARK_M: f32 = 0.12;
/// The marker on something lying in the world, metres.
pub(super) const WORLD_MARK_M: f32 = 0.06;
/// The marker where a held charge would stick, metres.
pub(super) const CHARGE_MARK_M: f32 = 0.08;
/// The marker on a braced foregrip, metres.
const FOREGRIP_MARK_M: f32 = 0.03;
