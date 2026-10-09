#![cfg_attr(any(), rustfmt::skip)]
#[derive(Copy, Clone)]
#[repr(C, align(4))]
/// `CHUDUI::SButtonHint`: one queued hint.
pub struct ButtonHint {
    pub m_Request: crate::ui::hud_ui::ButtonHintRequest,
    /// Two input key ids, left as the request's copy leaves them; the hint's glyphs are built from
    /// the mapping's actions instead.
    pub m_KeyId1: u32,
    pub m_KeyId2: u32,
    /// Seconds left before a timed hint is removed.
    pub m_TimeLeft: f32,
    /// The id [`HUDUI::AddButtonHint`](crate::ui::hud_ui::HUDUI::AddButtonHint) returned for it, by which its source removes it.
    pub m_Id: u32,
}
fn _ButtonHint_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x34], ButtonHint>([0u8; 0x34]);
    }
    unreachable!()
}
impl ButtonHint {}
impl std::convert::AsRef<ButtonHint> for ButtonHint {
    fn as_ref(&self) -> &ButtonHint {
        self
    }
}
impl std::convert::AsMut<ButtonHint> for ButtonHint {
    fn as_mut(&mut self) -> &mut ButtonHint {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(4))]
/// `CHUDUI::SButtonHintRequest`: what a source asks the HUD to show.
pub struct ButtonHintRequest {
    pub m_Type: crate::ui::hud_ui::ButtonHintType,
    /// The semantic mapping the hint names: its actions, its interaction, and the `buttonhint_*`
    /// key of its description.
    pub m_Mapping: crate::input::button_mapping::MappingInfo,
    /// How long the hint shows, seconds; zero or less shows it until it is removed.
    pub m_Lifetime: f32,
}
fn _ButtonHintRequest_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x24], ButtonHintRequest>([0u8; 0x24]);
    }
    unreachable!()
}
impl ButtonHintRequest {}
impl std::convert::AsRef<ButtonHintRequest> for ButtonHintRequest {
    fn as_ref(&self) -> &ButtonHintRequest {
        self
    }
}
impl std::convert::AsMut<ButtonHintRequest> for ButtonHintRequest {
    fn as_mut(&mut self) -> &mut ButtonHintRequest {
        self
    }
}
#[repr(u32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// The in-game HUD (`CHUDUI`, a `CUIBase` Scaleform movie wrapper). Only the fields and methods
/// below are mapped.
/// `CHUDUI::EButtonHintType`: where the HUD shows a button hint.
pub enum ButtonHintType {
    BUTTON_HINT_NORMAL = 0isize as _,
    BUTTON_HINT_Y_USE = 1isize as _,
    BUTTON_HINT_BIG = 2isize as _,
}
fn _ButtonHintType_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], ButtonHintType>([0u8; 0x4]);
    }
    unreachable!()
}
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
pub struct HUDUI {
    _field_0: [u8; 388],
    /// The current HUD visibility state.
    pub m_State: crate::ui::hud_ui::HUDState,
    _field_188: [u8; 4],
    /// Seconds left before the weapon readout hides.
    /// [`UpdateShowWeaponsAndGrenadesUiTimer`](crate::ui::hud_ui::HUDUI::UpdateShowWeaponsAndGrenadesUiTimer) sets it to
    /// [`SHOW_WEAPONS_UI_TIME`](crate::ui::hud_ui::HUDUI::SHOW_WEAPONS_UI_TIME) on every update in which
    /// `CCharacter::ShouldShowWeaponsUi` holds, counts it down otherwise, and zeroes it while the
    /// character holsters its weapon.
    pub m_ShowWeaponsUiTimer: f32,
    /// Whether the weapon readout is shown: the last of the movie's `ShowWeaponsUi` /
    /// `HideWeaponsUi` that [`UpdateShowWeaponsAndGrenadesUiTimer`](crate::ui::hud_ui::HUDUI::UpdateShowWeaponsAndGrenadesUiTimer)
    /// invoked.
    pub m_IsShowingWeaponsUi: bool,
    _field_191: [u8; 3],
    /// Seconds left before the grenade readout hides; the grenade counterpart of
    /// [`m_ShowWeaponsUiTimer`](crate::ui::hud_ui::HUDUI::m_ShowWeaponsUiTimer).
    pub m_ShowGrenadesUiTimer: f32,
    /// Whether the grenade readout is shown (`ShowGrenadesUi` / `HideGrenadesUi`).
    pub m_IsShowingGrenadesUi: bool,
    _field_199: [u8; 55],
    /// The lock guarding [`m_POIListHead`](crate::ui::hud_ui::HUDUI::m_POIListHead): POIs are registered and unregistered from
    /// other systems, and [`UpdatePOIs`](crate::ui::hud_ui::HUDUI::UpdatePOIs) holds it for its whole walk.
    pub m_POIListLock: *mut crate::graphics_engine::device::CRITICAL_SECTION,
    /// The head (sentinel) node of the registered points of interest
    /// (`std::list<CHUDUI::SPOIRecord>`).
    pub m_POIListHead: *mut crate::ui::hud_ui::POIListNode,
    /// The number of records in [`m_POIListHead`](crate::ui::hud_ui::HUDUI::m_POIListHead).
    pub m_POIListSize: u64,
    _field_1e8: [u8; 8],
    /// The [`m_ID`](crate::ui::poi::POI::m_ID) of the one on-screen POI whose label is shown: the one
    /// nearest the screen centre among those whose label box contains the centre, chosen by
    /// [`UpdatePOIs`](crate::ui::hud_ui::HUDUI::UpdatePOIs). Zero when none qualifies.
    pub m_ShowingLabelPOIId: u32,
    _field_1f4: [u8; 36],
    /// Whether the sniper-scope overlay is shown, set by [`ShowSniperOverlay`](crate::ui::hud_ui::HUDUI::ShowSniperOverlay)
    /// and cleared by [`HideSniperOverlay`](crate::ui::hud_ui::HUDUI::HideSniperOverlay).
    pub m_ShowingSniperOverlay: bool,
    _field_219: [u8; 3],
    /// The selection state last pushed to the movie by
    /// [`UpdateWeaponSelection`](crate::ui::hud_ui::HUDUI::UpdateWeaponSelection).
    pub m_LastWeaponSelectionParams: crate::ui::hud_ui::WeaponSelectionParams,
    /// The movie's weapon selection clip, `MCI_weapon_selection_wheel`: a managed display-object
    /// value. [`UpdateWeaponSelectionPlacement`](crate::ui::hud_ui::HUDUI::UpdateWeaponSelectionPlacement) writes its
    /// position each frame.
    pub m_SelectionWheelUi: crate::ui::scaleform::Value,
    _field_258: [u8; 24],
    /// The selection clip's extent around its position for the current layout, in stage pixels, as
    /// the movie's `GetWeaponSelectionWheelBB{Bottom,Top,Left,Right}` report it, in that order (`x`
    /// to `w`). [`RecalculateWheelBB`](crate::ui::hud_ui::HUDUI::RecalculateWheelBB) refreshes it.
    /// [`UpdateWeaponSelectionPlacement`](crate::ui::hud_ui::HUDUI::UpdateWeaponSelectionPlacement) keeps the clip's x
    /// between the safe area's left minus `z` and its right minus `w`, and its y between the safe
    /// area's top minus `y` and its bottom minus `x`, so each component is an edge's signed offset
    /// from the clip's position.
    pub m_SelectionWheelBB: crate::types::math::Vector4,
    _field_280: [u8; 514],
    /// The health last pushed to the movie by [`UpdateHealth`](crate::ui::hud_ui::HUDUI::UpdateHealth);
    /// [`InitHealth`](crate::ui::hud_ui::HUDUI::InitHealth) resets it to -1 so the next update pushes again.
    pub m_LastHealth: i16,
    _field_484: [u8; 676],
    /// Whether [`m_ButtonHints`](crate::ui::hud_ui::HUDUI::m_ButtonHints) changed since
    /// [`UpdateButtonHints`](crate::ui::hud_ui::HUDUI::UpdateButtonHints) last pushed it to the movie.
    pub m_ButtonHintsNeedsUpdate: bool,
    _field_729: [u8; 7],
    /// The queued button hints, in the order they were added.
    pub m_ButtonHints: crate::types::std_vector::Vector<crate::ui::hud_ui::ButtonHint>,
    /// The lock guarding [`m_ButtonHints`](crate::ui::hud_ui::HUDUI::m_ButtonHints) and
    /// [`m_ButtonHintsNeedsUpdate`](crate::ui::hud_ui::HUDUI::m_ButtonHintsNeedsUpdate): hints are added and removed
    /// from the systems that raise them, and [`UpdateButtonHints`](crate::ui::hud_ui::HUDUI::UpdateButtonHints) holds it
    /// for its whole walk.
    pub m_ButtonHintsLock: *mut crate::graphics_engine::device::CRITICAL_SECTION,
}
fn _HUDUI_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x758], HUDUI>([0u8; 0x758]);
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
    pub const InitWeapons_ADDRESS: usize = 0x140E4AD30;
    /// Rebuilds the weapon selection's list from the local player's inventory: each slot's weapon
    /// icon and name, or none for an empty slot, then shows the selected weapon. The inventory's
    /// pre-reset event handler calls it while the HUD is up.
    pub unsafe fn InitWeapons(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::InitWeapons_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const OnPickupWeapon_ADDRESS: usize = 0x140E2A460;
    /// Shows `weapon` in its slot's entry of the weapon selection: the weapon's icon, slot, and
    /// name ([`WeaponBase::m_WeaponNameHash`](crate::equipment::weapon::WeaponBase::m_WeaponNameHash)).
    /// [`WeaponBase::OnPickup`](crate::equipment::weapon::WeaponBase::OnPickup) calls it when a locally
    /// controlled character takes a weapon.
    pub unsafe fn OnPickupWeapon(
        &mut self,
        weapon: *const crate::types::shared_ptr::SharedPtr<
            crate::equipment::weapon::WeaponBase,
        >,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                weapon: *const crate::types::shared_ptr::SharedPtr<
                    crate::equipment::weapon::WeaponBase,
                >,
            ) = ::std::mem::transmute(Self::OnPickupWeapon_ADDRESS);
            f(self as *mut Self as _, weapon)
        }
    }
    pub const Update_ADDRESS: usize = 0x140F11D40;
    /// The per-frame HUD update: refreshes [`m_State`](crate::ui::hud_ui::HUDUI::m_State) through `UpdateState`, then,
    /// unless the state hides the HUD, updates the reticles, health, vehicle, wingsuit, points of
    /// interest, tethers, heat, button hints, subtitles, and notifications. `CUIManager::PreRender`
    /// calls it (through the UI's vtable) on every active UI before it advances the movie and
    /// captures it once, so each capture follows exactly one HUD update.
    pub unsafe fn Update(&mut self, dt: f32, dtf: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32, dtf: f32) = ::std::mem::transmute(
                Self::Update_ADDRESS,
            );
            f(self as *mut Self as _, dt, dtf)
        }
    }
    pub const AddButtonHint_ADDRESS: usize = 0x140EBA580;
    /// Queues a button hint under [`m_ButtonHintsLock`](crate::ui::hud_ui::HUDUI::m_ButtonHintsLock): appends it to
    /// [`m_ButtonHints`](crate::ui::hud_ui::HUDUI::m_ButtonHints) with its lifetime as the time left and the next id of
    /// a running counter, and sets [`m_ButtonHintsNeedsUpdate`](crate::ui::hud_ui::HUDUI::m_ButtonHintsNeedsUpdate).
    /// Returns the id.
    pub unsafe fn AddButtonHint(
        &mut self,
        request: *const crate::ui::hud_ui::ButtonHintRequest,
    ) -> u32 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                request: *const crate::ui::hud_ui::ButtonHintRequest,
            ) -> u32 = ::std::mem::transmute(Self::AddButtonHint_ADDRESS);
            f(self as *mut Self as _, request)
        }
    }
    pub const UpdateButtonHints_ADDRESS: usize = 0x140E8AEA0;
    /// Ages the queued hints and pushes them to the movie, under
    /// [`m_ButtonHintsLock`](crate::ui::hud_ui::HUDUI::m_ButtonHintsLock). The first three
    /// [`BUTTON_HINT_NORMAL`](crate::ui::hud_ui::ButtonHintType::BUTTON_HINT_NORMAL) hints and every other hint
    /// count down their time left by `dt`, and one that runs out is removed; a big hint, one with
    /// no time left to begin with, and a normal hint past the third do not age. When the queue
    /// changed, it builds the movie's three arrays (normal, use, big) of id, two key strings (from
    /// the mapping's [`m_Action1`](crate::input::button_mapping::MappingInfo::m_Action1) and [`m_Action2`](crate::input::button_mapping::MappingInfo::m_Action2)),
    /// interaction, and localised description, again counting only the first three normal hints,
    /// and invokes the movie's `UpdateButtonHints`.
    pub unsafe fn UpdateButtonHints(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::UpdateButtonHints_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
    pub const UpdatePOIs_ADDRESS: usize = 0x140F03B70;
    /// Projects and places every registered point of interest. Unless
    /// [`m_State`](crate::ui::hud_ui::HUDUI::m_State) is [`HUD_CINEMATIC`](crate::ui::hud_ui::HUDState::HUD_CINEMATIC), it holds
    /// [`m_POIListLock`](crate::ui::hud_ui::HUDUI::m_POIListLock) and walks [`m_POIListHead`](crate::ui::hud_ui::HUDUI::m_POIListHead):
    /// records whose POI is destroyed are removed (invoking the movie's `RemovePoi`), invisible POIs
    /// are skipped, and a record without a clip gets one. For each POI it calls
    /// [`POI::UpdatePOI`](crate::ui::poi::POI::UpdatePOI), takes the translation of
    /// [`POI::m_Transform`](crate::ui::poi::POI::m_Transform) as the world point, and projects it through
    /// [`UIManager::Get2DInfo`](crate::ui::ui_manager::UIManager::Get2DInfo) with the active camera's
    /// view-projection, its orthonormalised transform, its field of view, a fixed screen margin,
    /// the POI's [`m_ClampToScreen`](crate::ui::poi::POI::m_ClampToScreen), and its
    /// [`m_Attach2DOffset`](crate::ui::poi::POI::m_Attach2DOffset). The marker shows while the distance to
    /// the player character is within the POI's range, it is not behind the camera (unless
    /// clamped), and the vehicle, use, settlement, mission, and ROM filters pass; a change invokes
    /// `ShowPoi` / `HidePoi` and updates [`POIRecord::m_Visible`](crate::ui::hud_ui::POIRecord::m_Visible). A shown marker gets its icon
    /// scale and alpha, ping, distance text, edge-arrow state, flash, label, progress, and weapon
    /// icon through the movie's `UpdatePoi`, and then its x and y (rounded to whole stage pixels)
    /// through [`SetDisplayInfo`](crate::ui::scaleform::ValueObjectInterface::SetDisplayInfo). Finally it
    /// stores the labelled POI in [`m_ShowingLabelPOIId`](crate::ui::hud_ui::HUDUI::m_ShowingLabelPOIId). Called from
    /// [`Update`](crate::ui::hud_ui::HUDUI::Update) with the local player and character.
    pub unsafe fn UpdatePOIs(
        &mut self,
        player: *mut crate::character::player::Player,
        character: *mut crate::character::character::Character,
        dt: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                player: *mut crate::character::player::Player,
                character: *mut crate::character::character::Character,
                dt: f32,
            ) = ::std::mem::transmute(Self::UpdatePOIs_ADDRESS);
            f(self as *mut Self as _, player, character, dt)
        }
    }
    pub const UpdateShowWeaponsAndGrenadesUiTimer_ADDRESS: usize = 0x140E12730;
    /// Drives the weapon and grenade readouts' visibility timers
    /// ([`m_ShowWeaponsUiTimer`](crate::ui::hud_ui::HUDUI::m_ShowWeaponsUiTimer),
    /// [`m_ShowGrenadesUiTimer`](crate::ui::hud_ui::HUDUI::m_ShowGrenadesUiTimer)) and invokes the movie's
    /// `ShowWeaponsUi` / `HideWeaponsUi` and `ShowGrenadesUi` / `HideGrenadesUi` on their edges. It
    /// also shows and hides the beacon helper with the selected grenade slot.
    pub unsafe fn UpdateShowWeaponsAndGrenadesUiTimer(
        &mut self,
        player: *mut crate::character::player::Player,
        character: *mut crate::character::character::Character,
        dt: f32,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                player: *mut crate::character::player::Player,
                character: *mut crate::character::character::Character,
                dt: f32,
            ) -> bool = ::std::mem::transmute(
                Self::UpdateShowWeaponsAndGrenadesUiTimer_ADDRESS,
            );
            f(self as *mut Self as _, player, character, dt)
        }
    }
    pub const UpdateHealth_ADDRESS: usize = 0x140E2CC00;
    /// Pushes the character's health ([`Character::GetHealth`](crate::character::character::Character::GetHealth)) to the movie's `UpdateHealth`
    /// with `dt` whenever it differs from [`m_LastHealth`](crate::ui::hud_ui::HUDUI::m_LastHealth). The movie draws it
    /// as the full-screen damage vignette and the reticle's low-health outline; there is no
    /// numeric readout.
    pub unsafe fn UpdateHealth(
        &mut self,
        character: *mut crate::character::character::Character,
        dt: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                character: *mut crate::character::character::Character,
                dt: f32,
            ) = ::std::mem::transmute(Self::UpdateHealth_ADDRESS);
            f(self as *mut Self as _, character, dt)
        }
    }
    pub const InitHealth_ADDRESS: usize = 0x140E2CA90;
    /// Resets [`m_LastHealth`](crate::ui::hud_ui::HUDUI::m_LastHealth) and invokes the movie's `InitHealth` with the
    /// local player character's [`m_MaxHealth`](crate::character::character::Character::m_MaxHealth) and a second, unidentified
    /// value from the local player's data.
    pub unsafe fn InitHealth(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::InitHealth_ADDRESS,
            );
            f(self as *mut Self as _)
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
    pub const UpdateWeaponSelection_ADDRESS: usize = 0x140E2BCA0;
    /// Pushes the weapon selection to the movie's `UpdateWeaponSelection` (the selected slot, the
    /// shown flag, and the four valid flags) when any field differs from
    /// [`m_LastWeaponSelectionParams`](crate::ui::hud_ui::HUDUI::m_LastWeaponSelectionParams), and stores it there. Its
    /// only caller is [`Inventory::UpdateWeaponHandling`](crate::character::inventory::Inventory::UpdateWeaponHandling),
    /// which calls it every frame for the player, so it runs in the characters' parallel update. In
    /// the movie, a show edge tweens the arms out, the shown selection highlights the selected arm
    /// and dims the others, and a hide edge fades the selected arm out while scaling it up.
    pub unsafe fn UpdateWeaponSelection(
        &mut self,
        params: *const crate::ui::hud_ui::WeaponSelectionParams,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                params: *const crate::ui::hud_ui::WeaponSelectionParams,
            ) = ::std::mem::transmute(Self::UpdateWeaponSelection_ADDRESS);
            f(self as *mut Self as _, params)
        }
    }
    pub const UpdateWeaponSelectionPlacement_ADDRESS: usize = 0x140E2BF20;
    /// Places the weapon selection clip ([`m_SelectionWheelUi`](crate::ui::hud_ui::HUDUI::m_SelectionWheelUi)) over
    /// `character`'s hips. It takes the hips bone's world position, projects it through the render
    /// camera's view-projection with [`UIManager::Convert3DCoords`](crate::ui::ui_manager::UIManager::Convert3DCoords),
    /// clamps it so that [`m_SelectionWheelBB`](crate::ui::hud_ui::HUDUI::m_SelectionWheelBB) stays inside
    /// [`UIManager::m_SafeArea`](crate::ui::ui_manager::UIManager::m_SafeArea) (or, when the point does not
    /// project, centres it horizontally at the bottom of the safe area), averages it with the last
    /// three positions, and writes the clip's x and y through
    /// [`SetDisplayInfo`](crate::ui::scaleform::ValueObjectInterface::SetDisplayInfo). Called every frame
    /// from [`Update`](crate::ui::hud_ui::HUDUI::Update); it does not read `dt`.
    pub unsafe fn UpdateWeaponSelectionPlacement(
        &mut self,
        character: *mut crate::character::character::Character,
        dt: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                character: *mut crate::character::character::Character,
                dt: f32,
            ) = ::std::mem::transmute(Self::UpdateWeaponSelectionPlacement_ADDRESS);
            f(self as *mut Self as _, character, dt)
        }
    }
    pub const RecalculateWheelBB_ADDRESS: usize = 0x140E117E0;
    /// Refreshes [`m_SelectionWheelBB`](crate::ui::hud_ui::HUDUI::m_SelectionWheelBB) from the movie. Called when the
    /// weapons are initialised and when gamepad use changes, which changes the clip's layout.
    pub unsafe fn RecalculateWheelBB(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::RecalculateWheelBB_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl HUDUI {
    /// The value `OnOmniDamage` writes into [`UIManager::m_GrabScreenTimer`](crate::ui::ui_manager::UIManager::m_GrabScreenTimer) (seconds) when
    /// omnidirectional damage is taken.
    pub const OMNI_DAMAGE_GRAB_SCREEN_TIME: f32 = 2.0;
    /// The value, in seconds, that [`UpdateShowWeaponsAndGrenadesUiTimer`](crate::ui::hud_ui::HUDUI::UpdateShowWeaponsAndGrenadesUiTimer)
    /// sets [`m_ShowWeaponsUiTimer`](crate::ui::hud_ui::HUDUI::m_ShowWeaponsUiTimer) and
    /// [`m_ShowGrenadesUiTimer`](crate::ui::hud_ui::HUDUI::m_ShowGrenadesUiTimer) to while their readouts should show.
    pub const SHOW_WEAPONS_UI_TIME: f32 = 10.0;
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
#[repr(C, align(8))]
/// A node of [`HUDUI::m_POIListHead`](crate::ui::hud_ui::HUDUI::m_POIListHead) (an MSVC `std::list<CHUDUI::SPOIRecord>` node). The list's head
/// node is a sentinel without a record: its [`m_Next`](crate::ui::hud_ui::POIListNode::m_Next) is the first element, and the
/// last element's links back to it.
pub struct POIListNode {
    pub m_Next: *mut crate::ui::hud_ui::POIListNode,
    pub m_Prev: *mut crate::ui::hud_ui::POIListNode,
    pub m_Record: crate::ui::hud_ui::POIRecord,
}
fn _POIListNode_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x68], POIListNode>([0u8; 0x68]);
    }
    unreachable!()
}
impl POIListNode {}
impl std::convert::AsRef<POIListNode> for POIListNode {
    fn as_ref(&self) -> &POIListNode {
        self
    }
}
impl std::convert::AsMut<POIListNode> for POIListNode {
    fn as_mut(&mut self) -> &mut POIListNode {
        self
    }
}
#[repr(C, align(8))]
/// One registered point of interest (`CHUDUI::SPOIRecord`): the [`POI`](crate::ui::poi::POI) and the HUD's marker clip
/// for it.
pub struct POIRecord {
    /// The POI. [`HUDUI::UpdatePOIs`](crate::ui::hud_ui::HUDUI::UpdatePOIs) drops the record once the POI is destroyed.
    pub m_POI: crate::ui::poi::WeakPtrPOI,
    /// A strong reference that keeps the POI alive while it is registered, for POIs registered with
    /// the keep flag of `CHUDUI::RegisterPOI`; null otherwise.
    pub m_KeepPOI: crate::types::shared_ptr::SharedPtr<crate::ui::poi::POI>,
    /// The marker clip: a managed display-object value taken from the movie's pooled `poi` clips by
    /// `CHUDUI::CreatePOI` on the record's first update, and undefined (a zero type) until then.
    /// [`HUDUI::UpdatePOIs`](crate::ui::hud_ui::HUDUI::UpdatePOIs) writes its position each frame through
    /// [`SetDisplayInfo`](crate::ui::scaleform::ValueObjectInterface::SetDisplayInfo).
    pub m_Element: crate::ui::scaleform::Value,
    /// Whether the marker is shown: the last visibility [`HUDUI::UpdatePOIs`](crate::ui::hud_ui::HUDUI::UpdatePOIs) pushed to the movie
    /// (`ShowPoi` / `HidePoi`).
    pub m_Visible: bool,
    _field_51: [u8; 7],
}
fn _POIRecord_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x58], POIRecord>([0u8; 0x58]);
    }
    unreachable!()
}
impl POIRecord {}
impl std::convert::AsRef<POIRecord> for POIRecord {
    fn as_ref(&self) -> &POIRecord {
        self
    }
}
impl std::convert::AsMut<POIRecord> for POIRecord {
    fn as_mut(&mut self) -> &mut POIRecord {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(4))]
/// The weapon selection's state as [`Inventory::UpdateWeaponHandling`](crate::character::inventory::Inventory::UpdateWeaponHandling)
/// computes it each frame for the player (`CHUDUI::SWeaponSelectionParams`), passed to
/// [`HUDUI::UpdateWeaponSelection`](crate::ui::hud_ui::HUDUI::UpdateWeaponSelection).
pub struct WeaponSelectionParams {
    /// Whether the selection is shown: while a slot-select input is held (always under gamepad
    /// semantics; under keyboard semantics not for the planted explosive), and for a short while
    /// after a release whose commit is still pending or a weapon cycle step.
    pub m_Show: bool,
    _field_1: [u8; 3],
    /// The highlighted slot: the winning slot-select input's slot, the pending or cycled slot, or
    /// [`E_NONE`](crate::character::inventory::WeaponSlot::E_NONE) when the winner holds no weapon.
    pub m_SelectedSlot: crate::character::inventory::WeaponSlot,
    /// Whether the [`E_SLOT_DUAL_WIELD`](crate::character::inventory::WeaponSlot::E_SLOT_DUAL_WIELD) slot is offered: it holds a
    /// weapon.
    pub m_DualWieldIsValid: bool,
    /// Whether the [`E_SLOT_TWO_HANDED`](crate::character::inventory::WeaponSlot::E_SLOT_TWO_HANDED) slot is offered: it holds a
    /// weapon.
    pub m_TwoHandedIsValid: bool,
    /// Whether the [`E_SLOT_TWO_HANDED_SPECIAL`](crate::character::inventory::WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL) slot is
    /// offered: it holds a weapon, other than a mortar whose mortar mode is unavailable.
    pub m_TwoHandedSpecialIsValid: bool,
    /// Whether the [`E_SLOT_PLANTED_EXPLOSIVE`](crate::character::inventory::WeaponSlot::E_SLOT_PLANTED_EXPLOSIVE) slot is
    /// offered (see
    /// [`Inventory::m_DoesAnimationAllowSwitchToPlantedExplosive`](crate::character::inventory::Inventory::m_DoesAnimationAllowSwitchToPlantedExplosive)).
    pub m_PlantedExplosiveIsValid: bool,
}
fn _WeaponSelectionParams_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xC], WeaponSelectionParams>([0u8; 0xC]);
    }
    unreachable!()
}
impl WeaponSelectionParams {}
impl std::convert::AsRef<WeaponSelectionParams> for WeaponSelectionParams {
    fn as_ref(&self) -> &WeaponSelectionParams {
        self
    }
}
impl std::convert::AsMut<WeaponSelectionParams> for WeaponSelectionParams {
    fn as_mut(&mut self) -> &mut WeaponSelectionParams {
        self
    }
}
