use super::*;
use crate::controllers::summary::labels::ModText;

#[test]
fn current_door_label_keeps_the_exact_physical_grip_hand() {
    assert_eq!(ModText::OpenVehicleDoor.english(), "Open vehicle door");
    for hand in Hand::BOTH {
        let row = Row {
            control: Control::Grip(hand),
            gesture: Gesture::Press,
            effect: Effect::Mod(ModAction::OpenVehicleDoor),
        };
        let mut snapshot = snapshot([true; 2], [TriggerUse::Reserved; 2]);
        snapshot.mechanics.push(row.clone());
        let admitted = eligibility::resolve(&[], &snapshot);
        assert_eq!(admitted.context.usable, vec![row]);
        let current = project(
            &[],
            MODE,
            &admitted.context,
            &[],
            &english,
            &|_| panic!("a physical grip has no runtime semantic source"),
            &|_| None,
        );
        let cell = match hand {
            Hand::Left => CellKind::Left,
            Hand::Right => CellKind::Right,
        };
        let hints = &current.cells[cell.index()];
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].text, "Open vehicle door");
        assert!(hints[0].prefix.is_none());
        assert_eq!(
            hints[0].sources,
            vec![Source {
                control: Control::Grip(hand),
                name: None
            }]
        );
        assert_eq!(current.cells.iter().map(Vec::len).sum::<usize>(), 1);
        assert!(!text(&current).contains("buttonhint_"));
        snapshot.mechanics.clear();
        let absent = eligibility::resolve(&[], &snapshot);
        let current = project(
            &[],
            MODE,
            &absent.context,
            &[],
            &english,
            &|_| panic!("a physical grip has no runtime semantic source"),
            &|_| None,
        );
        assert!(current.cells.iter().all(Vec::is_empty));
    }
}
