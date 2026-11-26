//! A copy of the vegetation's indirect draw arguments with every instance count doubled.
//!
//! The GPU-indirect bark and foliage draws read their arguments from one buffer the vegetation update
//! fills from compute before the frame's passes draw, five dwords per draw, with every pass's draws at
//! their own offsets. The CPU never sees the counts, so a single draw for both eyes cannot double
//! them itself; instead the payload's own compute pass copies the whole buffer, doubling the instance
//! count of every record, and the single draw is issued against the copy at the draw's own offset.
//!
//! The copy is refreshed once per routed pass of each dispatch, at the first draw that needs it, so a
//! buffer the engine rewrote between passes is never read stale. The compute state the pass binds is
//! put back exactly as it was found.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use anyhow::{Context as _, ensure};
use parking_lot::Mutex;
use windows::{
    Win32::Graphics::{
        Direct3D::D3D11_SRV_DIMENSION_BUFFER,
        Direct3D11::{
            D3D11_BIND_SHADER_RESOURCE, D3D11_BIND_UNORDERED_ACCESS, D3D11_BUFFER_DESC,
            D3D11_BUFFER_SRV, D3D11_BUFFER_SRV_0, D3D11_BUFFER_SRV_1, D3D11_BUFFER_UAV,
            D3D11_RESOURCE_MISC_DRAWINDIRECT_ARGS, D3D11_SHADER_RESOURCE_VIEW_DESC,
            D3D11_SHADER_RESOURCE_VIEW_DESC_0, D3D11_UAV_DIMENSION_BUFFER,
            D3D11_UNORDERED_ACCESS_VIEW_DESC, D3D11_UNORDERED_ACCESS_VIEW_DESC_0,
            D3D11_USAGE_DEFAULT, ID3D11Buffer, ID3D11ComputeShader, ID3D11DeviceContext,
            ID3D11ShaderResourceView, ID3D11UnorderedAccessView,
        },
        Dxgi::Common::DXGI_FORMAT_R32_UINT,
    },
    core::Interface,
};

/// The copy of `source` with its instance counts doubled, refreshed for the current pass. `None` when
/// the copy could not be made, which is logged once and makes [`failed`] report it from then on.
pub(super) fn doubled(
    ctx: &ID3D11DeviceContext,
    source: &ID3D11Buffer,
    pass: u8,
) -> Option<ID3D11Buffer> {
    if FAILED.load(Ordering::Relaxed) {
        return None;
    }
    let refreshed_for = (GENERATION.load(Ordering::Relaxed), pass);
    let mut state = STATE.lock();
    let result = (|| -> anyhow::Result<ID3D11Buffer> {
        let fresh = match state.as_ref() {
            Some(copy) if copy.source == source.as_raw() as usize => false,
            _ => {
                *state = Some(Copy::create(ctx, source)?);
                true
            }
        };
        let copy = state.as_mut().expect("just created");
        if fresh || copy.refreshed_for != Some(refreshed_for) {
            copy.refresh(ctx, source);
            copy.refreshed_for = Some(refreshed_for);
        }
        Ok(copy.args.clone())
    })();
    match result {
        Ok(args) => Some(args),
        Err(error) => {
            *state = None;
            if !FAILED.swap(true, Ordering::Relaxed) {
                tracing::warn!(
                    target: "single_pass",
                    "{error:#}; the GPU-indirect vegetation keeps the per-eye re-issue"
                );
            }
            None
        }
    }
}

/// Whether making the copy has failed this session, in which case the blocks with GPU-indirect draws
/// keep the whole-block re-issue.
pub(super) fn failed() -> bool {
    FAILED.load(Ordering::Relaxed)
}

/// Start a dispatch: every copy made before it is stale.
pub(super) fn begin_dispatch() {
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// Drop the copy and the compute shader. Runs on eject, once no draw can reach them.
pub(super) fn release() {
    *STATE.lock() = None;
    FAILED.store(false, Ordering::Relaxed);
}

/// A one-line status for the debug UI.
pub(super) fn status() -> &'static str {
    if failed() {
        "argument copy failed; see the log"
    } else if STATE.lock().is_some() {
        "argument copy live"
    } else {
        "argument copy not made yet"
    }
}

static STATE: Mutex<Option<Copy>> = Mutex::new(None);
static FAILED: AtomicBool = AtomicBool::new(false);
static GENERATION: AtomicU64 = AtomicU64::new(0);

const COMPUTE_DXBC: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/indirect_args_double_cs.dxbc"
));
/// The doubling shader's thread-group width.
const GROUP_SIZE: u32 = 64;

/// The copy of one argument buffer, and what refreshes it.
struct Copy {
    /// The engine buffer the copy is of, as an address; it is only compared.
    source: usize,
    /// The number of dwords in the buffer.
    dwords: u32,
    /// A shader-readable copy of the engine's buffer, which need not be shader-readable itself.
    staging: ID3D11Buffer,
    staging_view: ID3D11ShaderResourceView,
    /// The doubled arguments the single draws read.
    args: ID3D11Buffer,
    args_view: ID3D11UnorderedAccessView,
    shader: ID3D11ComputeShader,
    /// The dispatch generation and pass the copy was last refreshed for.
    refreshed_for: Option<(u64, u8)>,
}

impl Copy {
    fn create(ctx: &ID3D11DeviceContext, source: &ID3D11Buffer) -> anyhow::Result<Self> {
        // SAFETY: both are live COM objects; the getters only read.
        let device = unsafe { ctx.GetDevice() }.context("vegetation once: no device")?;
        let mut desc = D3D11_BUFFER_DESC::default();
        unsafe { source.GetDesc(&mut desc) };
        ensure!(
            desc.MiscFlags & D3D11_RESOURCE_MISC_DRAWINDIRECT_ARGS.0 as u32 != 0,
            "vegetation once: the indirect draw's buffer is not an argument buffer"
        );
        let dwords = desc.ByteWidth / 4;
        ensure!(
            dwords > 0,
            "vegetation once: the indirect argument buffer is empty"
        );
        let buffer = |bind: u32, misc: u32| -> anyhow::Result<ID3D11Buffer> {
            let desc = D3D11_BUFFER_DESC {
                ByteWidth: dwords * 4,
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: bind,
                CPUAccessFlags: 0,
                MiscFlags: misc,
                StructureByteStride: 0,
            };
            let mut buffer = None;
            // SAFETY: a plain default-usage buffer with no initial data.
            unsafe { device.CreateBuffer(&desc, None, Some(&mut buffer)) }
                .context("vegetation once: creating an argument copy buffer")?;
            buffer.context("vegetation once: no argument copy buffer")
        };
        let staging = buffer(D3D11_BIND_SHADER_RESOURCE.0 as u32, 0)?;
        let args = buffer(
            D3D11_BIND_UNORDERED_ACCESS.0 as u32,
            D3D11_RESOURCE_MISC_DRAWINDIRECT_ARGS.0 as u32,
        )?;
        let srv_desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
            Format: DXGI_FORMAT_R32_UINT,
            ViewDimension: D3D11_SRV_DIMENSION_BUFFER,
            Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                Buffer: D3D11_BUFFER_SRV {
                    Anonymous1: D3D11_BUFFER_SRV_0 { FirstElement: 0 },
                    Anonymous2: D3D11_BUFFER_SRV_1 {
                        NumElements: dwords,
                    },
                },
            },
        };
        let mut staging_view = None;
        // SAFETY: a typed view over the whole staging buffer.
        unsafe {
            device.CreateShaderResourceView(&staging, Some(&srv_desc), Some(&mut staging_view))
        }
        .context("vegetation once: creating the argument copy's source view")?;
        let uav_desc = D3D11_UNORDERED_ACCESS_VIEW_DESC {
            Format: DXGI_FORMAT_R32_UINT,
            ViewDimension: D3D11_UAV_DIMENSION_BUFFER,
            Anonymous: D3D11_UNORDERED_ACCESS_VIEW_DESC_0 {
                Buffer: D3D11_BUFFER_UAV {
                    FirstElement: 0,
                    NumElements: dwords,
                    Flags: 0,
                },
            },
        };
        let mut args_view = None;
        // SAFETY: a typed view over the whole argument buffer.
        unsafe { device.CreateUnorderedAccessView(&args, Some(&uav_desc), Some(&mut args_view)) }
            .context("vegetation once: creating the argument copy's view")?;
        let mut shader = None;
        // SAFETY: the committed bytecode of the doubling shader.
        unsafe { device.CreateComputeShader(COMPUTE_DXBC, None, Some(&mut shader)) }
            .context("vegetation once: creating the doubling shader")?;
        Ok(Self {
            source: source.as_raw() as usize,
            dwords,
            staging,
            staging_view: staging_view.context("vegetation once: no source view")?,
            args,
            args_view: args_view.context("vegetation once: no argument view")?,
            shader: shader.context("vegetation once: no doubling shader")?,
            refreshed_for: None,
        })
    }

    /// Copy the engine's arguments and double their instance counts into [`args`](Self::args),
    /// putting back the compute shader, the first shader resource, and the first unordered access
    /// view it binds.
    fn refresh(&self, ctx: &ID3D11DeviceContext, source: &ID3D11Buffer) {
        let mut shader: Option<ID3D11ComputeShader> = None;
        let mut resource: [Option<ID3D11ShaderResourceView>; 1] = [None];
        let mut access: [Option<ID3D11UnorderedAccessView>; 1] = [None];
        // SAFETY: the immediate context, on the draw thread that owns it for this draw; every binding
        // read here is put back below.
        unsafe {
            ctx.CSGetShader(&mut shader, None, None);
            ctx.CSGetShaderResources(0, Some(&mut resource));
            ctx.CSGetUnorderedAccessViews(0, Some(&mut access));
            ctx.CopySubresourceRegion(&self.staging, 0, 0, 0, 0, source, 0, None);
            ctx.CSSetShader(&self.shader, None);
            ctx.CSSetShaderResources(0, Some(&[Some(self.staging_view.clone())]));
            ctx.CSSetUnorderedAccessViews(0, 1, Some(&Some(self.args_view.clone())), None);
            ctx.Dispatch(self.dwords.div_ceil(GROUP_SIZE), 1, 1);
            ctx.CSSetUnorderedAccessViews(0, 1, Some(&access[0]), None);
            ctx.CSSetShaderResources(0, Some(&resource));
            ctx.CSSetShader(shader.as_ref(), None);
        }
    }
}
