//! The binding table tests, split by concern. The shared fixtures live here.

use glam::Vec2;
use jc3gi::input::input_action_map::Action::{self, *};

use super::*;
use crate::controllers::{
    bindings::{controls::HandControls, grab::HolsterSite},
    mode::{OnFootVariant, VehicleSeat},
};

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

fn wants(step: &Step, map: MapKind, action: Action) -> Option<Want> {
    step.desired
        .iter()
        .find(|d| d.target == Target { map, action })
        .map(|d| d.want)
}

fn player(step: &Step, action: Action) -> Option<Want> {
    wants(step, MapKind::Player, action)
}

mod menus;
mod on_foot;
mod vehicles;
