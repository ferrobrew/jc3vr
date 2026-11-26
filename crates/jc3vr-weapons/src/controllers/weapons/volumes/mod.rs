//! Placing the body's grab volumes: the grenade pouch, the magazine pouch, and the C4 pouch, beside
//! the sockets' reach ([`SocketTuning`]; a socket's volume sits where the socket's weapon is
//! placed). Pure. The placement's tuning is [`VolumesTuning`], in the hold tuning file.
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
//! - the belt's front, for the magazine pouch: above the hip joints, in front of the belly, on the
//!   midline with one gun or none, and a few centimetres to each side with a gun in each hand.
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

mod stride;

use body_rig::math::{shoulders::EYES_FORWARD_OF_SHOULDERS, smooth::smoothstep_between};
use glam::{Mat3, Quat, Vec3};
use jc3vr_body::controllers::{
    body::rig::frames::SkeletonLandmarks,
    grab::{Candidate, GrabVolume},
};
use jc3vr_core::config::engine::{Invalid, positive, range, vec3};
use jc3vr_input::frame::Hand;
use serde::Deserialize;
pub(crate) use stride::StrideFilter;
pub use stride::StrideTuning;

use crate::controllers::{loadout::Socket, weapons::hold::tuning::HOLD};

/// Where the body's grab volumes go: the sockets' reach, the pouches' volumes, and the landmarks of
/// the skeleton they ride. Lengths that scale with the body are fractions of the skeleton's own
/// torso or shoulder width.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VolumesTuning {
    pub sockets: SocketTuning,
    pub pouches: PouchTuning,
    /// How far the back's sockets' reach is moved from where their weapons sit, in the chest's
    /// frame (+X right, mirrored for the left side; +Y up the spine; +Z back), metres: down by about
    /// a hand's width and out from the spine. In the 10 November 17:07 session the releases over
    /// the shoulder that dropped the weapon came lower and further out than the place; the draws,
    /// which closed at eye height behind the head on the weapon, are left there
    /// ([`reach_back_sockets`]).
    pub back_reach_offset_m: Vec3,
    /// The look down below which the chest pouches ride the chest as it is, radians: about ten
    /// degrees ([`BodyFrame::looking`]).
    pub look_down_start_rad: f32,
    /// The look down from which the chest pouches are kept off the whole bend, radians: about forty
    /// degrees.
    pub look_down_full_rad: f32,
    /// The spine's tilt from upright within which a look down's bend is undone, radians: about
    /// thirty degrees.
    pub bend_full_rad: f32,
    /// The spine's tilt from upright past which nothing is undone, radians: about fifty degrees.
    pub bend_none_rad: f32,
    /// How far above the hip joints the belt's front is, where the magazine pouch hangs, as a
    /// fraction of the torso: the waist.
    pub belt_rise_fraction: f32,
    /// How far out from the midline the belt's front landmark on a side sits, as a fraction of the
    /// shoulder width: toward the middle of the belt, well inside the hip socket on that side, so
    /// that a hand feeling for the pouch does not land in the socket.
    pub belt_out_fraction: f32,
    /// How far in front of the hip joints' line the front of the belly is, metres, beyond the eyes'
    /// own offset in front of the shoulders ([`EYES_FORWARD_OF_SHOULDERS`]).
    pub belly_front_m: f32,
    /// How far below the shoulder joints the chest pouches' landmark sits, as a fraction of the
    /// torso: the upper chest.
    pub chest_pouch_drop_fraction: f32,
    /// How far out from the midline the chest pouches' landmark sits, as a fraction of the shoulder
    /// width: about under the shoulder joint.
    pub chest_pouch_out_fraction: f32,
    /// How far in front of the shoulder joints' line the front of the chest is, metres, beyond the
    /// eyes' own offset in front of the shoulders: a little deeper than the belly.
    pub chest_front_m: f32,
    pub stride: StrideTuning,
}

impl VolumesTuning {
    pub(crate) fn validate(&self) -> Result<(), Invalid> {
        positive(
            "volumes.sockets.hip_radius_m",
            self.sockets.hip_radius_m,
            1.0,
        )?;
        positive(
            "volumes.sockets.shoulder_radius_m",
            self.sockets.shoulder_radius_m,
            1.0,
        )?;
        self.pouches.grenade.validate("volumes.pouches.grenade")?;
        self.pouches.c4.validate("volumes.pouches.c4")?;
        self.pouches.magazine.validate("volumes.pouches.magazine")?;
        range(
            "volumes.pouches.reach_margin_m",
            self.pouches.reach_margin_m,
            0.0,
            0.5,
        )?;
        vec3("volumes.back_reach_offset_m", self.back_reach_offset_m, 1.0)?;
        let half_turn = std::f32::consts::PI;
        range(
            "volumes.look_down_start_rad",
            self.look_down_start_rad,
            0.0,
            half_turn,
        )?;
        range(
            "volumes.look_down_full_rad",
            self.look_down_full_rad,
            self.look_down_start_rad + f32::EPSILON,
            half_turn,
        )?;
        range("volumes.bend_full_rad", self.bend_full_rad, 0.0, half_turn)?;
        range(
            "volumes.bend_none_rad",
            self.bend_none_rad,
            self.bend_full_rad + f32::EPSILON,
            half_turn,
        )?;
        range(
            "volumes.belt_rise_fraction",
            self.belt_rise_fraction,
            -1.0,
            1.0,
        )?;
        range(
            "volumes.belt_out_fraction",
            self.belt_out_fraction,
            0.0,
            1.0,
        )?;
        range("volumes.belly_front_m", self.belly_front_m, -0.5, 0.5)?;
        range(
            "volumes.chest_pouch_drop_fraction",
            self.chest_pouch_drop_fraction,
            -1.0,
            1.0,
        )?;
        range(
            "volumes.chest_pouch_out_fraction",
            self.chest_pouch_out_fraction,
            0.0,
            1.0,
        )?;
        range("volumes.chest_front_m", self.chest_front_m, -0.5, 0.5)?;
        self.stride.validate()
    }
}

/// The body's sockets: how near a hand must come to each, metres. A socket's grab volume is centred
/// where the socket places its weapon, so the socket has no placement of its own here.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SocketTuning {
    /// The sockets on both hips. A hand reaching for the pistol closes on the front of the hip
    /// more often than on the pistol's grip behind it, and in the 10 November 17:07 session five
    /// draws missed a reach of 0.18 m by 1 to 4 cm.
    pub hip_radius_m: f32,
    /// The sockets over the shoulders, the right one where the game docks the two-handed slot and
    /// the left one where it docks the special slot.
    pub shoulder_radius_m: f32,
}

/// The pouches on the front of the body: the magazine pouch at the front of the belt, and the
/// grenade and C4 pouches on either side of the chest. Each is placed only while its feature is on:
/// the grenade pouch with the grenade throw, the C4 pouch with the C4 throw, and the magazine pouch
/// while the true reload reloads the held weapon. Offsets are given for the right side and mirrored
/// for the left.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PouchTuning {
    /// The grenade pouch, offset from the chest's front on its side: at the chest's surface and a
    /// little below the landmark, and wider than the magazine pouch, so that both a hand laid flat
    /// on the chest with the gaze level and a hand reaching out in front of it close inside it. In
    /// the 2 November 2026 standing session a pouch centred a few centimetres out from the surface
    /// missed hands laid on the chest by 4 to 10 cm, which sat about 13 cm behind and 4 cm below
    /// its centre, while the takes came from hands pushed out in front of the chest. Of that
    /// session's level-gaze presses at the chest, the shipped centre and radius take 22 of 48,
    /// against 18 for the old pouch and 17 for one moved the whole 13 cm back. The pouch models sit
    /// partly inside the character's chest until his chest is aligned with the player's.
    pub grenade: VolumeSpec,
    /// The C4 pouch, offset from the chest's front on its side: as the grenade pouch, but wider and
    /// nearer the midline. The charge is taken with the hand reaching across the body to the far
    /// side of the chest, and that hand stops short of the far shoulder: in the 10 November 17:07
    /// session every take and near miss closed 11 to 17 cm toward the midline from a pouch under
    /// the shoulder joint, so that all seven takes came from the reach margin and five presses
    /// missed it by up to 3 cm. Moved 10 cm in, the pouch takes eleven of those twelve presses
    /// inside its own radius and the last within its margin.
    pub c4: VolumeSpec,
    /// The universal reload pouch, offset from the belt's front, at its middle or on one side
    /// ([`centre_magazine_pouch`], [`magazine_pouch`]): its centre a few centimetres out from the
    /// belly's surface. It hands out what the held weapon takes: a magazine, a shell, a rocket, or
    /// a pod.
    pub magazine: VolumeSpec,
    /// How far outside a pouch's surface a grip press still takes from it, metres, when the hand
    /// is in no other volume: a hand feeling for a pouch lands a little off it, and a press there
    /// must not fire the grapple instead.
    pub reach_margin_m: f32,
}

/// A spherical grab volume placed relative to a body landmark.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VolumeSpec {
    /// The centre's offset from the landmark in the body frame (+X right, +Y up, +Z back), metres,
    /// given for the right side and mirrored across the body's midline for the left.
    pub offset_m: Vec3,
    pub radius_m: f32,
}

impl VolumeSpec {
    fn validate(&self, path: &str) -> Result<(), Invalid> {
        vec3(&format!("{path}.offset_m"), self.offset_m, 1.0)?;
        positive(&format!("{path}.radius_m"), self.radius_m, 1.0)
    }
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

/// Move the shoulders' socket volumes in `candidates` from where a weapon would sit on `body` to
/// where a hand putting one away over the shoulder closes: lower and further out from the spine, by
/// [`VolumesTuning::back_reach_offset_m`] in the chest's frame. A long gun on the back sits with
/// its grip up behind the head, and a hand reaching over the shoulder to put a gun there closes
/// below and outside that. Only the reach for docking moves: a draw is reached on
/// the weapon the player sees, a docked weapon stays where its socket puts it, and the attraction
/// travels a weapon to the socket's own place.
pub fn reach_back_sockets(candidates: &mut [Candidate], body: &BodyFrame) {
    let back_reach = HOLD.get().volumes.back_reach_offset_m;
    let midline = body.skeleton.chest();
    let right = body.chest * Vec3::X;
    for candidate in candidates {
        if !matches!(candidate.volume, GrabVolume::Socket(Socket::Shoulder(_))) {
            continue;
        }
        let side = if (candidate.centre - midline).dot(right) < 0.0 {
            -1.0
        } else {
            1.0
        };
        let offset = Vec3::new(back_reach.x * side, back_reach.y, back_reach.z);
        let centre = candidate.centre + body.chest * offset;
        if centre.is_finite() {
            candidate.centre = centre;
        }
    }
}

/// Give the back's unmarked sockets, and the long guns docked on them, a hand while neither hand
/// holds a weapon (`empty`): an offer with no hand of its own goes to the dominant hand, and an
/// offer the socket already names stays with that hand. With a weapon in a hand, the offers are
/// left as they are.
pub fn reserve_long_guns(candidates: &mut Vec<Candidate>, dominant: Hand, empty: bool) {
    if !empty {
        return;
    }
    candidates.retain_mut(|c| {
        if !matches!(c.volume, GrabVolume::Socket(Socket::Shoulder(_))) {
            return true;
        }
        match c.hand {
            None => {
                c.hand = Some(dominant);
                true
            }
            Some(_) => true,
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

/// The universal reload pouch at the middle of the belt's front, a volume for `grabber`: where the
/// reload ([`crate::controllers::weapons::reload`]) puts it with one gun or none, so that the free
/// hand's take and the gun hand's push onto it ([`magazine_pouch`]'s one-handed reload) both reach
/// it without crossing the body.
pub fn centre_magazine_pouch(
    frame: &BodyFrame,
    pouches: &PouchTuning,
    grabber: Option<Hand>,
) -> Candidate {
    let landmark = belt_at(frame, 0.0);
    // The offset is mirrored for the left side, so a pouch spec with a sideways offset would leave
    // the midline; the shipped one has none.
    place(
        GrabVolume::MagazinePouch,
        landmark,
        frame.pelvis,
        &VolumeSpec {
            offset_m: pouches.magazine.offset_m * Vec3::new(0.0, 1.0, 1.0),
            ..pouches.magazine
        },
        Hand::Right,
        grabber,
    )
}

/// The universal reload pouch at the front of the belt on `side` of the body, a volume for
/// `grabber`: with a gun in each hand, the reload places one on each gun's own side, so neither gun
/// is pushed across the body onto the one pouch. A gun's well pushed onto it reloads in one hand,
/// so it is placed whether or not a hand is free.
pub fn magazine_pouch(
    frame: &BodyFrame,
    pouches: &PouchTuning,
    side: Hand,
    grabber: Option<Hand>,
) -> Candidate {
    place(
        GrabVolume::MagazinePouch,
        belt(frame, side),
        frame.pelvis,
        &pouches.magazine,
        side,
        grabber,
    )
}

/// The chest of a frame: its middle and its orientation, as [`BodyFrame::chest`].
#[derive(Debug, Copy, Clone, PartialEq)]
struct Girdle {
    centre: Vec3,
    basis: Quat,
}

/// The share of the look-down bend the chest pouches are kept off for a head turned `head`: none
/// for a look down of up to [`VolumesTuning::look_down_start_rad`], all of it from
/// [`VolumesTuning::look_down_full_rad`], eased between.
fn look_down_share(head: Quat) -> f32 {
    let tuning = &HOLD.get().volumes;
    let down = (-(head * Vec3::NEG_Z).y).clamp(-1.0, 1.0).asin();
    smoothstep_between(tuning.look_down_start_rad, tuning.look_down_full_rad, down)
}

/// The share of the bend the chest pouches are kept off for a spine running along `up`: all of it
/// within [`VolumesTuning::bend_full_rad`] of upright, which covers the bend a look down puts in
/// the body, none past [`VolumesTuning::bend_none_rad`], where the body is lying or folded by more
/// than a look explains.
fn bend_share(up: Vec3) -> f32 {
    let tuning = &HOLD.get().volumes;
    let bend = up.y.clamp(-1.0, 1.0).acos();
    1.0 - smoothstep_between(tuning.bend_full_rad, tuning.bend_none_rad, bend)
}

/// The belt's front landmark on `side`: on the belly's surface at the belt's height, at the front
/// of the hip on that side.
fn belt(frame: &BodyFrame, side: Hand) -> Vec3 {
    belt_at(
        frame,
        frame.shoulder_width() * HOLD.get().volumes.belt_out_fraction * side_sign(side),
    )
}

/// The point on the belly's surface at the belt's height, `out` metres to the right of the midline.
fn belt_at(frame: &BodyFrame, out: f32) -> Vec3 {
    let tuning = &HOLD.get().volumes;
    let rise = frame.torso() * tuning.belt_rise_fraction;
    let belly_front = tuning.belly_front_m + EYES_FORWARD_OF_SHOULDERS;
    frame.skeleton.hip_centre() + frame.pelvis * Vec3::new(out, rise, -belly_front)
}

/// The chest pouch `volume` on `side`: on the chest's surface below and inside the shoulder joint,
/// riding the pouches' chest ([`BodyFrame::looking`]).
fn chest_pouch(volume: GrabVolume, frame: &BodyFrame, spec: &VolumeSpec, side: Hand) -> Candidate {
    let tuning = &HOLD.get().volumes;
    let out = frame.shoulder_width() * tuning.chest_pouch_out_fraction * side_sign(side);
    let chest = frame.pouch_chest;
    let landmark = chest.centre
        + chest.basis
            * Vec3::new(
                out,
                -frame.torso() * tuning.chest_pouch_drop_fraction,
                -(tuning.chest_front_m + EYES_FORWARD_OF_SHOULDERS),
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
