use super::*;

#[test]
fn stale_world_and_missing_or_reused_slots_never_dispatch_wrapper_operations() {
    let wrapper = NonNull::dangling();
    let mut slot: hknpBody = unsafe { std::mem::zeroed() };
    slot.m_motionId = 1;
    let mut world: hknpWorld = unsafe { std::mem::zeroed() };
    world.m_bodies = &mut slot;
    world.m_motions = NonNull::dangling().as_ptr();
    let mut body = HandBody {
        wrapper,
        body_id: 0,
        spec: BodySpec {
            shape: BodyShape::Capsule(Capsule {
                half_length_m: 0.05,
                radius_m: 0.03,
            }),
            mass_kg: 1.0,
            system_group: 1,
            contact_events: false,
            gravity: false,
        },
        source: crate::controllers::frame::Hand::Left.into(),
        world: 0,
    };
    for operation in ["is_added", "add", "remove", "transform"] {
        for failure in ["world", "slot", "instance"] {
            body.world = if failure == "world" {
                0
            } else {
                &world as *const hknpWorld as usize
            };
            unsafe {
                (*world.m_bodies).m_motionId = if failure == "slot" {
                    hknpBody::INVALID_MOTION_ID
                } else {
                    1
                }
            };
            let mut calls = Vec::new();
            let result = checked_call(
                || unsafe { body.slot_with(&world, |_, _| Some(99)) },
                |_| calls.push(operation),
            );
            assert!(matches!(result, Err(BodyError::Lost)));
            assert!(calls.is_empty(), "{failure}: {operation}");
        }
    }
    unsafe { (*world.m_bodies).m_motionId = 1 };
    assert!(unsafe { body.slot_with(&world, |_, _| Some(wrapper.as_ptr() as usize)) }.is_ok());
}
