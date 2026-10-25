//! The binding table tests, split by concern. The shared fixtures live here.

use glam::Vec2;
use jc3gi::input::input_action_map::Action::{self, *};

use super::*;
use crate::{
    controllers::{
        bindings::{controls::HandControls, intents::simulate::simulate},
        loadout::Socket,
        mode::{OnFootVariant, VehicleSeat},
    },
    vr::input::semantic::{SemanticSet, defaults::default_binding},
};

const DT: f32 = 1.0 / 30.0;
/// The table inputs are already accepted; chord timing does not change their interpretation.
const CONFIG: BindingsConfig = BindingsConfig {
    stick_chord_window_s: 0.0,
    panel_chord_window_s: 0.0,
    ..BindingsConfig::new()
};
/// The default timing also leaves already-accepted table inputs unchanged.
const CHORD_CONFIG: BindingsConfig = BindingsConfig::new();
const ON_FOOT: Mode = Mode::OnFoot(OnFootVariant::Ground);

fn down() -> ButtonControl {
    ButtonControl {
        down: true,
        pressed: true,
        released: false,
    }
}

fn controls(dominant: Hand, set: impl FnOnce(&mut HandControls, &mut HandControls)) -> Controls {
    let mut c = Controls {
        dominant,
        ..Default::default()
    };
    let [left, right] = &mut c.hands;
    let (dom, off) = match dominant {
        Hand::Right => (right, left),
        Hand::Left => (left, right),
    };
    set(dom, off);
    c
}

impl Bindings {
    /// Step the tables with the semantic input the shipped default bindings make of `controls`.
    fn sim_step(
        &mut self,
        mode: Mode,
        controls: &Controls,
        dt: f32,
        config: &BindingsConfig,
    ) -> Step {
        let input = simulate(
            controls,
            SemanticSet::of(mode.action_set()),
            default_binding,
        );
        self.step(mode, controls, &input, dt, config)
    }
}

fn wants(step: &Step, map: MapKind, action: Action) -> Option<Want> {
    step.desired
        .iter()
        .find(|d| d.target == Target { map, action })
        .map(|d| d.want)
}

fn player(step: &Step, action: Action) -> Option<Want> {
    wants(step, MapKind::Player, action)
}

mod equivalence;
mod layout;
pub(crate) mod legacy;
mod menus;
mod on_foot;
mod vehicles;
