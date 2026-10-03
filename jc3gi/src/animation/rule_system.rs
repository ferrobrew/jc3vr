#![cfg_attr(any(), rustfmt::skip)]
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// How long a modifier set through [`AnimationRuleSystem::SetModifier`](crate::animation::rule_system::AnimationRuleSystem::SetModifier) stays active
/// (`CAnimationRuleSystem::EAnimationModifierType`). Each value is a bit index in the rule system's
/// per-modifier type mask.
pub enum AnimationModifierType {
    FRAME_BOUND = 0isize as _,
    STATE_BOUND = 1isize as _,
}
fn _AnimationModifierType_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], AnimationModifierType>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// An animation rule system (`CAnimationRuleSystem`): drives one afsmb state machine for a character.
/// A character's [`AnimatedModel`](crate::character::character::AnimatedModel) holds one per animated model,
/// the first being the body's.
pub struct AnimationRuleSystem {
    _field_0: [u8; 72],
    /// The running state machine instance this rule system drives.
    pub m_StateMachineInstance: crate::types::shared_ptr::SharedPtr<
        crate::animation::rule_system::StateMachineInstance,
    >,
}
fn _AnimationRuleSystem_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x58], AnimationRuleSystem>([0u8; 0x58]);
    }
    unreachable!()
}
impl AnimationRuleSystem {
    pub const SetModifier_ADDRESS: usize = 0x140479A10;
    /// Marks an animation modifier (`ava::idstring<NAnimationSystem::SModifierIdTag>`, passed as a
    /// pointer to its id) active on this rule system: it sets the bit for `modifier_type` in the
    /// rule system's modifier-to-type-mask map. The animation set's nodes select their variant clips
    /// by the active modifiers (an `ANIM_*` node lists one variant per modifier combination, such as
    /// a weapon's held or aimed stance). [`Character::SetModifier`](crate::character::character::Character::SetModifier)
    /// forwards to this for every rule system of the character's animated model.
    pub unsafe fn SetModifier(
        &mut self,
        modifier: *const u32,
        modifier_type: crate::animation::rule_system::AnimationModifierType,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                modifier: *const u32,
                modifier_type: crate::animation::rule_system::AnimationModifierType,
            ) = ::std::mem::transmute(Self::SetModifier_ADDRESS);
            f(self as *mut Self as _, modifier, modifier_type)
        }
    }
}
impl std::convert::AsRef<AnimationRuleSystem> for AnimationRuleSystem {
    fn as_ref(&self) -> &AnimationRuleSystem {
        self
    }
}
impl std::convert::AsMut<AnimationRuleSystem> for AnimationRuleSystem {
    fn as_mut(&mut self) -> &mut AnimationRuleSystem {
        self
    }
}
#[repr(C, align(8))]
/// An animation state (`NAnimationSystem::CState`): one node of a character's animation state machine,
/// i.e. an afsmb `S_*` rule state. Identified by its name hash.
pub struct State {
    _field_0: [u8; 16],
    /// The state's name hash (`hashlittle(state_name)`, e.g. `hashlittle("S_IDLE")`), identifying
    /// which afsmb state this is. Read by `CCharacter::IsInVehicleAttachState` and the other
    /// current-state checks.
    pub m_HashID: crate::hash::HashString,
    _field_14: [u8; 4],
}
fn _State_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x18], State>([0u8; 0x18]);
    }
    unreachable!()
}
impl State {}
impl std::convert::AsRef<State> for State {
    fn as_ref(&self) -> &State {
        self
    }
}
impl std::convert::AsMut<State> for State {
    fn as_mut(&mut self) -> &mut State {
        self
    }
}
#[repr(C, align(8))]
/// A running animation state machine (`NAnimationSystem::CStateMachineInstance`): tracks the current
/// [`State`](crate::animation::rule_system::State) for one [`AnimationRuleSystem`](crate::animation::rule_system::AnimationRuleSystem).
pub struct StateMachineInstance {
    _field_0: [u8; 24],
    /// The state the machine is currently in.
    pub m_CurrentState: *mut crate::animation::rule_system::State,
}
fn _StateMachineInstance_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x20], StateMachineInstance>([0u8; 0x20]);
    }
    unreachable!()
}
impl StateMachineInstance {}
impl std::convert::AsRef<StateMachineInstance> for StateMachineInstance {
    fn as_ref(&self) -> &StateMachineInstance {
        self
    }
}
impl std::convert::AsMut<StateMachineInstance> for StateMachineInstance {
    fn as_mut(&mut self) -> &mut StateMachineInstance {
        self
    }
}
