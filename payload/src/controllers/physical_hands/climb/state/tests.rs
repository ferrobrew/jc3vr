use super::*;
use crate::controllers::physical_hands::climb::engine::BodyIdentity;

fn grip(hand: Hand, body: u32, height: f32) -> Grip {
    Grip {
        hand,
        hold: Hold {
            point: Vec3::Y * height,
            normal: Vec3::Y,
            body,
            moving: true,
            identity: BodyIdentity {
                world: 3,
                bodies: 4,
                owner: body as usize,
                instance: body as usize,
                ..Default::default()
            },
        },
        ledge: true,
        body: Vec3::ZERO,
        palm: Vec3::ZERO,
        pin: (Vec3::ZERO, Quat::IDENTITY),
        frame: Some(Affine3A::IDENTITY),
        rig_epoch: 0,
        missing_steps: 0,
    }
}

#[test]
fn mantle_trajectory_and_handoff_keep_the_captured_owner_when_the_handset_changes() {
    let mut state = STATE.lock();
    let original = state.grips;
    let a = grip(Hand::BOTH[0], 10, 1.0);
    let b = grip(Hand::BOTH[1], 20, 2.0);
    state.grips = [Some(a), None];
    let selected = state.ledge_grip().unwrap();
    let mut captured = MantleFrame {
        owner: selected.hold,
        transform: Affine3A::IDENTITY,
    };
    let context = Context {
        character: 1,
        proxy: 2,
        world: 3,
        bodies: 4,
        epoch: 5,
    };
    let mut mantle = Mantle::start(a.hold.point, Vec3::Z).unwrap();
    for hands in [[Some(a), Some(b)], [None, Some(b)]] {
        state.grips = hands;
        assert_eq!(state.ledge_grip().unwrap().hold.body, b.hold.body);
        let next = Affine3A::from_translation(Vec3::X);
        mantle.top = carried(captured.transform, next, mantle.top);
        captured.transform = next;
        let landing = captured.completion(context, mantle.top);
        assert_eq!(landing.body, a.hold.body);
        assert_eq!(landing.identity, a.hold.identity);
        assert_ne!(landing.identity.owner, b.hold.identity.owner);
    }
    state.grips = original;
}
