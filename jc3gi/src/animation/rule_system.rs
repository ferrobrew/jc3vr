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
    _field_0: [u8; 52],
    /// The character's height over the ground or the water surface, whichever is nearer, metres.
    /// Written for the character's first rule system by
    /// [`CalculateHeightOverGround`](crate::character::character::Character::CalculateHeightOverGround),
    /// and zeroed by [`DoGroundInfoRaycasts`](crate::character::character::Character::DoGroundInfoRaycasts)
    /// while the proxy is supported or sliding with no ray cast. The graph's
    /// `CHeightOverGroundCondition` reads it or
    /// [`m_HeightOverGroundExcludingWater`](crate::animation::rule_system::AnimationRuleSystem::m_HeightOverGroundExcludingWater) by its flag
    /// (unverified).
    pub m_HeightOverGround: f32,
    /// The character's height over the ground alone, ignoring water, metres. Written and zeroed
    /// alongside [`m_HeightOverGround`](crate::animation::rule_system::AnimationRuleSystem::m_HeightOverGround); a cast that hits nothing within
    /// its 200 metres leaves the full length less the cast's lift.
    pub m_HeightOverGroundExcludingWater: f32,
    _field_3c: [u8; 12],
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
    pub const SetStateById_ADDRESS: usize = 0x14044AD50;
    /// Looks the state named by `state_id` up in the running machine's definition and, when it
    /// exists, moves the machine to it through
    /// [`StateMachineInstance::SetStateInternal`](crate::animation::rule_system::StateMachineInstance::SetStateInternal) whatever the current state's transitions allow.
    /// Returns whether the state was found. The character's own forced and layered state sets
    /// reach the machines through it.
    pub unsafe fn SetStateById(
        &mut self,
        state_id: *const crate::hash::HashString,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                state_id: *const crate::hash::HashString,
            ) -> bool = ::std::mem::transmute(Self::SetStateById_ADDRESS);
            f(self as *mut Self as _, state_id)
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
    _field_0: [u8; 16],
    /// The resource handle for the state machine definition that supplies the states and global transitions.
    pub m_StateMachine: *mut crate::game::ResourceHandle,
    /// The state the machine is currently in.
    pub m_CurrentState: *mut crate::animation::rule_system::State,
    /// The game object the machine animates (`CStateMachineInstance::GetUserData`): for a
    /// character's machines, its [`Character::GAME_OBJECT_BASE_OFFSET`](crate::character::character::Character::GAME_OBJECT_BASE_OFFSET)
    /// subobject. The graph's conditions read their character through it.
    pub m_UserData: *mut crate::game::GameObject,
}
fn _StateMachineInstance_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x28], StateMachineInstance>([0u8; 0x28]);
    }
    unreachable!()
}
impl StateMachineInstance {
    pub const TryEvent_ADDRESS: usize = 0x1403DD5E0;
    /// Tests whether an event has an eligible transition, evaluating the current state's transitions before the global transitions. Evaluates transition conditions, but does not enqueue the event, change the current state, or execute state tasks.
    pub unsafe fn TryEvent(&self, event: *const u32) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                event: *const u32,
            ) -> bool = ::std::mem::transmute(Self::TryEvent_ADDRESS);
            f(self as *const Self as _, event)
        }
    }
    pub const SetStateInternal_ADDRESS: usize = 0x1403C7F00;
    /// Moves the machine to `new_state`: leaves the current state (its state tasks' exits run),
    /// makes `new_state` current, enters it (its state tasks' entries run), and calls the
    /// instance's transition callback with the old and new states, which for a character's rule
    /// system starts the new state's animation. Every state change goes through it: an event's
    /// transition, an external event's, and a state set by id
    /// ([`AnimationRuleSystem::SetStateById`](crate::animation::rule_system::AnimationRuleSystem::SetStateById)).
    pub unsafe fn SetStateInternal(
        &mut self,
        new_state: *mut crate::animation::rule_system::State,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                new_state: *mut crate::animation::rule_system::State,
            ) = ::std::mem::transmute(Self::SetStateInternal_ADDRESS);
            f(self as *mut Self as _, new_state)
        }
    }
}
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
#[repr(C, align(8))]
/// A state-machine transition (`NAnimationSystem::CTransition`). Only its destination is mapped.
pub struct Transition {}
impl Transition {
    pub const GetDestination_ADDRESS: usize = 0x1403BC310;
    /// The state the transition leads to.
    pub unsafe fn GetDestination(&self) -> *const crate::animation::rule_system::State {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
            ) -> *const crate::animation::rule_system::State = ::std::mem::transmute(
                Self::GetDestination_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
}
impl std::convert::AsRef<Transition> for Transition {
    fn as_ref(&self) -> &Transition {
        self
    }
}
impl std::convert::AsMut<Transition> for Transition {
    fn as_mut(&mut self) -> &mut Transition {
        self
    }
}
#[repr(C, align(8))]
/// The state-machine condition `CValidateStuntPositionCondition`: whether the character can take
/// the stunt position of a vehicle graph node with a given identifier. A helicopter's door-side
/// arrival drops into the helicopter's stunt hang through it once its clip ends; without that
/// transition the door-side state has no way out at the clip's end, and the character stays
/// attached on the skid by the door. Only its evaluation is mapped.
pub struct ValidateStuntPositionCondition {}
impl ValidateStuntPositionCondition {
    pub const Evaluate_ADDRESS: usize = 0x1407D0D80;
    /// Finds the node with the condition's identifier in the graph the character uses or, failing
    /// that, through the available entry whose state is `transition`'s destination; needs the
    /// character's grappling hook, and an attached active wire on the node's graph object; true
    /// when the node's best point is not blocked.
    pub unsafe fn Evaluate(
        &self,
        machine: *const crate::animation::rule_system::StateMachineInstance,
        transition: *const crate::animation::rule_system::Transition,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                machine: *const crate::animation::rule_system::StateMachineInstance,
                transition: *const crate::animation::rule_system::Transition,
            ) -> bool = ::std::mem::transmute(Self::Evaluate_ADDRESS);
            f(self as *const Self as _, machine, transition)
        }
    }
}
impl std::convert::AsRef<ValidateStuntPositionCondition>
for ValidateStuntPositionCondition {
    fn as_ref(&self) -> &ValidateStuntPositionCondition {
        self
    }
}
impl std::convert::AsMut<ValidateStuntPositionCondition>
for ValidateStuntPositionCondition {
    fn as_mut(&mut self) -> &mut ValidateStuntPositionCondition {
        self
    }
}
