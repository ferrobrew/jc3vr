//! The input tick's side of a revolver's cylinder ([`crate::controllers::weapons::reload::revolver`]):
//! each gun hand's cylinder stepped on the gun's measured frame, its dump, and what the skeleton
//! and the well read of it.

use glam::{Mat4, Quat};
use jc3gi::equipment::weapon::WeaponData;

use crate::controllers::{
    frame::Hand,
    haptics::{self, Event},
    weapons::reload::{
        GunTick, State, Tick, WeaponTick, casings,
        engine::{dump_cylinder, gun_address, gun_has_bone, gun_world},
        plan::{LoadKind, ReloadPlan, WellFrame},
        render,
        revolver::{Cylinder, CylinderEvent, CylinderInputs, REVOLVER},
    },
};

/// A revolver's cylinder as the tick reads it.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct CylinderTick {
    /// Swung all the way out.
    pub out: bool,
    /// Shut, so the gun may fire.
    pub shut: bool,
    /// Out with every chamber empty, ready for a speedloader.
    pub takes_speedloader: bool,
}

impl State {
    /// The cylinder of `hand`'s gun, a revolver whose skeleton carries the crane: kept while the
    /// hand holds the same weapon, made afresh (shut) for another.
    ///
    /// # Safety
    ///
    /// The weapon must be owned and live on the input thread, outside the world update.
    pub(super) unsafe fn cylinder_for(
        &mut self,
        hand: Hand,
        plan: &ReloadPlan,
        weapon: usize,
        gun: usize,
    ) -> Option<Cylinder> {
        let i = hand.index();
        let kept = self.cylinders[i].filter(|c| c.weapon == weapon && c.hand == hand);
        let cylinder = match kept {
            Some(cylinder) => Some(cylinder),
            None if plan.load == LoadKind::Cylinder
                && unsafe { gun_has_bone(weapon, gun, WeaponData::CHAMBER_BONE_HASH) } =>
            {
                Some(Cylinder::new(weapon, hand))
            }
            None => None,
        };
        self.cylinders[i] = cylinder;
        cylinder
    }

    /// Step each gun hand's cylinder: the flicks that swing it, and the dump with the muzzle up.
    pub(super) fn cylinders(&mut self, tick: &Tick, dt: f32) {
        // SAFETY: the input tick's steps run on the game thread before the world update.
        unsafe { casings::tick() };
        for hand in Hand::BOTH {
            let i = hand.index();
            let target = tick.gun(hand).filter(|(weapon, _)| {
                self.cylinders[i].is_some_and(|c| c.weapon == weapon.address)
            });
            let (Some((weapon, gun)), Some(mut cylinder)) = (target, self.cylinders[i]) else {
                self.cylinders[i] = None;
                continue;
            };
            let event = cylinder.step(
                CylinderInputs {
                    orientation: gun.frame.map(|f| f.pose.orientation),
                    dt,
                },
                &REVOLVER,
            );
            self.cylinders[i] = Some(cylinder);
            match event {
                Some(CylinderEvent::Opened) => {
                    self.event(format!("the {hand:?} revolver's cylinder swung out"));
                    haptics::play(hand, Event::ActionDetent);
                }
                Some(CylinderEvent::Closed) => {
                    self.event(format!("the {hand:?} revolver's cylinder swung shut"));
                    haptics::play(hand, Event::ActionDetent);
                }
                Some(CylinderEvent::Dumped) => self.dump(weapon, gun, hand, &cylinder),
                None => {}
            }
        }
    }

    /// Dump the cylinder of `hand`'s revolver: its rounds go back to the reserve and every chamber's
    /// casing falls out of the swung-out cylinder.
    fn dump(&mut self, weapon: &WeaponTick, gun: &GunTick, hand: Hand, cylinder: &Cylinder) {
        // SAFETY (each engine call): the input tick's steps run on the game thread before the
        // world update, with the weapon read this tick.
        let rounds = unsafe { dump_cylinder(weapon.address) };
        let dropped = match (weapon.plan.round, weapon.plan.well, unsafe {
            gun_world(weapon.address, gun.index)
        }) {
            (Some(round), WellFrame::Gun { mouth, axis }, Some(world)) => {
                let face = cylinder.carried(mouth);
                let out = world.transform_vector3(-axis);
                let lie = Quat::from_rotation_arc(round.axis, axis);
                let casings: Vec<_> = round
                    .offsets()
                    .map(|offset| casings::Dumped {
                        matrix: world * Mat4::from_rotation_translation(lie, face + lie * offset),
                        out,
                    })
                    .collect();
                // SAFETY: as above.
                unsafe { casings::drop(round.name_hash, &casings) }
            }
            _ => 0,
        };
        self.event(format!(
            "the {hand:?} revolver's cylinder was dumped: {} rounds back to the reserve, {dropped} casings dropped",
            rounds.unwrap_or(0)
        ));
        haptics::play(hand, Event::Eject);
    }

    /// Whether the weapon at `weapon` is a revolver whose cylinder is not shut.
    pub(super) fn cylinder_out(&self, weapon: usize) -> bool {
        self.cylinders
            .iter()
            .flatten()
            .any(|c| c.weapon == weapon && !c.is_shut())
    }

    /// A speedloader or a belt push loaded `hand`'s revolver.
    pub(super) fn cylinder_loaded(&mut self, hand: Hand) {
        if let Some(cylinder) = self.cylinders[hand.index()].as_mut() {
            cylinder.loaded();
        }
    }

    /// The cylinder of `hand`'s gun as the skeleton draws it.
    pub(super) fn cylinder_part(&self, tick: &Tick, hand: Hand) -> Option<render::CylinderPart> {
        let cylinder = self.cylinders[hand.index()]?;
        let (weapon, gun) = tick.gun(hand)?;
        if weapon.address != cylinder.weapon {
            return None;
        }
        // SAFETY: the input tick's steps run on the game thread before the world update.
        let gun_address = unsafe { gun_address(weapon.address, gun.index) }?;
        Some(render::CylinderPart {
            weapon: weapon.address,
            gun_address,
            crane_rad: cylinder.crane_rad(),
            crane_shift_m: cylinder.crane_shift_m(),
            spin_rad: cylinder.spin_rad(&REVOLVER),
        })
    }
}

/// The tick's reading of `cylinder`.
pub(super) fn reading(cylinder: &Cylinder) -> CylinderTick {
    CylinderTick {
        out: cylinder.is_out(),
        shut: cylinder.is_shut(),
        takes_speedloader: cylinder.takes_speedloader(),
    }
}

/// The well a revolver's speedloader goes into: the cylinder's rear face, carried out on the crane
/// as far as the cylinder has swung.
pub(super) fn well(frame: WellFrame, cylinder: Option<&Cylinder>) -> WellFrame {
    match (frame, cylinder) {
        (WellFrame::Gun { mouth, axis }, Some(cylinder)) => WellFrame::Gun {
            mouth: cylinder.carried(mouth),
            axis,
        },
        _ => frame,
    }
}
