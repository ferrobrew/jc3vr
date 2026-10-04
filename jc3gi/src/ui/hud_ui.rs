#![cfg_attr(any(), rustfmt::skip)]
#[repr(u32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// `CHUDUI::EHUDState`: which parts of the HUD are shown. [`HUDUI::Update`](crate::ui::hud_ui::HUDUI::Update) skips most per-frame
/// element updates (reticles, health, vehicle, points of interest, button hints) in
/// [`HUD_HIDE_ALL`](crate::ui::hud_ui::HUDState::HUD_HIDE_ALL) and
/// [`HUD_HIDE_ALL_NON_OVERLAY`](crate::ui::hud_ui::HUDState::HUD_HIDE_ALL_NON_OVERLAY).
pub enum HUDState {
    HUD_SHOW_ALL = 0isize as _,
    HUD_CINEMATIC = 1isize as _,
    HUD_HIDE_ALL = 2isize as _,
    HUD_ROM = 3isize as _,
    HUD_MISSION_B = 4isize as _,
    HUD_HIDE_ALL_NON_OVERLAY = 5isize as _,
    HUD_REWARD_SEQUENCE = 6isize as _,
    HUD_ALL_DISABLED = 7isize as _,
    HUD_ALL_DISABLED_KEEP_RETICLE = 8isize as _,
}
fn _HUDState_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], HUDState>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// The in-game HUD (`CHUDUI`, a `CUIBase` Scaleform movie wrapper). Only the fields and methods
/// below are mapped.
pub struct HUDUI {
    _field_0: [u8; 388],
    /// The current HUD visibility state.
    pub m_State: crate::ui::hud_ui::HUDState,
    _field_188: [u8; 144],
    /// Whether the sniper-scope overlay is shown, set by [`ShowSniperOverlay`](crate::ui::hud_ui::HUDUI::ShowSniperOverlay)
    /// and cleared by [`HideSniperOverlay`](crate::ui::hud_ui::HUDUI::HideSniperOverlay).
    pub m_ShowingSniperOverlay: bool,
    _field_219: [u8; 7],
}
fn _HUDUI_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x220], HUDUI>([0u8; 0x220]);
    }
    unreachable!()
}
impl HUDUI {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5418223648usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl HUDUI {
    pub const Update_ADDRESS: usize = 0x140F11D40;
    /// The per-frame HUD update: refreshes [`m_State`](crate::ui::hud_ui::HUDUI::m_State) through `UpdateState`, then,
    /// unless the state hides the HUD, updates the reticles, health, vehicle, wingsuit, points of
    /// interest, tethers, heat, button hints, subtitles, and notifications.
    pub unsafe fn Update(&mut self, dt: f32, dtf: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32, dtf: f32) = ::std::mem::transmute(
                Self::Update_ADDRESS,
            );
            f(self as *mut Self as _, dt, dtf)
        }
    }
    pub const ShowSniperOverlay_ADDRESS: usize = 0x140DFA3D0;
    /// Sets [`m_ShowingSniperOverlay`](crate::ui::hud_ui::HUDUI::m_ShowingSniperOverlay) and invokes the movie's
    /// `ShowSniperOverlay`.
    pub unsafe fn ShowSniperOverlay(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::ShowSniperOverlay_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const HideSniperOverlay_ADDRESS: usize = 0x140DFA410;
    /// Clears [`m_ShowingSniperOverlay`](crate::ui::hud_ui::HUDUI::m_ShowingSniperOverlay) and invokes the movie's
    /// `HideSniperOverlay`.
    pub unsafe fn HideSniperOverlay(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::HideSniperOverlay_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const UpdateWeaponReticle_ADDRESS: usize = 0x140E28890;
    /// Updates the weapon reticle for the local player's character: shape, spread, and hit and
    /// target feedback, pushed to the movie.
    pub unsafe fn UpdateWeaponReticle(
        &mut self,
        player: *mut crate::character::player::Player,
        character: *mut crate::character::character::Character,
        dt: f32,
        dtf: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                player: *mut crate::character::player::Player,
                character: *mut crate::character::character::Character,
                dt: f32,
                dtf: f32,
            ) = ::std::mem::transmute(Self::UpdateWeaponReticle_ADDRESS);
            f(self as *mut Self as _, player, character, dt, dtf)
        }
    }
    pub const UpdateGrappleReticle_ADDRESS: usize = 0x140E29410;
    /// Updates the grapple reticle: projects the grapple aim and active-wire attachment points to
    /// the screen through `CUIManager::Convert3DCoords` and pushes them to the movie.
    pub unsafe fn UpdateGrappleReticle(
        &mut self,
        player: *mut crate::character::player::Player,
        character: *mut crate::character::character::Character,
        dt: f32,
        dtf: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                player: *mut crate::character::player::Player,
                character: *mut crate::character::character::Character,
                dt: f32,
                dtf: f32,
            ) = ::std::mem::transmute(Self::UpdateGrappleReticle_ADDRESS);
            f(self as *mut Self as _, player, character, dt, dtf)
        }
    }
    pub const OnOmniDamage_ADDRESS: usize = 0x140E2CDB0;
    /// Sets the UI manager's grab-screen timer to
    /// [`OMNI_DAMAGE_GRAB_SCREEN_TIME`](crate::ui::hud_ui::HUDUI::OMNI_DAMAGE_GRAB_SCREEN_TIME) and invokes the movie's
    /// `OnOmniDamage`.
    pub unsafe fn OnOmniDamage(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::OnOmniDamage_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const OnOutOfAmmoFire_ADDRESS: usize = 0x140DFA520;
    /// Invokes the movie's `OnOutOfAmmoFire`, the out-of-ammunition notice. Its only caller is
    /// [`WeaponBase::CheckIfReloadShouldBeTriggeredBase`](crate::equipment::weapon::WeaponBase::CheckIfReloadShouldBeTriggeredBase),
    /// at a trigger pull of the local player's weapon that is empty, cannot reload, and found no
    /// weapon to switch to.
    pub unsafe fn OnOutOfAmmoFire(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::OnOutOfAmmoFire_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl HUDUI {
    /// The value `OnOmniDamage` writes into [`UIManager::m_GrabScreenTimer`](crate::ui::ui_manager::UIManager::m_GrabScreenTimer) (seconds) when
    /// omnidirectional damage is taken.
    pub const OMNI_DAMAGE_GRAB_SCREEN_TIME: f32 = 2.0;
}
impl std::convert::AsRef<HUDUI> for HUDUI {
    fn as_ref(&self) -> &HUDUI {
        self
    }
}
impl std::convert::AsMut<HUDUI> for HUDUI {
    fn as_mut(&mut self) -> &mut HUDUI {
        self
    }
}
