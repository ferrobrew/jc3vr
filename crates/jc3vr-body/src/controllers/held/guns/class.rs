//! The weight class a weapon's tuning comes from: the weapon handling's weight of a long gun tunes
//! each class, and the hand bodies take a held gun's fallback mass and limits by it.

use jc3gi::equipment::weapon::WeaponClass;

/// The weight class a weapon's one-handed tuning comes from.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HeftClass {
    /// Pistols and revolvers.
    Sidearm,
    /// SMGs: a sidearm in one hand, the lightest of the long guns braced.
    Smg,
    /// Assault rifles, shotguns, and sniper rifles.
    Rifle,
    /// Launchers, miniguns, mortars, mounted-calibre guns, and the special weapons.
    Heavy,
}

impl HeftClass {
    pub const ALL: [HeftClass; 4] = [
        HeftClass::Sidearm,
        HeftClass::Smg,
        HeftClass::Rifle,
        HeftClass::Heavy,
    ];

    /// The class's place in [`HeftClass::ALL`].
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The class of a weapon of class `class` (the raw [`WeaponClass`] value, which may be out of
    /// the enum's range), held in a two-handed slot or not. A weapon whose class is unset or
    /// unknown is a rifle in a two-handed slot and a sidearm otherwise.
    pub fn of(class: i32, two_handed: bool) -> Self {
        const SIDEARMS: [WeaponClass; 2] = [
            WeaponClass::E_WEAPON_CLASS_PISTOL,
            WeaponClass::E_WEAPON_CLASS_REVOLVER,
        ];
        const RIFLES: [WeaponClass; 3] = [
            WeaponClass::E_WEAPON_CLASS_ASSAULT_RIFLE,
            WeaponClass::E_WEAPON_CLASS_SHOTGUN,
            WeaponClass::E_WEAPON_CLASS_SNIPER,
        ];
        const HEAVY: [WeaponClass; 7] = [
            WeaponClass::E_WEAPON_CLASS_GRENADE_LAUNCHER,
            WeaponClass::E_WEAPON_CLASS_RPG,
            WeaponClass::E_WEAPON_CLASS_MINIGUN,
            WeaponClass::E_WEAPON_CLASS_AA_GUN,
            WeaponClass::E_WEAPON_CLASS_MORTAR,
            WeaponClass::E_WEAPON_CLASS_50CAL,
            WeaponClass::E_WEAPON_CLASS_SPECIAL,
        ];
        let is = |set: &[WeaponClass]| set.iter().any(|&c| c as i32 == class);
        if is(&SIDEARMS) {
            HeftClass::Sidearm
        } else if class == WeaponClass::E_WEAPON_CLASS_SMG as i32 {
            HeftClass::Smg
        } else if is(&RIFLES) {
            HeftClass::Rifle
        } else if is(&HEAVY) {
            HeftClass::Heavy
        } else if two_handed {
            HeftClass::Rifle
        } else {
            HeftClass::Sidearm
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            HeftClass::Sidearm => "sidearm",
            HeftClass::Smg => "SMG",
            HeftClass::Rifle => "rifle",
            HeftClass::Heavy => "heavy",
        }
    }
}
