use super::*;
use crate::controllers::{
    bindings::tables::describe::flight_mechanics, mode::AirborneVariant, summary::labels::ModText,
    vehicles::wingsuit_gestures::GestureAvailability,
};

#[test]
fn current_flight_projects_only_physical_mode_specific_labels() {
    for (mode, availability, labels) in [
        (
            Mode::Airborne(AirborneVariant::Falling),
            GestureAvailability::Deploy,
            vec![ModText::GestureDeploy],
        ),
        (
            Mode::Airborne(AirborneVariant::Wingsuit),
            GestureAvailability::CloseAndBrake,
            vec![ModText::GestureClose, ModText::GestureBrake],
        ),
    ] {
        let mechanics = flight_mechanics(availability);
        let mut snapshot = snapshot([true; 2], [TriggerUse::Free; 2]);
        let absent = eligibility::resolve(&mechanics, &snapshot);
        assert!(absent.context.usable.is_empty());
        snapshot.mechanics = mechanics.clone();
        let admitted = eligibility::resolve(&[], &snapshot);
        assert_eq!(admitted.context.usable, mechanics);
        let current = project(
            &[],
            mode,
            &admitted.context,
            &[],
            &english,
            &|_| panic!("physical gestures have no runtime button name"),
            &|_| None,
        );
        for cell in [CellKind::Left, CellKind::Right, CellKind::ViewMenu] {
            assert!(current.cells[cell.index()].is_empty());
        }
        let hints = &current.cells[CellKind::Available.index()];
        assert_eq!(hints.len(), labels.len());
        for (hint, label) in hints.iter().zip(labels) {
            assert_eq!(hint.text, label.english());
            assert!(hint.prefix.is_none());
            assert_eq!(
                hint.sources,
                vec![Source {
                    control: Control::EitherMotion,
                    name: None
                }]
            );
        }
        assert!(!text(&current).contains("buttonhint_"));
        let unavailable = Eligibility {
            mechanics: flight_mechanics(GestureAvailability::Unavailable),
            ..snapshot
        };
        assert!(
            eligibility::resolve(&mechanics, &unavailable)
                .context
                .usable
                .is_empty()
        );
    }
}

#[test]
fn flight_labels_name_the_bilateral_motion_and_request_scope() {
    assert_eq!(
        ModText::GestureDeploy.english(),
        "Lower Both Arms, Then Hold Spread: Request Wingsuit Deploy"
    );
    assert_eq!(
        ModText::GestureClose.english(),
        "Lower, Then Spread Both Arms to Arm; Hold Both Withdrawn: Request Wingsuit Close"
    );
    assert_eq!(
        ModText::GestureBrake.english(),
        "Lower, Then Spread Both Arms to Arm; Turn Both Palms Forward While Spread: Brake"
    );
}
