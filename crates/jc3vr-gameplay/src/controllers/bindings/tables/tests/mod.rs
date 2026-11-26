//! The binding table tests, split by concern. The shared fixtures live here.

use glam::Vec2;
use jc3gi::input::input_action_map::Action::{self, *};
use jc3vr_body::controllers::{
    intent::{
        config::{BindingsConfig, long_press_s, menu_stick_push},
        controls::HandControls,
    },
    mode::{OnFootVariant, VehicleSeat},
};
use jc3vr_input::semantic::SemanticSet;
use jc3vr_view::vr::input::semantic::defaults::default_binding;
use jc3vr_weapons::controllers::loadout::Socket;

use super::*;
use crate::controllers::bindings::intents::simulate::simulate;

const DT: f32 = 1.0 / 30.0;
const CONFIG: BindingsConfig = BindingsConfig::new();
const ON_FOOT: Mode = Mode::OnFoot(OnFootVariant::Ground);

fn down() -> ButtonControl {
    ButtonControl {
        down: true,
        pressed: true,
        released: false,
    }
}

fn up() -> ButtonControl {
    ButtonControl {
        down: false,
        pressed: false,
        released: true,
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
    fn sim_step(&mut self, mode: Mode, controls: &Controls, dt: f32) -> Step {
        let input = simulate(
            controls,
            SemanticSet::of(mode.action_set()),
            default_binding,
        );
        self.step(mode, controls, &input, dt)
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
mod snatch;
mod vehicles;
