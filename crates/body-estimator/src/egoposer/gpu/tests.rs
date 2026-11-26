//! The GPU pass against the CPU reference, on the golden fixture's windows. Each kernel is checked
//! in isolation (the CPU op fed the GPU's own input to it, so one kernel's error does not hide in
//! another's), and the whole pass stage by stage.
//!
//! The tests need a Direct3D 11 device: a hardware one where there is one, WARP otherwise. Under
//! wine, `./scripts/xwin_test.sh -p body-estimator` runs them on wine's own Direct3D; see
//! `docs/mod/body/body-estimator.md` for running them on the real GPU through DXVK. Like the CPU
//! golden tests, those that need the weights read their path from `JC3VR_EGOPOSER_WEIGHTS` and
//! skip without it.
//!
//! With 16-bit weights, the kernels are checked against the CPU ops on the same rounded weights,
//! which isolates the kernels; how far the rounding itself moves the estimate is reported against
//! the CPU reference on the checkpoint's own weights.

use half::f16;
use windows::{
    Win32::{
        Foundation::HMODULE,
        Graphics::{
            Direct3D::{
                D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP,
                D3D_FEATURE_LEVEL_11_0,
            },
            Direct3D11::{
                D3D11_CREATE_DEVICE_FLAG, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
                ID3D11DeviceContext,
            },
            Dxgi::IDXGIDevice,
        },
    },
    core::Interface,
};

use super::*;
use crate::{
    egoposer::{
        gpu::plan::{LayerTensors, TensorId},
        network::{TOKENS, attend},
        weights::{Decoder, Weights},
    },
    features::HandVisibility,
    fixture::{Fixture, max_abs_diff},
    ops::{Linear, layer_norm, relu},
    smpl::{PELVIS, SPINE3, room_yaw},
};

/// The environment variable naming the weights (a safetensors file).
const WEIGHTS_VAR: &str = "JC3VR_EGOPOSER_WEIGHTS";

/// The largest error tolerated where the GPU and the CPU compute the same thing, relative to the
/// largest magnitude in the reference tensor: summation order and the GPU's transcendentals.
const RELATIVE_TOLERANCE: f64 = 1e-4;

/// The largest yaw error tolerated from 16-bit weights against the checkpoint's own, degrees.
const F16_YAW_TOLERANCE_DEG: f64 = 0.1;

/// The largest error tolerated in a 6D value from 16-bit weights against the checkpoint's own.
const F16_TOLERANCE: f64 = 5e-3;

#[test]
fn every_kernel_matches_the_cpu_op_on_its_own_input() {
    let Some((device, context, model)) = setup() else {
        return;
    };
    let fixture = Fixture::load();
    for precision in [WeightPrecision::F32, WeightPrecision::F16] {
        eprintln!("{precision:?} weights:");
        let weights = reference_weights(&model.weights, precision);
        let mut gpu =
            GpuEgoPoser::new(&device, &model, precision).unwrap_or_else(|e| panic!("{e}"));
        let window = fixture_window(&fixture, 0);
        gpu.infer_blocking(&context, &window)
            .unwrap_or_else(|e| panic!("{e}"));
        let read = |id: TensorId| read_tensor(&device, &context, &gpu, id);

        let tokens = token_inputs(&window);
        let mut embedding = weights.embedding.clone();
        embedding.bias.iter_mut().for_each(|b| *b *= 2.0);
        let mut x = read(gpu.plan.embedded);
        check("  embedding", &x, &embedding.forward(&tokens, TOKENS));

        for (i, (layer, tensors)) in weights.layers.iter().zip(&gpu.plan.layers).enumerate() {
            let LayerTensors {
                projections,
                context: attention_context,
                attended,
                normed,
                hidden,
                fed_forward,
                output,
                queries,
            } = *tensors;
            let width = WIDTH;
            let first = (TOKENS - queries) * width;
            let fused = Linear {
                weight: [&layer.query, &layer.key, &layer.value]
                    .iter()
                    .flat_map(|l| l.weight.iter().copied())
                    .collect(),
                bias: [&layer.query, &layer.key, &layer.value]
                    .iter()
                    .flat_map(|l| l.bias.iter().copied())
                    .collect(),
                inputs: width,
                outputs: 3 * width,
            };
            let gpu_projections = read(projections);
            check(
                &format!("  layer {i} projections"),
                &gpu_projections,
                &fused.forward(&x, TOKENS),
            );

            let part = |k: usize, rows: std::ops::Range<usize>| -> Vec<f32> {
                rows.flat_map(|r| {
                    gpu_projections[(r * 3 + k) * width..(r * 3 + k + 1) * width]
                        .iter()
                        .copied()
                })
                .collect()
            };
            let query = part(0, TOKENS - queries..TOKENS);
            let (key, value) = (part(1, 0..TOKENS), part(2, 0..TOKENS));
            let gpu_context = read(attention_context);
            check(
                &format!("  layer {i} attention"),
                &gpu_context,
                &attend(&query, &key, &value, queries, width),
            );

            let gpu_attended = read(attended);
            check(
                &format!("  layer {i} output projection"),
                &gpu_attended,
                &layer.attention_out.forward(&gpu_context, queries),
            );

            let gpu_normed = read(normed);
            let mut cpu_normed: Vec<f32> = gpu_attended
                .iter()
                .zip(&x[first..])
                .map(|(a, r)| a + r)
                .collect();
            layer_norm(
                &mut cpu_normed,
                width,
                &layer.norm1.gamma,
                &layer.norm1.beta,
            );
            check(&format!("  layer {i} first norm"), &gpu_normed, &cpu_normed);

            let gpu_hidden = read(hidden);
            let mut cpu_hidden = layer.feed_forward1.forward(&gpu_normed, queries);
            relu(&mut cpu_hidden);
            check(
                &format!("  layer {i} feed-forward hidden"),
                &gpu_hidden,
                &cpu_hidden,
            );

            let gpu_fed_forward = read(fed_forward);
            check(
                &format!("  layer {i} feed-forward output"),
                &gpu_fed_forward,
                &layer.feed_forward2.forward(&gpu_hidden, queries),
            );

            let gpu_output = read(output);
            let mut cpu_output: Vec<f32> = gpu_fed_forward
                .iter()
                .zip(&gpu_normed)
                .map(|(f, r)| f + r)
                .collect();
            layer_norm(
                &mut cpu_output,
                width,
                &layer.norm2.gamma,
                &layer.norm2.beta,
            );
            check(
                &format!("  layer {i} second norm"),
                &gpu_output,
                &cpu_output,
            );
            x = gpu_output;
        }

        let estimate = read(gpu.plan.output);
        check(
            "  root orientation decoder",
            &estimate[ROOT_ORIENT_AT..ROOT_ORIENT_AT + 6],
            &decode(&weights.root_orient, &x),
        );
        check(
            "  body pose decoder",
            &estimate[POSE_BODY_AT..POSE_BODY_AT + POSE_BODY],
            &decode(&weights.pose_body, &x),
        );
    }
}

#[test]
fn every_stage_matches_the_cpu_reference() {
    let Some((device, context, model)) = setup() else {
        return;
    };
    let fixture = Fixture::load();
    for precision in [WeightPrecision::F32, WeightPrecision::F16] {
        eprintln!("{precision:?} weights, against the CPU on the same weights:");
        let reference = EgoPoser {
            weights: reference_weights(&model.weights, precision),
        };
        let mut gpu =
            GpuEgoPoser::new(&device, &model, precision).unwrap_or_else(|e| panic!("{e}"));
        let window = fixture_window(&fixture, 0);
        let output = gpu
            .infer_blocking(&context, &window)
            .unwrap_or_else(|e| panic!("{e}"));
        let (expected, trace) = reference.infer_traced(&window);
        let read = |id: TensorId| read_tensor(&device, &context, &gpu, id);
        check("  embedded", &read(gpu.plan.embedded), &trace.embedded);
        for (i, (tensors, layer)) in gpu.plan.layers.iter().zip(&trace.layers).enumerate() {
            check(&format!("  layer {i}"), &read(tensors.output), layer);
        }
        check(
            "  root orientation",
            &output.root_orient,
            &expected.root_orient,
        );
        check("  body pose", &output.pose_body, &expected.pose_body);
    }
}

#[test]
fn every_window_matches_the_cpu_reference() {
    let Some((device, context, model)) = setup() else {
        return;
    };
    let fixture = Fixture::load();
    let (left, right) = (fixture.tensor("w0.fov_l"), fixture.tensor("w0.fov_r"));
    let hidden_hands = HandVisibility {
        left: std::array::from_fn(|i| left[i] > 0.5),
        right: std::array::from_fn(|i| right[i] > 0.5),
    };
    let mut windows: Vec<(String, SparseWindow)> = (0..fixture.windows())
        .map(|w| (format!("window {w}"), fixture_window(&fixture, w)))
        .collect();
    windows.push((
        "window 0 with hidden hands".to_owned(),
        fixture_window(&fixture, 0).with_visibility(hidden_hands),
    ));

    for precision in [WeightPrecision::F32, WeightPrecision::F16] {
        eprintln!("{precision:?} weights, against the CPU on the checkpoint's own weights:");
        let mut gpu =
            GpuEgoPoser::new(&device, &model, precision).unwrap_or_else(|e| panic!("{e}"));
        for (name, window) in &windows {
            let ours = gpu
                .infer_blocking(&context, window)
                .unwrap_or_else(|e| panic!("{e}"));
            let expected = model.infer(window);
            let pelvis = yaw_error_deg(&ours, &expected, PELVIS);
            let chest = yaw_error_deg(&ours, &expected, SPINE3);
            let root = max_abs_diff(&ours.root_orient, &expected.root_orient);
            let pose = max_abs_diff(&ours.pose_body, &expected.pose_body);
            eprintln!(
                "  {name}: max abs error root {root:.2e}, pose {pose:.2e}; yaw error pelvis \
                 {pelvis:.2e} deg, chest {chest:.2e} deg"
            );
            match precision {
                WeightPrecision::F32 => {
                    check(
                        "    root orientation",
                        &ours.root_orient,
                        &expected.root_orient,
                    );
                    check("    body pose", &ours.pose_body, &expected.pose_body);
                }
                WeightPrecision::F16 => {
                    assert!(
                        root < F16_TOLERANCE && pose < F16_TOLERANCE,
                        "{name}: 16-bit weights move the estimate by {root} and {pose}"
                    );
                    assert!(
                        pelvis < F16_YAW_TOLERANCE_DEG && chest < F16_YAW_TOLERANCE_DEG,
                        "{name}: 16-bit weights move the yaws by {pelvis} and {chest} degrees"
                    );
                }
            }
        }
    }
}

#[test]
fn polling_collects_in_order_without_waiting_and_drops_the_overwritten() {
    let Some((device, context, model)) = setup() else {
        return;
    };
    let fixture = Fixture::load();
    let mut gpu =
        GpuEgoPoser::new(&device, &model, WeightPrecision::F32).unwrap_or_else(|e| panic!("{e}"));
    let window = fixture_window(&fixture, 0);
    let expected = gpu
        .infer_blocking(&context, &window)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(gpu.poll(&context).unwrap_or_else(|e| panic!("{e}")), None);

    // One more inference than the ring holds: the first is overwritten before it is collected.
    let submitted: Vec<u64> = (0..=RING).map(|_| gpu.submit(&context, &window)).collect();
    // SAFETY: a plain call on the test's own context.
    unsafe { context.Flush() };
    let mut collected = Vec::new();
    for _ in 0..1_000_000 {
        match gpu.poll(&context).unwrap_or_else(|e| panic!("{e}")) {
            Some(readback) => {
                assert_eq!(readback.output, expected);
                collected.push(readback.inference);
                if collected.len() == RING {
                    break;
                }
            }
            None => std::thread::yield_now(),
        }
    }
    assert_eq!(collected, submitted[1..]);
    assert_eq!(gpu.poll(&context).unwrap_or_else(|e| panic!("{e}")), None);
}

/// A device, its immediate context, and the model, or `None` (with the reason printed) when the
/// weights are not given.
fn setup() -> Option<(ID3D11Device, ID3D11DeviceContext, EgoPoser)> {
    let Some(path) = std::env::var_os(WEIGHTS_VAR) else {
        eprintln!(
            "skipping: set {WEIGHTS_VAR} to EgoPoser's weights (a safetensors file) to run this test"
        );
        return None;
    };
    let model = EgoPoser::load(&path).unwrap_or_else(|e| panic!("{e}"));
    let (device, context) = create_device();
    Some((device, context, model))
}

/// A hardware device where there is one, WARP otherwise.
fn create_device() -> (ID3D11Device, ID3D11DeviceContext) {
    let create = |driver: D3D_DRIVER_TYPE| {
        let (mut device, mut context) = (None, None);
        // SAFETY: plain device creation with valid out pointers.
        unsafe {
            D3D11CreateDevice(
                None,
                driver,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_FLAG(0),
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        }
        .map(|()| (device.expect("a device"), context.expect("a context")))
    };
    let (device, context) = create(D3D_DRIVER_TYPE_HARDWARE)
        .or_else(|hardware| {
            eprintln!("no hardware device ({hardware}); trying WARP");
            create(D3D_DRIVER_TYPE_WARP)
        })
        .unwrap_or_else(|e| panic!("egoposer gpu tests: could not create a device: {e}"));
    // SAFETY: plain queries on the live device.
    let adapter = unsafe {
        device
            .cast::<IDXGIDevice>()
            .and_then(|dxgi| dxgi.GetAdapter())
            .and_then(|adapter| adapter.GetDesc())
    };
    match adapter {
        Ok(desc) => {
            let len = desc.Description.iter().position(|&c| c == 0).unwrap_or(128);
            eprintln!("on {}", String::from_utf16_lossy(&desc.Description[..len]));
        }
        Err(e) => eprintln!("on an adapter that does not name itself ({e})"),
    }
    (device, context)
}

/// The weights the CPU reference uses for `precision`: the checkpoint's, or its matrices rounded
/// to 16 bits as the executor uploads them.
fn reference_weights(weights: &Weights, precision: WeightPrecision) -> Weights {
    let mut weights = weights.clone();
    if precision == WeightPrecision::F16 {
        let round = |layer: &mut Linear| {
            for v in &mut layer.weight {
                *v = f16::from_f32(*v).to_f32();
            }
        };
        round(&mut weights.embedding);
        for layer in &mut weights.layers {
            for linear in [
                &mut layer.query,
                &mut layer.key,
                &mut layer.value,
                &mut layer.attention_out,
                &mut layer.feed_forward1,
                &mut layer.feed_forward2,
            ] {
                round(linear);
            }
        }
        for decoder in [&mut weights.root_orient, &mut weights.pose_body]
            .into_iter()
            .chain(weights.betas.as_mut())
        {
            round(&mut decoder.hidden);
            round(&mut decoder.out);
        }
    }
    weights
}

/// A window of the fixture's own features.
fn fixture_window(fixture: &Fixture, window: usize) -> SparseWindow {
    let features = fixture.tensor(&format!("w{window}.features"));
    SparseWindow {
        features: std::array::from_fn(|f| std::array::from_fn(|k| features[f * 54 + k])),
        visibility: HandVisibility::ALWAYS,
    }
}

/// A tensor's values, copied to the CPU through a staging buffer, waiting for the GPU.
fn read_tensor(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    gpu: &GpuEgoPoser,
    id: TensorId,
) -> Vec<f32> {
    let len = gpu.plan.tensors[id.0];
    let staging = Resources { device }
        .staging("test readback", len * 4)
        .unwrap_or_else(|e| panic!("{e}"));
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    // SAFETY: both buffers are live and equally long; the map waits for the copy.
    unsafe {
        context.CopyResource(&staging, &gpu.tensors[id.0].buffer);
        context
            .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .unwrap_or_else(|e| panic!("egoposer gpu tests: mapping a tensor: {e}"));
        let values = std::slice::from_raw_parts(mapped.pData as *const f32, len).to_vec();
        context.Unmap(&staging, 0);
        values
    }
}

fn decode(decoder: &Decoder, token: &[f32]) -> Vec<f32> {
    let mut hidden = decoder.hidden.forward(token, 1);
    relu(&mut hidden);
    decoder.out.forward(&hidden, 1)
}

fn yaw_error_deg(ours: &EgoPoserOutput, expected: &EgoPoserOutput, joint: usize) -> f64 {
    let (a, b) = (
        room_yaw(ours.global_rotations()[joint]),
        room_yaw(expected.global_rotations()[joint]),
    );
    (a - b).sin().atan2((a - b).cos()).abs().to_degrees()
}

/// Compare `ours` with `reference`, report the errors under `name`, and fail past the tolerance.
fn check(name: &str, ours: &[f32], reference: &[f32]) {
    let error = max_abs_diff(ours, reference);
    let scale = reference
        .iter()
        .map(|v| f64::from(v.abs()))
        .fold(0.0, f64::max)
        .max(1e-6);
    eprintln!(
        "{name}: max abs error {error:.2e}, relative to the largest value {:.2e}",
        error / scale
    );
    assert!(
        error / scale < RELATIVE_TOLERANCE,
        "{name}: max abs error {error} against a scale of {scale}"
    );
}
