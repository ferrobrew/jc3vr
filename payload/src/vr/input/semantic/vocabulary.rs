//! The semantic action vocabulary: one enum per action set, each action's OpenXR name, its type,
//! and the localized name the runtime's binding interface shows.
//!
//! An action's name is part of the player's saved bindings, so a name never changes meaning: a
//! rename is a removal plus an addition (see [`crate::vr::input::semantic::SCHEMA_VERSION`]). The
//! localized names are English sentence case and mention the overloads the game or the mod resolves
//! within the one action, such as a tap and a hold.

use crate::vr::input::semantic::{ActionDef, ActionKind, SemanticSet};

/// One semantic action of any set: the set, and the action's index in the set's `ALL` table. The
/// binding tables name actions through their set's enum and convert them to this.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ActionId {
    pub(crate) set: SemanticSet,
    pub(crate) index: u8,
}

impl ActionId {
    /// The action's index, for the per-action tables.
    pub(crate) const fn index(self) -> usize {
        self.index as usize
    }
}

/// Declare one set's action enum, its `ALL` table, and its `DEFS` table, from one list so that the
/// two tables cannot disagree. A variant's discriminant is its index in both.
macro_rules! semantic_actions {
    (
        $(#[$meta:meta])*
        $name:ident in $set:ident {
            $(
                $(#[$vmeta:meta])*
                $variant:ident => ($action:literal, $kind:ident, $localized:literal),
            )+
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
        pub(crate) enum $name {
            $( $(#[$vmeta])* $variant, )+
        }

        impl $name {
            /// Every action, in creation and stash order.
            pub(crate) const ALL: &'static [$name] = &[$($name::$variant,)+];

            /// The name, type, and localized name of every action, indexed like
            /// [`ALL`](Self::ALL).
            pub(crate) const DEFS: &'static [ActionDef] = &[$(
                ActionDef {
                    name: $action,
                    kind: ActionKind::$kind,
                    localized_name: $localized,
                },
            )+];

            /// The action as an [`ActionId`].
            pub(crate) const fn id(self) -> ActionId {
                ActionId {
                    set: SemanticSet::$set,
                    index: self as u8,
                }
            }
        }

        impl From<$name> for ActionId {
            fn from(action: $name) -> ActionId {
                action.id()
            }
        }
    };
}

semantic_actions! {
    /// The `global` set, active in every mode.
    GlobalAction in Global {
        /// A tap pauses, a hold opens the commlink; the mod tells the two apart.
        Pause => ("pause", Bool, "Pause (hold: commlink)"),
        OpenCommlink => ("open_commlink", Bool, "Open the commlink"),
        /// The stick-click chord is the built-in way to recenter; this action is unbound by
        /// default.
        RecenterView => ("recenter_view", Bool, "Recenter the view"),
        /// The stick-click and B chord is the built-in way; this action is unbound by default.
        ToggleDebugPanel => ("toggle_debug_panel", Bool, "Show or hide the mod's debug panel"),
        ShowControlsSummary => ("show_controls_summary", Bool, "Show the controls summary"),
    }
}

semantic_actions! {
    /// The `on_foot` set: on foot, swimming, falling, and under the parachute.
    OnFootAction in OnFoot {
        /// Camera-relative on foot, in the stick's own frame while airborne.
        Move => ("move", Vec2, "Move"),
        Turn => ("turn", Float, "Turn"),
        CrouchFlick => ("crouch_flick", Float, "Crouch (flick down, seated)"),
        Fire => ("fire", Float, "Fire"),
        /// Read only while dual-wielding.
        FireOffHandPistol => ("fire_off_hand_pistol", Float, "Fire the off-hand pistol (dual-wield)"),
        /// Read only when not dual-wielding.
        RetractTethers => ("retract_tethers", Float, "Retract tethers"),
        /// Read only while dual-wielding, when the off-hand trigger fires its pistol.
        RetractTethersAlt => ("retract_tethers_alt", Bool, "Retract tethers (while dual-wielding)"),
        /// The grapple on an analog input, for a player who binds it to one; unbound by default, in
        /// favour of [`OnFootAction::Grapple`].
        FireGrapple => ("fire_grapple", Float, "Grapple, on an analog input (hold: tether)"),
        Jump => ("jump", Bool, "Jump (in the air: open the parachute)"),
        Use => ("use", Bool, "Use or enter a vehicle (hold: gunner seat)"),
        Reload => ("reload", Bool, "Reload (hold: holster)"),
        /// Unbound by default: a double tap of the grapple releases the tethers.
        ReleaseTethers => ("release_tethers", Bool, "Release tethers"),
        /// Detonates the placed charges once held past the game's hold time; the charges come
        /// from their pouch.
        Detonate => ("detonate", Bool, "Detonate explosives (hold)"),
        /// Read when weapon cycling is on and not dual-wielding.
        NextWeapon => ("next_weapon", Bool, "Next weapon"),
        /// The fallback for the throw gesture; unbound by default.
        ThrowGrenade => ("throw_grenade", Bool, "Throw a grenade"),
        WeaponSelectPad => ("weapon_select_pad", Vec2, "Weapon select direction"),
        /// Opens the weapon wheel too, by default; a tap in a quadrant selects that slot.
        WeaponSelectPress => ("weapon_select_press", Float, "Weapon select press (hold: weapon wheel)"),
        /// Held past its open threshold, opens the weapon wheel; the hand that holds it chooses.
        /// By default the dominant stick pushed up, the half of its vertical axis that
        /// `crouch_flick` leaves unread.
        WeaponWheel => ("weapon_wheel", Float, "Weapon wheel (hold, point to choose)"),
        /// With tethers attached, a double tap releases them and a hold with nothing new under the
        /// aim tightens them; the mod reads the gestures.
        Grapple => ("grapple", Bool, "Grapple (hold: tether or tighten; double tap: release tethers)"),
    }
}

semantic_actions! {
    /// The `wingsuit` set.
    WingsuitAction in Wingsuit {
        Steer => ("steer", Vec2, "Steer"),
        Turn => ("turn", Float, "Turn"),
        AirBrake => ("air_brake", Float, "Air brake"),
        FireGuns => ("fire_guns", Float, "Fire the wingsuit guns"),
        FireSecondary => ("fire_secondary", Float, "Fire the wingsuit's secondary weapon"),
        Boost => ("boost", Bool, "Boost"),
        /// After a stick flick the mod turns the press into an evade.
        OpenParachute => ("open_parachute", Bool, "Open the parachute (after a stick flick: evade)"),
        Use => ("use", Bool, "Close the wingsuit"),
        ReleaseTethers => ("release_tethers", Bool, "Release tethers"),
        /// Unbound by default, in favour of [`WingsuitAction::Grapple`].
        FireGrapple => ("fire_grapple", Float, "Grapple, on an analog input (hold: tether)"),
        Grapple => ("grapple", Bool, "Grapple (hold: tether or tighten; double tap: release tethers)"),
    }
}

semantic_actions! {
    /// The `land_vehicle` set: cars, motorcycles, boats, and unresolved vehicles.
    LandVehicleAction in LandVehicle {
        Steer => ("steer", Vec2, "Steer, lean, and tilt"),
        Turn => ("turn", Float, "Turn"),
        /// Read in the driver's seat.
        Accelerate => ("accelerate", Float, "Accelerate"),
        /// Read in the driver's seat.
        Reverse => ("reverse", Float, "Brake and reverse"),
        FirePrimary => ("fire_primary", Float, "Fire the vehicle weapon"),
        FireSecondary => ("fire_secondary", Float, "Fire the secondary weapon"),
        Stunt => ("stunt", Bool, "Stunt position"),
        ExitVehicle => ("exit_vehicle", Bool, "Exit the vehicle (hold: gunner seat)"),
        VehicleMod => ("vehicle_mod", Bool, "Nitrous (tap: turbo jump)"),
        Handbrake => ("handbrake", Bool, "Handbrake"),
        Horn => ("horn", Bool, "Horn or magnet"),
        /// Read out of the driver's seat.
        Fire => ("fire", Float, "Fire (passenger)"),
        /// Read out of the driver's seat while dual-wielding.
        FireOffHandPistol => ("fire_off_hand_pistol", Float, "Fire the off-hand pistol (passenger)"),
        Grapple => ("grapple", Bool, "Grapple (hold: tether or tighten; double tap: release tethers)"),
    }
}

semantic_actions! {
    /// The `helicopter` set.
    HelicopterAction in Helicopter {
        Cyclic => ("cyclic", Vec2, "Pitch and roll"),
        Yaw => ("yaw", Float, "Yaw"),
        /// Read in the driver's seat.
        CollectiveUp => ("collective_up", Float, "Climb"),
        /// Read in the driver's seat.
        CollectiveDown => ("collective_down", Float, "Descend"),
        FirePrimary => ("fire_primary", Float, "Fire the vehicle weapon"),
        FireSecondary => ("fire_secondary", Float, "Fire the secondary weapon"),
        Stunt => ("stunt", Bool, "Stunt position"),
        ExitVehicle => ("exit_vehicle", Bool, "Exit the vehicle (hold: gunner seat)"),
        VehicleMod => ("vehicle_mod", Bool, "Vehicle mod"),
        Horn => ("horn", Bool, "Horn or magnet"),
        /// Read out of the driver's seat.
        Fire => ("fire", Float, "Fire (passenger)"),
        /// Read out of the driver's seat while dual-wielding.
        FireOffHandPistol => ("fire_off_hand_pistol", Float, "Fire the off-hand pistol (passenger)"),
        Grapple => ("grapple", Bool, "Grapple (hold: tether or tighten; double tap: release tethers)"),
    }
}

semantic_actions! {
    /// The `plane` set.
    PlaneAction in Plane {
        /// Forward pitches the nose down, as on the gamepad.
        PitchRoll => ("pitch_roll", Vec2, "Pitch and roll"),
        Rudder => ("rudder", Float, "Rudder"),
        /// Read in the driver's seat.
        ThrustUp => ("thrust_up", Float, "More thrust"),
        /// Read in the driver's seat.
        ThrustDown => ("thrust_down", Float, "Less thrust"),
        FirePrimary => ("fire_primary", Float, "Fire the vehicle weapon"),
        FireSecondary => ("fire_secondary", Float, "Fire the secondary weapon"),
        Stunt => ("stunt", Bool, "Stunt position"),
        ExitVehicle => ("exit_vehicle", Bool, "Exit the vehicle (hold: gunner seat)"),
        Horn => ("horn", Bool, "Horn or magnet"),
        /// Read out of the driver's seat.
        Fire => ("fire", Float, "Fire (passenger)"),
        /// Read out of the driver's seat while dual-wielding.
        FireOffHandPistol => ("fire_off_hand_pistol", Float, "Fire the off-hand pistol (passenger)"),
        Grapple => ("grapple", Bool, "Grapple (hold: tether or tighten; double tap: release tethers)"),
    }
}

semantic_actions! {
    /// The `menu` set: the game's menus and the native map.
    MenuAction in Menu {
        MenuConfirm => ("menu_confirm", Bool, "Confirm (with the grip: special or filter)"),
        MenuBack => ("menu_back", Bool, "Back (with the grip: reset to default)"),
        /// Each hand's modifier for its own presses, latched at each press.
        MenuModifier => ("menu_modifier", Float, "Menu modifier"),
        MenuNavigate => ("menu_navigate", Vec2, "Navigate (with the grip: switch tabs)"),
        MenuTabPad => ("menu_tab_pad", Vec2, "Tab direction"),
        MenuTabPress => ("menu_tab_press", Float, "Switch tabs"),
        /// Not read on the hand that owns the native UI pointer.
        MenuPagePrev => ("menu_page_prev", Float, "Previous page"),
        /// Not read on the hand that owns the native UI pointer.
        MenuPageNext => ("menu_page_next", Float, "Next page"),
        /// Read only on the hand that owns the native UI pointer.
        PointerSelect => ("pointer_select", Float, "Pointer click"),
        MenuFocus => ("menu_focus", Bool, "Focus"),
        MenuRecenter => ("menu_recenter", Bool, "Recenter the map"),
        MapZoom => ("map_zoom", Float, "Map zoom"),
    }
}
