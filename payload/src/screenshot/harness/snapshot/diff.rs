//! The diff pipeline: dispatches `snapshot_diff_cs` over pairs of snapshot planes and reads the
//! cells back. Render thread only, with the engine's immediate context locked.

use anyhow::{Context as _, bail};
use windows::Win32::Graphics::{
    Direct3D11::{
        D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_UNORDERED_ACCESS, D3D11_BOX, D3D11_BUFFER_DESC,
        D3D11_BUFFER_UAV, D3D11_CPU_ACCESS_READ, D3D11_CPU_ACCESS_WRITE, D3D11_MAP_READ,
        D3D11_MAP_WRITE_DISCARD, D3D11_MAPPED_SUBRESOURCE, D3D11_UAV_DIMENSION_BUFFER,
        D3D11_UNORDERED_ACCESS_VIEW_DESC, D3D11_UNORDERED_ACCESS_VIEW_DESC_0, D3D11_USAGE_DYNAMIC,
        D3D11_USAGE_STAGING, ID3D11Buffer, ID3D11ComputeShader, ID3D11Device, ID3D11DeviceContext,
        ID3D11ShaderResourceView, ID3D11UnorderedAccessView,
    },
    Dxgi::Common::DXGI_FORMAT_R32_UINT,
};

use crate::screenshot::harness::snapshot::{
    store::PlaneTexture,
    summary::{CELL_WORDS, Cell, Kind},
};

/// The heat map's cells across and down, for every comparison.
pub(crate) const GRID: [u32; 2] = [32, 32];

/// How many plane comparisons one batch can hold.
pub(crate) const MAX_JOBS: usize = 96;

/// One plane comparison: a region of `a` against the same-sized region of `b`.
pub(crate) struct Job<'a> {
    pub a: &'a PlaneTexture,
    pub b: &'a PlaneTexture,
    pub origin_a: [u32; 2],
    pub origin_b: [u32; 2],
    pub size: [u32; 2],
    pub threshold: f32,
    /// [`Kind::Unorm8`]: how many channels count.
    pub channels: u32,
}

/// The compiled shader and its buffers, made once per device.
pub(crate) struct Pipeline {
    shader: ID3D11ComputeShader,
    params: ID3D11Buffer,
    cells: ID3D11Buffer,
    cells_view: ID3D11UnorderedAccessView,
    staging: ID3D11Buffer,
}

impl Pipeline {
    pub(crate) fn new(device: &ID3D11Device) -> anyhow::Result<Self> {
        let words = (MAX_JOBS * cells_per_job() * CELL_WORDS) as u32;
        // SAFETY: the live device; every descriptor is valid for its call.
        unsafe {
            let mut shader: Option<ID3D11ComputeShader> = None;
            device
                .CreateComputeShader(SHADER, None, Some(&mut shader))
                .context("creating the diff shader")?;
            let shader = shader.context("the diff shader was not created")?;
            let mut params: Option<ID3D11Buffer> = None;
            device
                .CreateBuffer(
                    &D3D11_BUFFER_DESC {
                        ByteWidth: size_of::<Params>() as u32,
                        Usage: D3D11_USAGE_DYNAMIC,
                        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                        ..Default::default()
                    },
                    None,
                    Some(&mut params),
                )
                .context("creating the diff constants")?;
            let params = params.context("the diff constants were not created")?;
            let mut cells: Option<ID3D11Buffer> = None;
            device
                .CreateBuffer(
                    &D3D11_BUFFER_DESC {
                        ByteWidth: words * 4,
                        BindFlags: D3D11_BIND_UNORDERED_ACCESS.0 as u32,
                        ..Default::default()
                    },
                    None,
                    Some(&mut cells),
                )
                .context("creating the diff cells")?;
            let cells = cells.context("the diff cells were not created")?;
            let mut cells_view: Option<ID3D11UnorderedAccessView> = None;
            device
                .CreateUnorderedAccessView(
                    &cells,
                    Some(&D3D11_UNORDERED_ACCESS_VIEW_DESC {
                        Format: DXGI_FORMAT_R32_UINT,
                        ViewDimension: D3D11_UAV_DIMENSION_BUFFER,
                        Anonymous: D3D11_UNORDERED_ACCESS_VIEW_DESC_0 {
                            Buffer: D3D11_BUFFER_UAV {
                                FirstElement: 0,
                                NumElements: words,
                                Flags: 0,
                            },
                        },
                    }),
                    Some(&mut cells_view),
                )
                .context("creating the diff cells view")?;
            let cells_view = cells_view.context("the diff cells view was not created")?;
            let mut staging: Option<ID3D11Buffer> = None;
            device
                .CreateBuffer(
                    &D3D11_BUFFER_DESC {
                        ByteWidth: words * 4,
                        Usage: D3D11_USAGE_STAGING,
                        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                        ..Default::default()
                    },
                    None,
                    Some(&mut staging),
                )
                .context("creating the diff readback")?;
            let staging = staging.context("the diff readback was not created")?;
            Ok(Self {
                shader,
                params,
                cells,
                cells_view,
                staging,
            })
        }
    }

    /// Run `jobs` and return each one's cells, row by row over [`GRID`].
    ///
    /// # Safety
    /// Render thread, with the engine's immediate context locked; every job's planes are live.
    pub(crate) unsafe fn run(
        &self,
        context: &ID3D11DeviceContext,
        jobs: &[Job<'_>],
    ) -> anyhow::Result<Vec<Vec<Cell>>> {
        if jobs.is_empty() {
            return Ok(Vec::new());
        }
        if jobs.len() > MAX_JOBS {
            bail!("{} comparisons exceed the batch of {MAX_JOBS}", jobs.len());
        }
        // SAFETY: per the caller's contract.
        unsafe {
            let mut saved_shader: Option<ID3D11ComputeShader> = None;
            let mut saved_constants: [Option<ID3D11Buffer>; 1] = Default::default();
            let mut saved_resources: [Option<ID3D11ShaderResourceView>; SLOTS] = Default::default();
            let mut saved_uavs: [Option<ID3D11UnorderedAccessView>; 1] = Default::default();
            context.CSGetShader(&mut saved_shader, None, None);
            context.CSGetConstantBuffers(0, Some(&mut saved_constants));
            context.CSGetShaderResources(0, Some(&mut saved_resources));
            context.CSGetUnorderedAccessViews(0, Some(&mut saved_uavs));

            context.CSSetShader(&self.shader, None);
            context.CSSetConstantBuffers(0, Some(&[Some(self.params.clone())]));
            context.CSSetUnorderedAccessViews(0, 1, Some(&Some(self.cells_view.clone())), None);
            let mut failure = None;
            for (index, job) in jobs.iter().enumerate() {
                if let Err(error) = self.dispatch(context, job, index) {
                    failure = Some(error);
                    break;
                }
            }
            let cleared: [Option<ID3D11ShaderResourceView>; SLOTS] = Default::default();
            context.CSSetShaderResources(0, Some(&cleared));
            context.CSSetUnorderedAccessViews(0, 1, Some(saved_uavs.as_ptr()), None);
            context.CSSetConstantBuffers(0, Some(&saved_constants));
            context.CSSetShader(saved_shader.as_ref(), None);
            context.CSSetShaderResources(0, Some(&saved_resources));
            if let Some(error) = failure {
                return Err(error);
            }

            let bytes = (jobs.len() * cells_per_job() * CELL_WORDS * 4) as u32;
            context.CopySubresourceRegion(
                &self.staging,
                0,
                0,
                0,
                0,
                &self.cells,
                0,
                Some(&D3D11_BOX {
                    left: 0,
                    top: 0,
                    front: 0,
                    right: bytes,
                    bottom: 1,
                    back: 1,
                }),
            );
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            context
                .Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .context("mapping the diff readback")?;
            if mapped.pData.is_null() {
                context.Unmap(&self.staging, 0);
                bail!("the diff readback mapped no data");
            }
            let words = std::slice::from_raw_parts(mapped.pData.cast::<u32>(), bytes as usize / 4);
            let result = words
                .chunks_exact(cells_per_job() * CELL_WORDS)
                .map(|job| {
                    job.as_chunks::<CELL_WORDS>()
                        .0
                        .iter()
                        .map(Cell::from_words)
                        .collect()
                })
                .collect();
            context.Unmap(&self.staging, 0);
            Ok(result)
        }
    }

    unsafe fn dispatch(
        &self,
        context: &ID3D11DeviceContext,
        job: &Job<'_>,
        index: usize,
    ) -> anyhow::Result<()> {
        if job.a.format.kind != job.b.format.kind {
            bail!(
                "the planes are read differently ({:?} against {:?})",
                job.a.format.kind,
                job.b.format.kind
            );
        }
        for (plane, origin) in [(job.a, job.origin_a), (job.b, job.origin_b)] {
            if origin[0] + job.size[0] > plane.width || origin[1] + job.size[1] > plane.height {
                bail!(
                    "the compared region {}x{} at {origin:?} leaves a {}x{} plane",
                    job.size[0],
                    job.size[1],
                    plane.width,
                    plane.height
                );
            }
        }
        let kind = job.a.format.kind;
        let has_stencil = job.a.stencil.is_some() && job.b.stencil.is_some();
        let params = Params {
            origin_a: job.origin_a,
            origin_b: job.origin_b,
            size: job.size,
            kind: kind.shader_constant(),
            threshold: job.threshold,
            grid: GRID,
            cell_base: (index * cells_per_job()) as u32,
            channels: job.channels,
            has_stencil: u32::from(has_stencil),
            _pad: [0; 3],
        };
        // SAFETY: per the caller's contract; the constant buffer is ours and sized for `Params`.
        unsafe {
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            context
                .Map(
                    &self.params,
                    0,
                    D3D11_MAP_WRITE_DISCARD,
                    0,
                    Some(&mut mapped),
                )
                .context("mapping the diff constants")?;
            mapped.pData.cast::<Params>().write_unaligned(params);
            context.Unmap(&self.params, 0);
            let mut views: [Option<ID3D11ShaderResourceView>; SLOTS] = Default::default();
            let (a, b) = if kind == Kind::Uint4 { (2, 3) } else { (0, 1) };
            views[a] = Some(job.a.values.clone());
            views[b] = Some(job.b.values.clone());
            if has_stencil {
                views[4] = job.a.stencil.clone();
                views[5] = job.b.stencil.clone();
            }
            context.CSSetShaderResources(0, Some(&views));
            context.Dispatch(GRID[0], GRID[1], 1);
        }
        Ok(())
    }
}

/// The shader's `Params` constant buffer.
#[repr(C)]
#[derive(Clone, Copy)]
struct Params {
    origin_a: [u32; 2],
    origin_b: [u32; 2],
    size: [u32; 2],
    kind: u32,
    threshold: f32,
    grid: [u32; 2],
    cell_base: u32,
    channels: u32,
    has_stencil: u32,
    _pad: [u32; 3],
}

const SLOTS: usize = 6;

const SHADER: &[u8] = include_bytes!("../../../shaders/snapshot_diff_cs.dxbc");

fn cells_per_job() -> usize {
    (GRID[0] * GRID[1]) as usize
}

const _: () = assert!(size_of::<Params>().is_multiple_of(16));
