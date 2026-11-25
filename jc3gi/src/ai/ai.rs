#![cfg_attr(any(), rustfmt::skip)]
#[repr(u32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// The kind of an AI awareness event (`EAwareness`), the first argument of
/// [`BroadcastAwarenessEvent`](crate::ai::ai::BroadcastAwarenessEvent). Only established event kinds are mapped.
pub enum AwarenessEvent {
    EAwareness_WeaponFire = 4isize as _,
    EAwareness_SawGrenade = 14isize as _,
}
fn _AwarenessEvent_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], AwarenessEvent>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(4))]
/// An awareness record (`SAwarenessEvent`) consumed by the AI senses. Grenade records carry
/// their predicted landing position and ammunition-pool allocation identity in the event data.
pub struct AwarenessRecord {
    pub m_Kind: crate::ai::ai::AwarenessEvent,
    pub m_Position: crate::types::math::Vector3,
    pub m_Source: u32,
    pub m_GrenadeLanding: crate::types::math::Vector3,
    pub m_GrenadePoolIndex: u32,
    pub m_GrenadeInstanceUniqueId: u32,
    _field_28: [u8; 44],
}
fn _AwarenessRecord_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x54], AwarenessRecord>([0u8; 0x54]);
    }
    unreachable!()
}
impl AwarenessRecord {}
impl std::convert::AsRef<AwarenessRecord> for AwarenessRecord {
    fn as_ref(&self) -> &AwarenessRecord {
        self
    }
}
impl std::convert::AsMut<AwarenessRecord> for AwarenessRecord {
    fn as_mut(&mut self) -> &mut AwarenessRecord {
        self
    }
}
#[repr(C, align(8))]
/// An AI entity's awareness sense (`CAISenseAwareness`).
pub struct SenseAwareness {}
impl SenseAwareness {
    pub const GrenadeCheck_ADDRESS: usize = 0x140647360;
    /// Tests a grenade awareness event against the predicted landing distance and current
    /// position, then vision when needed. It does not check whether the grenade's fuse is running.
    pub unsafe fn GrenadeCheck(
        &self,
        event: *const crate::ai::ai::AwarenessRecord,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                event: *const crate::ai::ai::AwarenessRecord,
            ) -> bool = ::std::mem::transmute(Self::GrenadeCheck_ADDRESS);
            f(self as *const Self as _, event)
        }
    }
}
impl std::convert::AsRef<SenseAwareness> for SenseAwareness {
    fn as_ref(&self) -> &SenseAwareness {
        self
    }
}
impl std::convert::AsMut<SenseAwareness> for SenseAwareness {
    fn as_mut(&mut self) -> &mut SenseAwareness {
        self
    }
}
pub const AIMechFireGravityGun_ADDRESS: usize = 0x1406231E0;
/// A behavior-tree action callback (`AIMechFireGravityGun`), dispatched from
/// `BtCallbackHandler`: fires the gravity gun (the magnet weapon component) of the mech the AI
/// entity drives. It picks nearby physics objects and bodies to attract (restricting the magnet
/// component to them) using perception, faction relations, and the mech's turret yaw. `tree` is the
/// `STreeInstance`, `registry` the `SAiEntityRegistry`, and `target` the target position; the
/// return value is the behavior-tree node result.
pub unsafe fn AIMechFireGravityGun(
    p0: u32,
    p1: *mut ::std::ffi::c_void,
    tree: *mut ::std::ffi::c_void,
    registry: *mut ::std::ffi::c_void,
    target: *const crate::types::math::Vector3,
    p5: f32,
    p6: f32,
) -> u32 {
    unsafe {
        let f: unsafe extern "system" fn(
            p0: u32,
            p1: *mut ::std::ffi::c_void,
            tree: *mut ::std::ffi::c_void,
            registry: *mut ::std::ffi::c_void,
            target: *const crate::types::math::Vector3,
            p5: f32,
            p6: f32,
        ) -> u32 = ::std::mem::transmute(AIMechFireGravityGun_ADDRESS);
        f(p0, p1, tree, registry, target, p5, p6)
    }
}
pub const AiGetClosestEntityInRange_ADDRESS: usize = 0x1405F97F0;
/// `_AiGetClosestEntityInRange`: finds the AI entity of `entity_type` (1-based category) closest
/// to `point` within `range`, accepted by `faction` (and, with `alive_only`, alive), skipping
/// `ignore_entity` and the entity owned by `owner_go`. Writes the found entity handle to
/// `found_entity` (0 when none) and returns whether one was found. IDA labels this
/// address `CAIPerception::GetCharacterWithMentalState`; it is `_AiGetClosestEntityInRange`.
pub unsafe fn AiGetClosestEntityInRange(
    point: *const crate::types::math::Vector3,
    entity_type: u32,
    faction: i32,
    alive_only: u32,
    range: f32,
    ignore_entity: u32,
    found_entity: *mut u32,
    registry: *mut ::std::ffi::c_void,
    owner_go: *mut ::std::ffi::c_void,
) -> bool {
    unsafe {
        let f: unsafe extern "system" fn(
            point: *const crate::types::math::Vector3,
            entity_type: u32,
            faction: i32,
            alive_only: u32,
            range: f32,
            ignore_entity: u32,
            found_entity: *mut u32,
            registry: *mut ::std::ffi::c_void,
            owner_go: *mut ::std::ffi::c_void,
        ) -> bool = ::std::mem::transmute(AiGetClosestEntityInRange_ADDRESS);
        f(
            point,
            entity_type,
            faction,
            alive_only,
            range,
            ignore_entity,
            found_entity,
            registry,
            owner_go,
        )
    }
}
pub const BroadcastAwarenessEvent_ADDRESS: usize = 0x140646EF0;
/// Queues an AI awareness event of `kind` at `position` from the AI entity `source`, with `size`
/// bytes of event data copied from `data`: it appends one record to a fixed-capacity buffer
/// through an atomic counter, so it is safe from the parallel update passes, and drops the event
/// once the buffer is full. The disassembler mislabels it `ProfilerThreadAddBudget`.
pub unsafe fn BroadcastAwarenessEvent(
    kind: crate::ai::ai::AwarenessEvent,
    position: *const crate::types::math::Vector3,
    source: u32,
    data: *const ::std::ffi::c_void,
    size: u64,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            kind: crate::ai::ai::AwarenessEvent,
            position: *const crate::types::math::Vector3,
            source: u32,
            data: *const ::std::ffi::c_void,
            size: u64,
        ) = ::std::mem::transmute(BroadcastAwarenessEvent_ADDRESS);
        f(kind, position, source, data, size)
    }
}
