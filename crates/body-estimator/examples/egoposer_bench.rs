//! Times one EgoPoser inference: on the CPU reference, single-threaded, and on Windows with the
//! `d3d11` feature also on the GPU executor, with 32-bit and 16-bit weights. For the GPU it reports
//! the GPU time from timestamp queries, the CPU time `submit` takes to record the pass, and how
//! many frames later the estimate can be read back.
//!
//! Usage: `JC3VR_EGOPOSER_WEIGHTS=<weights.safetensors> cargo run --release --example
//! egoposer_bench`. The GPU half wants the real GPU: see `docs/mod/body/body-estimator.md` for
//! running it on Linux through DXVK.

use std::time::{Duration, Instant};

use body_estimator::{EgoPoser, HandVisibility, SparseWindow};
use safetensors::SafeTensors;

const WEIGHTS_VAR: &str = "JC3VR_EGOPOSER_WEIGHTS";
const CPU_ITERATIONS: u32 = 500;

fn main() {
    let path = std::env::var_os(WEIGHTS_VAR)
        .unwrap_or_else(|| panic!("egoposer bench: set {WEIGHTS_VAR} to the weights"));
    let model = EgoPoser::load(&path).unwrap_or_else(|e| panic!("egoposer bench: {e}"));
    let window = fixture_window();

    for _ in 0..50 {
        std::hint::black_box(model.infer(&window));
    }
    let started = Instant::now();
    for _ in 0..CPU_ITERATIONS {
        std::hint::black_box(model.infer(std::hint::black_box(&window)));
    }
    println!(
        "cpu: {:.3} ms per inference",
        ms(started.elapsed() / CPU_ITERATIONS)
    );

    #[cfg(all(windows, feature = "d3d11"))]
    gpu::run(&model, &window);
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

/// The golden fixture's first window.
fn fixture_window() -> SparseWindow {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/egoposer_golden.safetensors"
    ))
    .expect("egoposer bench: the golden fixture");
    let file = SafeTensors::deserialize(&bytes).expect("egoposer bench: the fixture's header");
    let features: Vec<f32> = file
        .tensor("w0.features")
        .expect("egoposer bench: the fixture's first window")
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    SparseWindow {
        features: std::array::from_fn(|f| std::array::from_fn(|k| features[f * 54 + k])),
        visibility: HandVisibility::ALWAYS,
    }
}

#[cfg(all(windows, feature = "d3d11"))]
mod gpu {
    use std::time::{Duration, Instant};

    use body_estimator::{
        EgoPoser, SparseWindow,
        egoposer::gpu::{GpuEgoPoser, WeightPrecision},
    };
    use windows::{
        Win32::{
            Foundation::{HMODULE, S_OK},
            Graphics::{
                Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0},
                Direct3D11::{
                    D3D11_CREATE_DEVICE_FLAG, D3D11_QUERY, D3D11_QUERY_DATA_TIMESTAMP_DISJOINT,
                    D3D11_QUERY_DESC, D3D11_QUERY_TIMESTAMP, D3D11_QUERY_TIMESTAMP_DISJOINT,
                    D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext,
                    ID3D11Query,
                },
                Dxgi::IDXGIDevice,
            },
        },
        core::Interface,
    };

    use crate::ms;

    const ITERATIONS: usize = 200;
    /// The unmeasured inferences ahead of each measured one that keep the GPU busy.
    const LOAD: usize = 20;
    const WARM_UP: Duration = Duration::from_secs(2);
    const FRAMES: usize = 60;
    /// A 90 Hz headset's frame.
    const FRAME: Duration = Duration::from_micros(11_111);

    pub fn run(model: &EgoPoser, window: &SparseWindow) {
        let (device, context) = hardware_device();
        for precision in [WeightPrecision::F32, WeightPrecision::F16] {
            let mut gpu = GpuEgoPoser::new(&device, model, precision)
                .unwrap_or_else(|e| panic!("egoposer bench: {e}"));
            for _ in 0..20 {
                gpu.infer_blocking(&context, window)
                    .unwrap_or_else(|e| panic!("egoposer bench: {e}"));
            }

            let mut idle = time_passes(&device, &context, &mut gpu, window, 0);
            // Ramp the clocks up the way a running game holds them, then keep the GPU busy with
            // unmeasured inferences ahead of each measured one.
            let started = Instant::now();
            while started.elapsed() < WARM_UP {
                for _ in 0..LOAD {
                    gpu.submit(&context, window);
                }
                gpu.infer_blocking(&context, window)
                    .unwrap_or_else(|e| panic!("egoposer bench: {e}"));
            }
            let mut busy = time_passes(&device, &context, &mut gpu, window, LOAD);
            drain(&context, &mut gpu, window);
            println!(
                "gpu, {precision:?} weights: GPU time {} with the GPU busy, {} from idle; CPU time \
                 to record {}",
                busy.gpu.summary(),
                idle.gpu.summary(),
                busy.record.summary(),
            );

            // A frame loop at 90 Hz: each frame collects what it can, then submits.
            let mut latencies = Vec::new();
            let mut first = None;
            for frame in 0..FRAMES as u64 {
                let started = Instant::now();
                while let Some(readback) = gpu
                    .poll(&context)
                    .unwrap_or_else(|e| panic!("egoposer bench: {e}"))
                {
                    let first = first.expect("a readback follows a submission");
                    latencies.push(frame + first - readback.inference);
                }
                let inference = gpu.submit(&context, window);
                first.get_or_insert(inference - frame);
                // SAFETY: the bench's own context.
                unsafe { context.Flush() };
                std::thread::sleep(FRAME.saturating_sub(started.elapsed()));
            }

            latencies.sort();
            println!(
                "gpu, {precision:?} weights: estimates read back {} to {} frames after their \
                 submission at 90 Hz",
                latencies[0],
                latencies[latencies.len() - 1],
            );
        }
    }

    /// Microsecond samples.
    struct Samples(Vec<f64>);

    impl Samples {
        fn summary(&mut self) -> String {
            self.0.sort_by(f64::total_cmp);
            format!(
                "{:.1} us median ({:.1} to {:.1} us over {} runs)",
                self.0[self.0.len() / 2],
                self.0[0],
                self.0[self.0.len() - 1],
                self.0.len()
            )
        }
    }

    struct Timing {
        gpu: Samples,
        record: Samples,
    }

    /// Time `ITERATIONS` passes, each after `load` unmeasured ones, by timestamp queries on the GPU
    /// and by the clock around `submit` on the CPU.
    fn time_passes(
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        gpu: &mut GpuEgoPoser,
        window: &SparseWindow,
        load: usize,
    ) -> Timing {
        let mut timing = Timing {
            gpu: Samples(Vec::with_capacity(ITERATIONS)),
            record: Samples(Vec::with_capacity(ITERATIONS)),
        };
        for _ in 0..ITERATIONS {
            for _ in 0..load {
                gpu.submit(context, window);
            }
            let [disjoint, begin, end] = [
                D3D11_QUERY_TIMESTAMP_DISJOINT,
                D3D11_QUERY_TIMESTAMP,
                D3D11_QUERY_TIMESTAMP,
            ]
            .map(|kind| query(device, kind));
            // SAFETY: the queries and the context are the bench's own.
            unsafe {
                context.Begin(&disjoint);
                context.End(&begin);
            }
            let started = Instant::now();
            gpu.submit(context, window);
            timing.record.0.push(ms(started.elapsed()) * 1e3);
            // SAFETY: as above.
            unsafe {
                context.End(&end);
                context.End(&disjoint);
            }
            let frequency: D3D11_QUERY_DATA_TIMESTAMP_DISJOINT = wait_for(context, &disjoint);
            let (begin, end): (u64, u64) = (wait_for(context, &begin), wait_for(context, &end));
            if !frequency.Disjoint.as_bool() {
                timing
                    .gpu
                    .0
                    .push((end - begin) as f64 / frequency.Frequency as f64 * 1e6);
            }
        }
        timing
    }

    /// Wait for everything submitted and collect it, so the ring holds nothing stale.
    fn drain(context: &ID3D11DeviceContext, gpu: &mut GpuEgoPoser, window: &SparseWindow) {
        gpu.infer_blocking(context, window)
            .unwrap_or_else(|e| panic!("egoposer bench: {e}"));
        while gpu
            .poll(context)
            .unwrap_or_else(|e| panic!("egoposer bench: {e}"))
            .is_some()
        {}
    }

    fn hardware_device() -> (ID3D11Device, ID3D11DeviceContext) {
        let (mut device, mut context) = (None, None);
        // SAFETY: plain device creation with valid out pointers.
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_FLAG(0),
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        }
        .unwrap_or_else(|e| panic!("egoposer bench: no hardware device: {e}"));
        let (device, context): (ID3D11Device, ID3D11DeviceContext) =
            (device.expect("a device"), context.expect("a context"));
        // SAFETY: plain queries on the live device.
        let adapter = unsafe {
            device
                .cast::<IDXGIDevice>()
                .and_then(|dxgi| dxgi.GetAdapter())
                .and_then(|adapter| adapter.GetDesc())
        }
        .map(|desc| {
            let len = desc.Description.iter().position(|&c| c == 0).unwrap_or(128);
            String::from_utf16_lossy(&desc.Description[..len])
        })
        .unwrap_or_else(|e| format!("an unnamed adapter ({e})"));
        println!("gpu: {adapter}");
        (device, context)
    }

    fn query(device: &ID3D11Device, kind: D3D11_QUERY) -> ID3D11Query {
        let mut query = None;
        // SAFETY: plain query creation with a valid descriptor.
        unsafe {
            device.CreateQuery(
                &D3D11_QUERY_DESC {
                    Query: kind,
                    MiscFlags: 0,
                },
                Some(&mut query),
            )
        }
        .unwrap_or_else(|e| panic!("egoposer bench: creating a query: {e}"));
        query.expect("a query")
    }

    /// A query's result, spinning until the GPU has it. The `windows` wrapper folds the
    /// not-ready `S_FALSE` into success, so this calls through the vtable for the raw result.
    fn wait_for<T: Default>(context: &ID3D11DeviceContext, query: &ID3D11Query) -> T {
        let mut data = T::default();
        loop {
            // SAFETY: `data` is the size the query writes; both objects are live.
            let result = unsafe {
                (Interface::vtable(context).GetData)(
                    context.as_raw(),
                    query.as_raw(),
                    (&mut data as *mut T).cast(),
                    size_of::<T>() as u32,
                    0,
                )
            };
            if result == S_OK {
                return data;
            }
            assert!(
                result.is_ok(),
                "egoposer bench: reading a query: {result:?}"
            );
        }
    }
}
