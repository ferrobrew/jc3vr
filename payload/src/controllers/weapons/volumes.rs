//! Placing the body's grab volumes: the grenade pouch, the magazine pouch, and the C4 pouch, beside
//! the sockets' reach ([`SOCKETS`]; a socket's volume sits where the socket's weapon is placed).
//! Pure.
//!
//! The volumes ride the character's skeleton, the body the player sees, carried into the cockpit
//! frame: what the player sees on the body is what they reach for, and the hands reach it as they
//! are drawn. The belt pouch rides the pelvis (the line through the hip joints) and the chest
//! pouches ride the chest (the line through the shoulder joints), both turned up along the spine ([`BodyFrame`]), so the volumes follow the body through a crouch,
//! a lean, a fall, or the parachute as the animation and the procedural body pose it. Lengths that
//! scale with the body are fractions of the skeleton's own torso.
//!
//! Each volume sits at an offset from a landmark of the skeleton:
//!
//! - the chest's front on each side, below and inside the shoulder joint, for the grenade and C4
//!   pouches ([`chest_sides`] says which pouch takes which side);
//! - the belt's front on the side of the hand without a gun, for the magazine pouch: above the hip
//!   joints, a few centimetres from the midline, in front of the belly.
//!
//! The pouches' sides are laid out around the hand that holds the guns ([`gun_side`]), not the
//! dominant hand, because the gun can ride the other hand.
//!
//! The landmarks of the chest's and the belt's front are on the body's surface. The magazine pouch's
//! offset carries it out in front of the surface, and the chest pouches' brings them back to it,
//! where a hand laid on the body closes. A look down bends the character's chest forward, which
//! the chest pouches are kept off ([`BodyFrame::looking`]).
//!
//! The back's free sockets are reached for docking lower and further out than their weapons sit
//! ([`reach_back_sockets`]).

use body_rig::smooth::smoothstep_between;
use glam::{Mat3, Quat, Vec3};

use crate::controllers::{
    body::{self, rig::frames::SkeletonLandmarks},
    frame::Hand,
    grab::{Candidate, GrabVolume},
    loadout::Socket,
};

/// The sockets' fixed reach.
pub const SOCKETS: SocketTuning = SocketTuning {
    hip_radius_m: 0.12,
    shoulder_radius_m: 0.17,
    chest_radius_m: 0.09,
};

/// The pouches' fixed placement. Each pouch's centre is a few centimetres out from the body's
/// surface, where a hand laid flat on the body closes.
pub const POUCHES: PouchTuning = PouchTuning {
    grenade: CHEST_POUCH,
    c4: C4_POUCH,
    magazine: POUCH,
    reach_margin_m: 0.05,
};

/// The body's sockets: how near a hand must come to each, metres. A socket's grab volume is centred
/// where the socket places its weapon, so the socket has no placement of its own here.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SocketTuning {
    /// The sockets on both hips.
    pub hip_radius_m: f32,
    /// The sockets over the shoulders, the right one where the game docks the two-handed slot and
    /// the left one where it docks the special slot, and the outer back.
    pub shoulder_radius_m: f32,
    /// The socket on the chest.
    pub chest_radius_m: f32,
}

/// The pouches on the front of the body: the magazine pouch at the front of the belt on the free
/// hand's side, and the grenade and C4 pouches on either side of the chest. Each is placed only
/// while its feature is on: the grenade pouch with the grenade throw, the C4 pouch with the C4
/// throw, and the magazine pouch while the true reload reloads the held weapon. Offsets are given
/// for the right side and mirrored for the left.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PouchTuning {
    /// The grenade pouch, offset from the chest's front on its side.
    pub grenade: VolumeSpec,
    /// The C4 pouch, offset from the chest's front on its side.
    pub c4: VolumeSpec,
    /// The universal reload pouch, offset from the belt's front on the free hand's side. It hands
    /// out what the held weapon takes: a magazine, a shell, a rocket, or a pod.
    pub magazine: VolumeSpec,
    /// How far outside a pouch's surface a grip press still takes from it, metres, when the hand
    /// is in no other volume: a hand feeling for a pouch lands a little off it, and a press there
    /// must not fire the grapple instead.
    pub reach_margin_m: f32,
}

/// A spherical grab volume placed relative to a body landmark.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct VolumeSpec {
    /// The centre's offset from the landmark in the body frame (+X right, +Y up, +Z back), metres,
    /// given for the right side and mirrored across the body's midline for the left.
    pub offset_m: Vec3,
    pub radius_m: f32,
}

/// The body the volumes ride: the character's skeleton in the cockpit frame, with its girdles'
/// orientations.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BodyFrame {
    pub skeleton: SkeletonLandmarks,
    /// The pelvis: +X along the line from the left hip joint to the right, +Y up the spine, and -Z
    /// forward.
    pub pelvis: Quat,
    /// The chest, as the pelvis but along the line through the shoulder joints.
    pub chest: Quat,
    /// The chest the chest pouches ride: the chest itself, or with the head looking down, the chest
    /// stood back up over the hips by the look's share ([`BodyFrame::looking`]).
    pouch_chest: Girdle,
}

impl BodyFrame {
    /// The frame of `skeleton` (in the cockpit frame), or `None` for a skeleton whose spine or
    /// girdles are degenerate.
    pub fn on(skeleton: SkeletonLandmarks) -> Option<Self> {
        if !skeleton.is_finite() {
            return None;
        }
        let spine = skeleton.chest() - skeleton.hip_centre();
        let chest = girdle_basis(skeleton.shoulders, spine)?;
        Some(Self {
            skeleton,
            pelvis: girdle_basis(skeleton.hips, spine)?,
            chest,
            pouch_chest: Girdle {
                centre: skeleton.chest(),
                basis: chest,
            },
        })
    }

    /// The frame with the chest pouches kept off the forward bend that the head's look down,
    /// `head` (its orientation in the cockpit frame, whose +Y is up), puts in the body. Looking
    /// down at the chest bends the character's back by a share of the look, and the body rig reads
    /// the neck pivot coming forward over the chest as a lean; the player's own chest barely
    /// moves, so pouches carried by the bend would leave the hand that reaches for them. The chest
    /// pouches therefore ride the chest turned back toward upright about the middle of the hip
    /// joints, by the look's share ([`look_down_share`]), and only while the spine is near upright
    /// ([`bend_share`]): a body lying in a skydive, or folded far past what a look explains, keeps
    /// its pouches on its chest. A lean with the head level keeps them on the bent chest too.
    pub fn looking(self, head: Quat) -> Self {
        let hips = self.skeleton.hip_centre();
        let spine = self.skeleton.chest() - hips;
        let Some(up) = spine.try_normalize() else {
            return self;
        };
        let share = look_down_share(head) * bend_share(up);
        if share <= 0.0 {
            return self;
        }
        let unbend = Quat::IDENTITY.slerp(Quat::from_rotation_arc(up, Vec3::Y), share);
        if !unbend.is_finite() {
            return self;
        }
        Self {
            pouch_chest: Girdle {
                centre: hips + unbend * spine,
                basis: (unbend * self.chest).normalize(),
            },
            ..self
        }
    }

    /// The orientation of the chest the chest pouches ride.
    pub fn pouch_chest(&self) -> Quat {
        self.pouch_chest.basis
    }

    /// The length of the torso, from the middle of the hip joints to the middle of the shoulder
    /// joints.
    fn torso(&self) -> f32 {
        self.skeleton.chest().distance(self.skeleton.hip_centre())
    }

    fn shoulder_width(&self) -> f32 {
        self.skeleton.shoulders[0].distance(self.skeleton.shoulders[1])
    }
}

/// Move the back's sockets' volumes (the shoulders' and the outer back's) in `candidates` from where
/// a weapon would sit on `body` to where a hand putting one away over the shoulder closes: lower
/// and further out from the spine, by [`BACK_REACH_OFFSET_M`] in the chest's frame. A long gun on
/// the back sits with its grip up behind the head, and a hand reaching over the shoulder to put a
/// gun there closes below and outside that. Only the reach for docking moves: a draw is reached on
/// the weapon the player sees, a docked weapon stays where its socket puts it, and the attraction
/// travels a weapon to the socket's own place.
pub fn reach_back_sockets(candidates: &mut [Candidate], body: &BodyFrame) {
    let midline = body.skeleton.chest();
    let right = body.chest * Vec3::X;
    for candidate in candidates {
        if !matches!(
            candidate.volume,
            GrabVolume::Socket(Socket::Shoulder(_) | Socket::BackOuter)
        ) {
            continue;
        }
        let side = if (candidate.centre - midline).dot(right) < 0.0 {
            -1.0
        } else {
            1.0
        };
        let offset = Vec3::new(
            BACK_REACH_OFFSET_M.x * side,
            BACK_REACH_OFFSET_M.y,
            BACK_REACH_OFFSET_M.z,
        );
        let centre = candidate.centre + body.chest * offset;
        if centre.is_finite() {
            candidate.centre = centre;
        }
    }
}

/// Keep the draws off the back for the dominant hand while neither hand holds a weapon (`empty`):
/// the back's sockets and the long guns docked on them are offered to the dominant hand alone, so
/// a long gun drawn by reaching with the off-hand does not move the layout that follows the gun
/// hand. With a weapon in a hand, either hand may take one.
pub fn reserve_long_guns(candidates: &mut Vec<Candidate>, dominant: Hand, empty: bool) {
    if !empty {
        return;
    }
    candidates.retain_mut(|c| {
        if !matches!(
            c.volume,
            GrabVolume::Socket(Socket::Shoulder(_) | Socket::BackOuter)
        ) {
            return true;
        }
        match c.hand {
            None => {
                c.hand = Some(dominant);
                true
            }
            Some(hand) => hand == dominant,
        }
    });
}

/// The sides of the chest the two chest pouches take, by the body's own sides.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ChestSides {
    pub grenade: Hand,
    pub c4: Hand,
}

/// The chest's sides for a player whose dominant hand is `dominant`: the grenade pouch on the
/// dominant side and the C4 pouch on the other. Unlike the magazine pouch, they do not follow the gun
/// hand: with a pistol in each hand there is no single gun hand, and pouches that changed sides with
/// each handover would have to be found again each time.
pub fn chest_sides(dominant: Hand) -> ChestSides {
    ChestSides {
        grenade: dominant,
        c4: dominant.other(),
    }
}

/// The hand the magazine pouch is laid out around: the hand holding a gun, `holder`, or with none
/// drawn, the hand that last held one, `last_holder`, or with none yet, the dominant hand. A gun
/// may be in either hand whichever hand is dominant ([`crate::controllers::weapons::hold`]), and a
/// layout keyed off the dominant hand would then put the magazine pouch on the gun's side, out of
/// the free hand's reach. Remembering the last holder keeps the pouches from changing sides each
/// time a gun is docked.
pub fn gun_side(holder: Option<Hand>, last_holder: Option<Hand>, dominant: Hand) -> Hand {
    holder.or(last_holder).unwrap_or(dominant)
}

/// The hands that may take from the chest pouches.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ChestGrabbers {
    Either,
    Only(Hand),
    /// No hand is free: the pouches are not placed.
    Neither,
}

impl ChestGrabbers {
    /// The hands that may take from the chest pouches while `gun_hands` (by [`Hand::index`]) hold
    /// guns: either hand with no gun drawn, the free hand while one hand holds a gun, and neither
    /// while both do. A gun hand that took a grenade or a charge would prime it in the gun's hand.
    pub fn for_guns(gun_hands: [bool; 2]) -> Self {
        match gun_hands {
            [false, false] => ChestGrabbers::Either,
            [true, true] => ChestGrabbers::Neither,
            [true, false] => ChestGrabbers::Only(Hand::Right),
            [false, true] => ChestGrabbers::Only(Hand::Left),
        }
    }

    /// The candidate's hand, or `None` for a pouch that is not placed.
    fn hand(self) -> Option<Option<Hand>> {
        match self {
            ChestGrabbers::Either => Some(None),
            ChestGrabbers::Only(hand) => Some(Some(hand)),
            ChestGrabbers::Neither => None,
        }
    }
}

/// The grenade pouch on the front of the chest, a volume for `grabbers` (whichever grabs it
/// throws), or `None` while no hand may take from it.
pub fn grenade_pouch(
    frame: &BodyFrame,
    pouches: &PouchTuning,
    dominant: Hand,
    grabbers: ChestGrabbers,
) -> Option<Candidate> {
    let side = chest_sides(dominant).grenade;
    let pouch = chest_pouch(GrabVolume::GrenadePouch, frame, &pouches.grenade, side);
    grabbers.hand().map(|hand| Candidate { hand, ..pouch })
}

/// The C4 pouch on the front of the chest, on the side the grenade pouch leaves, a volume for
/// `grabbers` (whichever grabs it throws), or `None` while no hand may take from it.
pub fn c4_pouch(
    frame: &BodyFrame,
    pouches: &PouchTuning,
    dominant: Hand,
    grabbers: ChestGrabbers,
) -> Option<Candidate> {
    let side = chest_sides(dominant).c4;
    let pouch = chest_pouch(GrabVolume::C4Pouch, frame, &pouches.c4, side);
    grabbers.hand().map(|hand| Candidate { hand, ..pouch })
}

/// The universal reload pouch at the front of the belt, a volume for `grabber`, the hand without a
/// gun (see [`crate::controllers::weapons::reload`]), on that hand's side; with no hand free, on the
/// side away from the guns, `gun_side`. A gun's well pushed onto it reloads in one hand, so it is
/// placed whether or not a hand is free.
pub fn magazine_pouch(
    frame: &BodyFrame,
    pouches: &PouchTuning,
    gun_side: Hand,
    grabber: Option<Hand>,
) -> Candidate {
    let side = grabber.unwrap_or(gun_side.other());
    place(
        GrabVolume::MagazinePouch,
        belt(frame, side),
        frame.pelvis,
        &pouches.magazine,
        side,
        grabber,
    )
}

/// The standing height of the hip joints as a fraction of the standing height: a generic
/// anthropometric figure, which the body dimensions do not carry because it varies little between
/// adults.
#[cfg(test)]
pub const HIP_HEIGHT_FRACTION: f32 = 0.53;

/// The magazine pouch's volume: its centre a few centimetres out from the belly's surface.
const POUCH: VolumeSpec = VolumeSpec {
    offset_m: Vec3::new(0.0, 0.0, -0.04),
    radius_m: 0.075,
};

/// The chest pouches' volume: at the chest's surface and a little below the landmark, and wider than
/// the magazine pouch's, so that both a hand laid flat on the chest with the gaze level and a hand
/// reaching out in front of it close inside it. In the 2 November 2026 standing session a pouch
/// centred a few centimetres out from the surface missed hands laid on the chest by 4 to 10 cm,
/// which sat about 13 cm behind and 4 cm below its centre, while the takes came from hands pushed
/// out in front of the chest. Of that session's level-gaze presses at the chest, this centre and
/// radius take 22 of 48, against 18 for the old pouch and 17 for one moved the whole 13 cm back.
/// The pouch models sit partly inside the character's chest until his chest is aligned with the
/// player's.
const CHEST_POUCH: VolumeSpec = VolumeSpec {
    offset_m: Vec3::new(0.0, -0.02, 0.04),
    radius_m: 0.09,
};

/// The C4 pouch's volume: placed as the grenade pouch is, and wider still. The charge is taken with
/// the hand reaching across the body to the far side of the chest, which lands further off the
/// pouch than a reach to the near side.
const C4_POUCH: VolumeSpec = VolumeSpec {
    radius_m: 0.1,
    ..CHEST_POUCH
};

/// The chest of a frame: its middle and its orientation, as [`BodyFrame::chest`].
#[derive(Debug, Copy, Clone, PartialEq)]
struct Girdle {
    centre: Vec3,
    basis: Quat,
}

/// How far the back's sockets' reach is moved from where their weapons sit, in the chest's frame
/// (+X right, mirrored for the left side; +Y up the spine; +Z back), metres: down by about a hand's
/// width and out from the spine. In the same session the releases over the shoulder that dropped
/// the weapon came lower and further out than the place; the draws, which closed at eye height
/// behind the head on the weapon, are left there.
const BACK_REACH_OFFSET_M: Vec3 = Vec3::new(0.05, -0.12, 0.0);

/// The share of the look-down bend the chest pouches are kept off for a head turned `head`: none
/// for a look down of up to [`LOOK_DOWN_START`], all of it from [`LOOK_DOWN_FULL`], eased between.
fn look_down_share(head: Quat) -> f32 {
    let down = (-(head * Vec3::NEG_Z).y).clamp(-1.0, 1.0).asin();
    smoothstep_between(LOOK_DOWN_START, LOOK_DOWN_FULL, down)
}

/// The share of the bend the chest pouches are kept off for a spine running along `up`: all of it
/// within [`BEND_FULL`] of upright, which covers the bend a look down puts in the body, none past
/// [`BEND_NONE`], where the body is lying or folded by more than a look explains.
fn bend_share(up: Vec3) -> f32 {
    let bend = up.y.clamp(-1.0, 1.0).acos();
    1.0 - smoothstep_between(BEND_FULL, BEND_NONE, bend)
}

/// The look down below which the chest pouches ride the chest as it is, radians: about ten degrees.
const LOOK_DOWN_START: f32 = 0.17;

/// The look down from which the chest pouches are kept off the whole bend, radians: about forty
/// degrees.
const LOOK_DOWN_FULL: f32 = 0.7;

/// The spine's tilt from upright within which a look down's bend is undone, radians: about thirty
/// degrees.
const BEND_FULL: f32 = 0.52;

/// The spine's tilt from upright past which nothing is undone, radians: about fifty degrees.
const BEND_NONE: f32 = 0.87;

/// How far above the hip joints the belt's front is, where the magazine pouch hangs, as a fraction
/// of the torso: the waist.
const BELT_RISE_FRACTION: f32 = 0.17;

/// How far out from the midline the belt's front landmark sits, as a fraction of the shoulder
/// width: toward the middle of the belt, well inside the hip socket on that side, so that a hand
/// feeling for the pouch does not land in the socket.
const BELT_OUT_FRACTION: f32 = 0.12;

/// How far in front of the hip joints' line the front of the belly is, metres.
const BELLY_FRONT_M: f32 = 0.03 + body::EYES_FORWARD_OF_SHOULDERS;

/// How far below the shoulder joints the chest pouches' landmark sits, as a fraction of the torso:
/// the upper chest.
const CHEST_POUCH_DROP_FRACTION: f32 = 0.22;

/// How far out from the midline the chest pouches' landmark sits, as a fraction of the shoulder
/// width: about under the shoulder joint, clear of the chest socket in the middle.
const CHEST_POUCH_OUT_FRACTION: f32 = 0.52;

/// How far in front of the shoulder joints' line the front of the chest is, metres: a little deeper
/// than the belly.
const CHEST_FRONT_M: f32 = 0.02 + body::EYES_FORWARD_OF_SHOULDERS;

/// The belt's front landmark on `side`: on the belly's surface at the belt's height, at the front
/// of the hip on that side.
fn belt(frame: &BodyFrame, side: Hand) -> Vec3 {
    let out = frame.shoulder_width() * BELT_OUT_FRACTION * side_sign(side);
    let rise = frame.torso() * BELT_RISE_FRACTION;
    frame.skeleton.hip_centre() + frame.pelvis * Vec3::new(out, rise, -BELLY_FRONT_M)
}

/// The chest pouch `volume` on `side`: on the chest's surface below and inside the shoulder joint,
/// riding the pouches' chest ([`BodyFrame::looking`]).
fn chest_pouch(volume: GrabVolume, frame: &BodyFrame, spec: &VolumeSpec, side: Hand) -> Candidate {
    let out = frame.shoulder_width() * CHEST_POUCH_OUT_FRACTION * side_sign(side);
    let chest = frame.pouch_chest;
    let landmark = chest.centre
        + chest.basis
            * Vec3::new(
                out,
                -frame.torso() * CHEST_POUCH_DROP_FRACTION,
                -CHEST_FRONT_M,
            );
    place(volume, landmark, chest.basis, spec, side, None)
}

/// The orientation of a girdle through `[left, right]` on a body whose spine runs along `spine`:
/// +X along the line from left to right, +Y up the spine (square to the line), and -Z forward.
/// `None` for a degenerate line or spine, or one along the other.
fn girdle_basis([left, right]: [Vec3; 2], spine: Vec3) -> Option<Quat> {
    let right_axis = (right - left).try_normalize()?;
    let up = spine.reject_from_normalized(right_axis).try_normalize()?;
    let back = right_axis.cross(up);
    let basis = Quat::from_mat3(&Mat3::from_cols(right_axis, up, back)).normalize();
    basis.is_finite().then_some(basis)
}

/// A volume at `spec`'s offset from `landmark` in the frame `basis`, mirrored for the left side.
fn place(
    volume: GrabVolume,
    landmark: Vec3,
    basis: Quat,
    spec: &VolumeSpec,
    side: Hand,
    hand: Option<Hand>,
) -> Candidate {
    let offset = Vec3::new(
        spec.offset_m.x * side_sign(side),
        spec.offset_m.y,
        spec.offset_m.z,
    );
    Candidate {
        volume,
        centre: landmark + basis * offset,
        radius: spec.radius_m.max(0.0),
        hand,
    }
}

fn side_sign(side: Hand) -> f32 {
    match side {
        Hand::Left => -1.0,
        Hand::Right => 1.0,
    }
}

#[cfg(test)]
mod tests;
