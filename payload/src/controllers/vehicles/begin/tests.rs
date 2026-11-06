use super::*;
use crate::controllers::{
    mode::VehicleClass,
    vehicles::physical::{ControlKind, ControlState, Hold, Outputs, Release},
};

fn driver() -> Option<VehicleSeat> {
    Some(VehicleSeat {
        class: VehicleClass::Car,
        driver: true,
    })
}

fn hold_entry(state: &mut State, identity: VehicleEntryIdentity) {
    state.entry = Some(identity);
    state.status.seat = driver();
    let hold = Hold {
        kind: ControlKind::Wheel,
        offset: Vec3::ZERO,
        orientation: None,
    };
    state.controls.holds = [Some(hold), None];
    state.controls.wheel_angle = 0.7;
    state.controls.levers = [0.5, 0.8];
    state.controls.buttons.phases = [[PushPhase::Pressed; 2]; 2];
    state.last_holds = [Some(hold), None];
    state.anchor_weights = [1.0, 0.0];
    state.latch = Some(VehicleLatch::new());
    state.afterburner = true;
    state.status.outputs.steer = Some(0.9);
}

#[test]
fn begin_adapter_releases_same_class_replacements_before_fit_and_offers() {
    let a = VehicleEntryIdentity::fixture(1, 10);
    for b in [
        VehicleEntryIdentity::fixture(2, 20),
        VehicleEntryIdentity::fixture(1, 20),
    ] {
        let mut state = State::new();
        hold_entry(&mut state, a);
        let mut events = Vec::new();
        assert!(prepare_entry(&mut state, driver(), Some(b), &mut events).is_none());
        assert_eq!(
            events,
            [ControlEvent::Released(
                Hand::Left,
                ControlKind::Wheel,
                Release::Gone
            )]
        );
        assert_eq!(state.controls, ControlState::default());
        assert_eq!(state.last_holds, [None; 2]);
        assert_eq!(state.anchor_weights, [0.0; 2]);
        assert_eq!(state.status.outputs, Outputs::NONE);
        assert!(!state.afterburner);
        assert_eq!(state.entry, Some(b));
        assert!(state.placement.is_none());
        assert!(state.render.is_none());
    }
}

#[test]
fn begin_adapter_missing_observation_clears_old_entry_and_reentry_is_fresh() {
    let mut state = State::new();
    hold_entry(&mut state, VehicleEntryIdentity::fixture(1, 10));
    let mut events = Vec::new();
    assert!(prepare_entry(&mut state, driver(), None, &mut events).is_none());
    assert_eq!(state.entry, None);
    assert_eq!(state.controls, ControlState::default());
    assert_eq!(state.last_holds, [None; 2]);
    let config = ControllersConfig::default();
    let xf = CockpitTransform {
        head_world: Vec3::ZERO,
        head_cockpit: Vec3::ZERO,
        body: glam::Quat::IDENTITY,
        world_scale: 1.0,
    };
    let fitted = fit::fit(
        &FitInputs {
            physical: &PhysicalControlsTuning::of(&config.vehicles.physical),
            view_enabled: config.vehicles.driving_view.enabled,
            seat: driver(),
            xf: &xf,
            eyes: None,
        },
        None,
        &mut state.latch,
    );
    assert!(fitted.readout.is_none());
    assert!(fitted.view.is_none());
    assert!(state.latch.is_none());
    let b = VehicleEntryIdentity::fixture(2, 20);
    assert!(prepare_entry(&mut state, driver(), Some(b), &mut events).is_none());
    assert_eq!(state.entry, Some(b));
    assert_eq!(events.len(), 1);
}

#[test]
fn begin_adapter_keeps_same_entry_and_resets_a_seat_change() {
    let mut state = State::new();
    let identity = VehicleEntryIdentity::fixture(1, 10);
    hold_entry(&mut state, identity);
    let mut events = Vec::new();
    let latch = prepare_entry(&mut state, driver(), Some(identity), &mut events);
    assert!(latch.is_some());
    assert!(state.controls.holds[0].is_some());
    assert!(events.is_empty());
    state.latch = latch;
    let passenger = Some(VehicleSeat {
        class: VehicleClass::Car,
        driver: false,
    });
    assert!(prepare_entry(&mut state, passenger, Some(identity), &mut events).is_none());
    assert_eq!(state.controls, ControlState::default());
    assert_eq!(state.anchor_weights, [0.0; 2]);
}
