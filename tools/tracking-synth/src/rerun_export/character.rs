//! The controller states: Rico's root and motion, the aim targets, the grapple's aim and its line,
//! the vehicle, and the drawn wrists in the world, with their scalars, their state lanes, and the
//! log lines for a standing error that starts or a root move the tick does not explain.

use glam::Vec3;
use rerun::{
    Arrows3D, Clear, Color, LineStrips3D, Points3D, RecordingStreamResult, Rgba32, Scalars,
    StateChange, TextLog, Transform3D, TransformAxes3D, components::TextLogLevel,
};
use tracking_format::{
    event::{ControllerState, GrappleState, VehicleState},
    model::Hand,
};

use crate::rerun_export::{
    Context,
    scene::{ARROW_S, hand_name, rotation},
};

/// Log the stretch's controller states.
pub(crate) fn log(context: &Context<'_>) -> RecordingStreamResult<()> {
    let rec = context.rec;
    log_trail(context)?;
    rec.log_static(
        "world/rico/root",
        &Points3D::new([[0.0, 0.0, 0.0]])
            .with_radii([0.08])
            .with_colors([RICO])
            .with_labels(["Rico"]),
    )?;
    let mut lanes = Lanes::default();
    let mut previous: Option<&ControllerState> = None;
    for &(time_ns, state) in &context.states.all {
        context.set_time(time_ns);
        log_rico(context, state, previous)?;
        log_aim(context, state)?;
        log_grapple(context, state, previous)?;
        log_vehicle(context, state, previous)?;
        log_scalars(context, state)?;
        lanes.log(context, state)?;
        log_changes(context, state, previous)?;
        previous = Some(state);
    }
    Ok(())
}

/// Log `value` on the lane at `path` when it differs from `last`.
pub(crate) fn lane(
    context: &Context<'_>,
    path: &str,
    last: &mut Option<String>,
    value: String,
) -> RecordingStreamResult<()> {
    if last.as_deref() == Some(value.as_str()) {
        return Ok(());
    }
    context
        .rec
        .log(path, &StateChange::single(value.as_str()))?;
    *last = Some(value);
    Ok(())
}

const RICO: Color = Color(Rgba32::from_rgb(255, 140, 0));
const VELOCITY: Color = Color(Rgba32::from_rgb(255, 200, 80));
const WANTED: Color = Color(Rgba32::from_rgb(200, 120, 255));
const AIM: Color = Color(Rgba32::from_rgb(230, 210, 60));
const GRAPPLE_TETHER: Color = Color(Rgba32::from_rgb(80, 220, 80));
const GRAPPLE_VALID: Color = Color(Rgba32::from_rgb(230, 210, 60));
const GRAPPLE_INVALID: Color = Color(Rgba32::from_rgb(220, 60, 60));
const WRIST: Color = Color(Rgba32::from_rgb(250, 250, 250));
const SLING: Color = Color(Rgba32::from_rgb(80, 220, 255));
const VEHICLE_AIM: Color = Color(Rgba32::from_rgb(230, 210, 60));

/// A root move the tick's wanted velocity does not explain beyond this is logged, metres.
const JUMP_LOG_M: f32 = 0.25;

/// The root's path through the stretch, as one static line.
fn log_trail(context: &Context<'_>) -> RecordingStreamResult<()> {
    let mut points: Vec<[f32; 3]> = Vec::new();
    for (_, state) in &context.states.all {
        let Some(root) = &state.root else { continue };
        let point = context.world(root.position);
        if points.last() != Some(&point) {
            points.push(point);
        }
    }
    if points.len() < 2 {
        return Ok(());
    }
    context.rec.log_static(
        "world/rico_trail",
        &LineStrips3D::new([points])
            .with_colors([Color::from_unmultiplied_rgba(255, 140, 0, 120)])
            .with_radii([0.015]),
    )
}

fn log_rico(
    context: &Context<'_>,
    state: &ControllerState,
    previous: Option<&ControllerState>,
) -> RecordingStreamResult<()> {
    let rec = context.rec;
    match &state.root {
        Some(root) => {
            if previous.and_then(|p| p.root.as_ref()).map(|r| r.tick) != Some(root.tick) {
                rec.log(
                    "world/rico",
                    &Transform3D::from_translation(context.world(root.position)),
                )?;
                rec.log(
                    "world/rico/wanted",
                    &Arrows3D::from_vectors([(root.wanted_mps * ARROW_S).to_array()])
                        .with_colors([WANTED]),
                )?;
            }
        }
        None => rec.log("world/rico/wanted", &Clear::flat())?,
    }
    match &state.character {
        Some(character) => rec.log(
            "world/rico/velocity",
            &Arrows3D::from_vectors([(character.velocity_mps * ARROW_S).to_array()])
                .with_colors([VELOCITY]),
        ),
        None => rec.log("world/rico/velocity", &Clear::flat()),
    }
}

fn log_aim(context: &Context<'_>, state: &ControllerState) -> RecordingStreamResult<()> {
    let rec = context.rec;
    for hand in Hand::BOTH {
        let name = hand_name(hand);
        match &state.aim.hands[hand.index()] {
            Some(aim) => rec.log(
                format!("world/aim/{name}"),
                &Points3D::new([context.world(aim.target)])
                    .with_radii([0.03])
                    .with_colors([AIM])
                    .with_labels([format!("{name} {}", aim.source)]),
            )?,
            None => rec.log(format!("world/aim/{name}"), &Clear::flat())?,
        }
        match state.late.hands[hand.index()].latest_wrist {
            Some(wrist) => rec.log(
                format!("world/wrists/{name}"),
                &Points3D::new([context.world(wrist)])
                    .with_radii([0.02])
                    .with_colors([WRIST]),
            )?,
            None => rec.log(format!("world/wrists/{name}"), &Clear::flat())?,
        }
    }
    match &state.aim.grapple {
        Some(grapple) if grapple.hit => {
            let color = match (grapple.valid, grapple.tetherable) {
                (true, true) => GRAPPLE_TETHER,
                (true, false) => GRAPPLE_VALID,
                (false, _) => GRAPPLE_INVALID,
            };
            rec.log(
                "world/grapple",
                &Points3D::new([context.world(grapple.point)])
                    .with_radii([0.05])
                    .with_colors([color])
                    .with_labels(["grapple aim"]),
            )
        }
        _ => rec.log("world/grapple", &Clear::flat()),
    }
}

/// The grapple's line from the device's end to the hook's end, green while the hook's end is fixed
/// and yellow while it is not, and the snatch's held line from its anchor to the target's centre.
fn log_grapple(
    context: &Context<'_>,
    state: &ControllerState,
    previous: Option<&ControllerState>,
) -> RecordingStreamResult<()> {
    let rec = context.rec;
    let Some(grapple) = &state.grapple else {
        if previous.is_some_and(|p| p.grapple.is_some()) {
            rec.log("world/grapple_line", &Clear::recursive())?;
            rec.log("world/grapple_sling", &Clear::recursive())?;
        }
        return Ok(());
    };
    let ends = [
        context.world(grapple.device.position),
        context.world(grapple.hook.position),
    ];
    let color = if grapple.hook.attached {
        GRAPPLE_TETHER
    } else {
        GRAPPLE_VALID
    };
    rec.log(
        "world/grapple_line",
        &LineStrips3D::new([ends])
            .with_colors([color])
            .with_radii([0.01])
            .with_labels([grapple_label(grapple)]),
    )?;
    match &grapple.sling {
        Some(sling) => {
            let points = [context.world(sling.anchor), context.world(sling.centre)];
            rec.log(
                "world/grapple_sling",
                &Points3D::new(points)
                    .with_radii([0.04, 0.05])
                    .with_colors([SLING])
                    .with_labels(["sling anchor", "target centre"]),
            )?;
            rec.log(
                "world/grapple_sling/line",
                &LineStrips3D::new([points])
                    .with_colors([SLING])
                    .with_radii([0.008]),
            )
        }
        None => rec.log("world/grapple_sling", &Clear::recursive()),
    }
}

fn grapple_label(grapple: &GrappleState) -> String {
    format!(
        "hook {} ({}), wire {}",
        grapple.hook_state, grapple.hook.state, grapple.wire_state
    )
}

/// The vehicle's rendered transform with axes, and a manned mounted gun's aim point.
fn log_vehicle(
    context: &Context<'_>,
    state: &ControllerState,
    previous: Option<&ControllerState>,
) -> RecordingStreamResult<()> {
    let rec = context.rec;
    let Some(vehicle) = &state.vehicle else {
        if previous.is_some_and(|p| p.vehicle.is_some()) {
            rec.log("world/vehicle", &Clear::recursive())?;
            rec.log("world/vehicle_aim", &Clear::flat())?;
        }
        return Ok(());
    };
    match vehicle.rendered.or(vehicle.transform) {
        Some(pose) => {
            rec.log(
                "world/vehicle",
                &Transform3D::from_translation_rotation(
                    context.world(pose.position),
                    rotation(pose.orientation),
                ),
            )?;
            rec.log("world/vehicle", &TransformAxes3D::new(1.0))?;
        }
        None => rec.log("world/vehicle", &Clear::recursive())?,
    }
    match vehicle.mounted_aim {
        Some(aim) if vehicle.mounted => rec.log(
            "world/vehicle_aim",
            &Points3D::new([context.world(aim)])
                .with_radii([0.05])
                .with_colors([VEHICLE_AIM])
                .with_labels(["mounted aim"]),
        ),
        _ => rec.log("world/vehicle_aim", &Clear::flat()),
    }
}

/// The vehicle's lane: its class, the player's seat, a manned gun, and what each hand holds.
fn vehicle_lane(vehicle: &VehicleState) -> String {
    let mut lane = format!(
        "{} ({})",
        vehicle.class,
        if vehicle.driver {
            "driving"
        } else {
            "passenger"
        }
    );
    if vehicle.mounted {
        lane.push_str(", mounted gun");
    }
    for hand in Hand::BOTH {
        if let Some(hold) = &vehicle.holds[hand.index()] {
            lane.push_str(&format!(", {} on {hold}", hand_name(hand)));
        }
    }
    lane
}

fn log_scalars(context: &Context<'_>, state: &ControllerState) -> RecordingStreamResult<()> {
    let rec = context.rec;
    let mut scalars: Vec<(String, f64)> = Vec::new();
    let mut push = |name: &str, value: f32| scalars.push((name.to_owned(), f64::from(value)));
    if let Some(age) = state.publication_age_us {
        push("frame/publication_age_ms", age as f32 / 1000.0);
    }
    if let Some(character) = &state.character {
        let v = character.velocity_mps;
        push("rico/speed_mps", v.length());
        push("rico/horizontal_mps", Vec3::new(v.x, 0.0, v.z).length());
        push("rico/vertical_mps", v.y);
        if let Some(height) = character.height {
            push("height/rico_over_ground", height.ground_m);
            push("height/rico_over_nearest", height.nearest_m);
        }
    }
    if let Some(root) = &state.root {
        push("rico/wanted_mps", root.wanted_mps.length());
        push("rico/unexplained_m", root.unexplained_m);
    }
    let roomscale = &state.roomscale;
    push("roomscale/gap_m", roomscale.gap_m);
    push("roomscale/residual_m", roomscale.residual_m);
    push("roomscale/pushback_m", roomscale.pushback_m);
    push("roomscale/eye_drop_m", roomscale.eye_drop_m);
    push("roomscale/fade", roomscale.fade_level);
    for hand in Hand::BOTH {
        let name = hand_name(hand);
        let ik = &state.ik[hand.index()];
        push(&format!("ik/weight/{name}"), ik.weight);
        push(&format!("ik/reach_m/{name}"), ik.reach_distance_m);
        if let Some(displacement) = state.late.hands[hand.index()].physical_displacement_m {
            push(&format!("late/displacement_m/{name}"), displacement);
        }
    }
    if let Some(frame) = &state.frame {
        if let Some(interval) = frame.interval_ms {
            push("frame/interval_ms", interval);
        }
        push("frame/ticks", frame.ticks as f32);
        for hand in Hand::BOTH {
            if let Some(amplitude) = frame.haptics[hand.index()] {
                push(&format!("hands/haptic/{}", hand_name(hand)), amplitude);
            }
        }
    }
    if let Some(grapple) = &state.grapple {
        push(
            "grapple/wire_m",
            grapple.device.position.distance(grapple.hook.position),
        );
        push("grapple/max_length_m", grapple.max_length_m);
        if let Some(sling) = &grapple.sling {
            push("grapple/sling_length_m", sling.length_m);
            push("grapple/tension_n", sling.tension_n);
            push("grapple/max_tension_n", sling.max_tension_n);
        }
        if let Some(lock) = &grapple.lock {
            push("grapple/lock_length_m", lock.length_m);
            push("grapple/lock_stretch_m", lock.stretch_m);
            push("grapple/lock_tension_n", lock.tension_n);
            push("grapple/lock_max_tension_n", lock.max_tension_n);
        }
    }
    if let Some(vehicle) = &state.vehicle {
        if let Some(step) = vehicle.step_m {
            push("vehicle/step_m", step.length());
        }
        if let Some(steering) = vehicle.steering {
            push("vehicle/steering/accelerate", steering.x);
            push("vehicle/steering/brake", steering.y);
            push("vehicle/steering/turn", steering.z);
        }
    }
    for (name, value) in scalars {
        if value.is_finite() {
            rec.log(format!("plots/{name}"), &Scalars::single(value))?;
        }
    }
    Ok(())
}

/// The state lanes the controller states feed, each logged when it changes.
#[derive(Debug, Default)]
struct Lanes {
    mode: Option<String>,
    posture: Option<String>,
    support: Option<String>,
    owner: Option<String>,
    grapple: Option<String>,
    vehicle: Option<String>,
}

impl Lanes {
    fn log(&mut self, context: &Context<'_>, state: &ControllerState) -> RecordingStreamResult<()> {
        let mode = match (&state.traversal, &state.held) {
            (Some(traversal), Some(held)) => format!("{} ({traversal}, {held})", state.mode),
            (Some(refinement), None) | (None, Some(refinement)) => {
                format!("{} ({refinement})", state.mode)
            }
            (None, None) => state.mode.to_string(),
        };
        lane(context, "states/mode", &mut self.mode, mode)?;
        lane(
            context,
            "states/posture",
            &mut self.posture,
            state.roomscale.posture.to_string(),
        )?;
        lane(
            context,
            "states/root_support",
            &mut self.support,
            state
                .root
                .as_ref()
                .map_or("no root tick".to_owned(), |root| root.support.to_string()),
        )?;
        lane(
            context,
            "states/root_owner",
            &mut self.owner,
            state
                .roomscale
                .root_owner
                .as_deref()
                .unwrap_or("none")
                .to_owned(),
        )?;
        // The lanes for the version 8 groups start with the group, so a version 7 recording has
        // none.
        if state.grapple.is_some() || self.grapple.is_some() {
            let value = state
                .grapple
                .as_ref()
                .map_or("no wire".to_owned(), grapple_label);
            lane(context, "states/grapple", &mut self.grapple, value)?;
        }
        if state.vehicle.is_some() || self.vehicle.is_some() {
            let value = state
                .vehicle
                .as_ref()
                .map_or("on foot".to_owned(), vehicle_lane);
            lane(context, "states/vehicle", &mut self.vehicle, value)?;
        }
        Ok(())
    }
}

/// The log lines a controller state's changes call for: a standing error that starts or ends, and
/// a root move the tick's wanted velocity does not explain.
fn log_changes(
    context: &Context<'_>,
    state: &ControllerState,
    previous: Option<&ControllerState>,
) -> RecordingStreamResult<()> {
    let rec = context.rec;
    let errors = [
        (
            "input",
            &state.input_error,
            previous.and_then(|p| p.input_error.as_ref()),
        ),
        (
            "raycast",
            &state.aim.raycast_error,
            previous.and_then(|p| p.aim.raycast_error.as_ref()),
        ),
    ];
    for (what, now, before) in errors {
        if now.as_ref() == before {
            continue;
        }
        match now {
            Some(error) => rec.log(
                "events/errors",
                &TextLog::new(format!("{what} error: {error}")).with_level(TextLogLevel::ERROR),
            )?,
            None => rec.log(
                "events/errors",
                &TextLog::new(format!("{what} error cleared")).with_level(TextLogLevel::INFO),
            )?,
        }
    }
    if let Some(root) = &state.root {
        let jumped = previous
            .and_then(|p| p.root.as_ref())
            .is_some_and(|p| root.jumps > p.jumps);
        if jumped && root.unexplained_m > JUMP_LOG_M {
            rec.log(
                "events/root",
                &TextLog::new(format!(
                    "the root moved {:.2} m that the wanted velocity does not explain ({}, proxy \
                     state {})",
                    root.unexplained_m, root.support, root.proxy_state
                ))
                .with_level(TextLogLevel::WARN),
            )?;
        }
    }
    Ok(())
}
