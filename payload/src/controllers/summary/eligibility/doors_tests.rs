use super::*;
use crate::controllers::{grab::PickupKind, summary::current::TriggerUse};

#[test]
fn door_row_requires_exact_current_physical_grip_press_identity() {
    for hand in Hand::BOTH {
        let row = Row {
            control: Control::Grip(hand),
            gesture: Gesture::Press,
            effect: Effect::Mod(ModAction::OpenVehicleDoor),
        };
        let mut snapshot = Eligibility {
            routing: CurrentRouting {
                ready: [true; 2],
                triggers: [TriggerUse::Reserved; 2],
            },
            chosen: Hand::BOTH.map(|h| (h == hand).then_some(GrabVolume::Pickup(PickupKind::Door))),
            ..Default::default()
        };
        let rows = std::slice::from_ref(&row);
        assert!(resolve(rows, &snapshot).context.usable.is_empty());
        for wrong in [
            Row {
                control: Control::A(hand),
                ..row.clone()
            },
            Row {
                control: Control::EitherGrip,
                ..row.clone()
            },
            Row {
                control: Control::Motion(hand),
                ..row.clone()
            },
            Row {
                control: Control::Grip(hand.other()),
                ..row.clone()
            },
            Row {
                gesture: Gesture::Hold,
                ..row.clone()
            },
            Row {
                effect: Effect::Mod(ModAction::Holster),
                ..row.clone()
            },
        ] {
            snapshot.mechanics = vec![wrong];
            assert!(!resolve(rows, &snapshot).context.usable.contains(&row));
        }
        snapshot.mechanics = vec![row.clone()];
        assert_eq!(resolve(&[], &snapshot).context.usable, vec![row.clone()]);
        assert_eq!(resolve(rows, &snapshot).context.usable, vec![row.clone()]);
        snapshot.routing.ready[hand.other().index()] = false;
        assert_eq!(resolve(&[], &snapshot).context.usable, vec![row.clone()]);
        snapshot.routing.ready[hand.index()] = false;
        assert!(resolve(rows, &snapshot).context.usable.is_empty());
        for malformed in [
            Row {
                control: Control::EitherGrip,
                ..row.clone()
            },
            Row {
                control: Control::A(hand),
                ..row.clone()
            },
            Row {
                gesture: Gesture::Hold,
                ..row.clone()
            },
        ] {
            snapshot.routing.ready = [true; 2];
            snapshot.mechanics = vec![malformed.clone()];
            assert!(
                resolve(std::slice::from_ref(&malformed), &snapshot)
                    .context
                    .usable
                    .is_empty()
            );
        }
    }
}
