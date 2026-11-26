//! EgoPoser's forward pass on the GPU, through Direct3D 11 compute shaders, so the game's own device
//! can run it without costing the CPU the reference pass's milliseconds.
//!
//! [`GpuEgoPoser::new`] uploads the weights once, as 32-bit or 16-bit floats
//! ([`WeightPrecision`]). Each inference then goes in three steps that never wait on the GPU:
//! [`GpuEgoPoser::submit`] uploads the window's tokens and records the pass on a device context,
//! ending with a copy of the estimate into a staging buffer from a small ring; a frame or more
//! later, [`GpuEgoPoser::poll`] maps the oldest staging buffer without waiting and returns its
//! estimate once the GPU has finished it. [`GpuEgoPoser::infer_blocking`] waits instead, for tests
//! and tools.
//!
//! The pass is four kernels (`src/shaders/`, compiled by `shadergen` into committed `.dxbc`): a
//! linear layer (in a 32-bit and a 16-bit weight variant), a residual add and layer norm, and the
//! multi-head attention. The `plan` module lays out the dispatches; every activation has a buffer
//! of its own, so any stage can be read back and checked against the CPU reference.
//!
//! The executor puts back the compute state it changes (the shader, the first constant buffer, the
//! first three shader resources, and the first unordered access view), so it can record onto a
//! context the engine shares.

mod plan;

use std::fmt;

use half::f16;
use windows::{
    Win32::Graphics::{
        Direct3D::{D3D_FEATURE_LEVEL_11_0, D3D11_SRV_DIMENSION_BUFFEREX},
        Direct3D11::{
            D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_SHADER_RESOURCE, D3D11_BIND_UNORDERED_ACCESS,
            D3D11_BUFFER_DESC, D3D11_BUFFER_UAV, D3D11_BUFFER_UAV_FLAG_RAW, D3D11_BUFFEREX_SRV,
            D3D11_BUFFEREX_SRV_FLAG_RAW, D3D11_CPU_ACCESS_READ, D3D11_MAP_FLAG_DO_NOT_WAIT,
            D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_RESOURCE_MISC_BUFFER_ALLOW_RAW_VIEWS,
            D3D11_SHADER_RESOURCE_VIEW_DESC, D3D11_SHADER_RESOURCE_VIEW_DESC_0,
            D3D11_SUBRESOURCE_DATA, D3D11_UAV_DIMENSION_BUFFER, D3D11_UNORDERED_ACCESS_VIEW_DESC,
            D3D11_UNORDERED_ACCESS_VIEW_DESC_0, D3D11_USAGE_DEFAULT, D3D11_USAGE_IMMUTABLE,
            D3D11_USAGE_STAGING, ID3D11Buffer, ID3D11ComputeShader, ID3D11Device,
            ID3D11DeviceContext, ID3D11ShaderResourceView, ID3D11UnorderedAccessView,
        },
        Dxgi::{Common::DXGI_FORMAT_R32_TYPELESS, DXGI_ERROR_WAS_STILL_DRAWING},
    },
    core::Error as WindowsError,
};

use crate::{
    egoposer::{
        EgoPoser, EgoPoserOutput,
        gpu::plan::{BETAS_AT, Binding, Kernel, POSE_BODY_AT, Plan, ROOT_ORIENT_AT, WIDTH},
        network::token_inputs,
        weights::{BETAS, POSE_BODY},
    },
    features::SparseWindow,
};

/// How the weight matrices are stored on the GPU. The biases, the norms, and every activation are
/// 32-bit either way, and so is the arithmetic.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WeightPrecision {
    /// As the checkpoint stores them.
    F32,
    /// Rounded to the nearest 16-bit float when uploaded: half the memory and bandwidth.
    F16,
}

/// Why the executor could not be created, or a result could not be read.
#[derive(Debug)]
pub enum GpuError {
    /// The checkpoint's model width is not the one the kernels are written for.
    Width { found: usize },
    CreateBuffer {
        buffer: &'static str,
        source: WindowsError,
    },
    CreateView {
        buffer: &'static str,
        source: WindowsError,
    },
    CreateShader {
        shader: &'static str,
        source: WindowsError,
    },
    /// A staging buffer could not be mapped for a reason other than the GPU still working on it.
    Map { source: WindowsError },
}

impl fmt::Display for GpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "egoposer gpu: ")?;
        match self {
            GpuError::Width { found } => write!(
                f,
                "the checkpoint's model width is {found}, but the kernels are written for {WIDTH}"
            ),
            GpuError::CreateBuffer { buffer, source } => {
                write!(f, "could not create the {buffer} buffer: {source}")
            }
            GpuError::CreateView { buffer, source } => {
                write!(
                    f,
                    "could not create a view of the {buffer} buffer: {source}"
                )
            }
            GpuError::CreateShader { shader, source } => {
                write!(f, "could not create the {shader} compute shader: {source}")
            }
            GpuError::Map { source } => write!(f, "could not map a staging buffer: {source}"),
        }
    }
}

impl std::error::Error for GpuError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            GpuError::Width { .. } => None,
            GpuError::CreateBuffer { source, .. }
            | GpuError::CreateView { source, .. }
            | GpuError::CreateShader { source, .. }
            | GpuError::Map { source } => Some(source),
        }
    }
}

/// A finished inference, read back.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Readback {
    pub output: EgoPoserOutput,
    /// The number [`GpuEgoPoser::submit`] gave the inference.
    pub inference: u64,
}

/// The network's weights and activations on a device, and the recorded pass.
pub struct GpuEgoPoser {
    steps: Vec<Step>,
    tensors: Vec<Tensor>,
    plan: Plan,
    estimates_shape: bool,
    ring: [Slot; RING],
    /// The ring slot the next inference copies into.
    next: usize,
    /// How many inferences have been submitted.
    submitted: u64,
}

impl GpuEgoPoser {
    /// Upload `model`'s weights to `device` in `precision` and prepare the pass.
    ///
    /// # Panics
    ///
    /// When `device` cannot run compute shaders: below feature level 11_0, which no machine that
    /// runs the game is.
    pub fn new(
        device: &ID3D11Device,
        model: &EgoPoser,
        precision: WeightPrecision,
    ) -> Result<GpuEgoPoser, GpuError> {
        // SAFETY: a plain query on a live device.
        let level = unsafe { device.GetFeatureLevel() };
        assert!(
            level.0 >= D3D_FEATURE_LEVEL_11_0.0,
            "egoposer gpu: the device's feature level ({:#x}) has no compute shaders; the game needs \
             feature level 11_0",
            level.0
        );
        let weights = &model.weights;
        if weights.width != WIDTH {
            return Err(GpuError::Width {
                found: weights.width,
            });
        }
        let plan = Plan::new(weights);
        let resources = Resources { device };

        let matrix_bytes: Vec<u8> = match precision {
            WeightPrecision::F32 => plan.matrices.iter().flat_map(|v| v.to_le_bytes()).collect(),
            WeightPrecision::F16 => plan
                .matrices
                .iter()
                .flat_map(|v| f16::from_f32(*v).to_le_bytes())
                .collect(),
        };
        let matrices = resources.weights("matrices", &matrix_bytes)?;
        let parameter_bytes: Vec<u8> = plan
            .parameters
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let parameters = resources.weights("parameters", &parameter_bytes)?;
        let tensors = plan
            .tensors
            .iter()
            .map(|&values| resources.tensor(values))
            .collect::<Result<Vec<_>, _>>()?;

        let linear = resources.shader(
            "linear",
            match precision {
                WeightPrecision::F32 => LINEAR_F32_DXBC,
                WeightPrecision::F16 => LINEAR_F16_DXBC,
            },
        )?;
        let layer_norm = resources.shader("layer norm", LAYER_NORM_DXBC)?;
        let attention = resources.shader("attention", ATTENTION_DXBC)?;

        let steps = plan
            .dispatches
            .iter()
            .map(|dispatch| {
                let srv = |binding: &Binding| match binding {
                    Binding::Matrices => matrices.clone(),
                    Binding::Parameters => parameters.clone(),
                    Binding::Tensor(id) => tensors[id.0].srv.clone(),
                };
                let mut inputs: [Option<ID3D11ShaderResourceView>; SRV_SLOTS] = Default::default();
                for (slot, binding) in inputs.iter_mut().zip(&dispatch.inputs) {
                    *slot = Some(srv(binding));
                }
                Ok(Step {
                    shader: match dispatch.kernel {
                        Kernel::Linear => linear.clone(),
                        Kernel::LayerNorm => layer_norm.clone(),
                        Kernel::Attention => attention.clone(),
                    },
                    constants: Some(resources.constants(&dispatch.constants)?),
                    inputs,
                    output: Some(tensors[dispatch.output.0].uav.clone()),
                    groups: dispatch.groups,
                })
            })
            .collect::<Result<Vec<_>, GpuError>>()?;

        let output_bytes = plan.tensors[plan.output.0] * 4;
        let ring = (0..RING)
            .map(|_| {
                Ok(Slot {
                    buffer: resources.staging("readback", output_bytes)?,
                    inference: None,
                })
            })
            .collect::<Result<Vec<_>, GpuError>>()?
            .try_into()
            .unwrap_or_else(|_| unreachable!("the ring is built from RING slots"));

        Ok(GpuEgoPoser {
            steps,
            tensors,
            estimates_shape: weights.betas.is_some(),
            plan,
            ring,
            next: 0,
            submitted: 0,
        })
    }

    /// Record one inference of `window` on `context`: the tokens' upload, the pass, and the copy of
    /// the estimate into the next staging buffer of the ring. Returns the inference's number, which
    /// its [`Readback`] carries. If that staging buffer still holds an estimate nobody has
    /// collected, the estimate is dropped.
    pub fn submit(&mut self, context: &ID3D11DeviceContext, window: &SparseWindow) -> u64 {
        let tokens = token_inputs(window);
        let tokens_buffer = &self.tensors[self.plan.tokens.0].buffer;
        let output_buffer = &self.tensors[self.plan.output.0].buffer;
        // SAFETY: every resource is ours and alive; the token buffer holds exactly the tokens'
        // values; the caller owns the context for the duration of the call.
        unsafe {
            context.UpdateSubresource(tokens_buffer, 0, None, tokens.as_ptr().cast(), 0, 0);
            let saved = SavedState::take(context);
            for step in &self.steps {
                context.CSSetShader(&step.shader, None);
                context.CSSetConstantBuffers(0, Some(std::slice::from_ref(&step.constants)));
                // The previous step's output is this step's input: unbind it as an output first,
                // or the runtime refuses it as an input.
                context.CSSetUnorderedAccessViews(0, 1, Some(&None), None);
                context.CSSetShaderResources(0, Some(&step.inputs));
                context.CSSetUnorderedAccessViews(0, 1, Some(&step.output), None);
                context.Dispatch(step.groups[0], step.groups[1], 1);
            }
            saved.restore(context);
            let slot = &mut self.ring[self.next];
            context.CopyResource(&slot.buffer, output_buffer);
            slot.inference = Some(self.submitted);
        }
        self.next = (self.next + 1) % RING;
        self.submitted += 1;
        self.submitted - 1
    }

    /// The oldest submitted inference not yet collected, if the GPU has finished it. Never waits.
    pub fn poll(&mut self, context: &ID3D11DeviceContext) -> Result<Option<Readback>, GpuError> {
        let Some(index) = (0..RING)
            .filter(|&i| self.ring[i].inference.is_some())
            .min_by_key(|&i| self.ring[i].inference)
        else {
            return Ok(None);
        };
        self.read_slot(context, index, false)
    }

    /// Run one inference of `window` and wait for its estimate. For tests and tools: this stalls
    /// the CPU until the GPU has caught up.
    pub fn infer_blocking(
        &mut self,
        context: &ID3D11DeviceContext,
        window: &SparseWindow,
    ) -> Result<EgoPoserOutput, GpuError> {
        let slot = self.next;
        self.submit(context, window);
        let readback = self
            .read_slot(context, slot, true)?
            .expect("a waiting map always returns the estimate");
        Ok(readback.output)
    }

    /// Whether the checkpoint estimates the body's shape.
    pub fn estimates_shape(&self) -> bool {
        self.estimates_shape
    }

    /// Map ring slot `index`, waiting for the GPU when `wait` is set, and collect its estimate.
    fn read_slot(
        &mut self,
        context: &ID3D11DeviceContext,
        index: usize,
        wait: bool,
    ) -> Result<Option<Readback>, GpuError> {
        let slot = &mut self.ring[index];
        let Some(inference) = slot.inference else {
            return Ok(None);
        };
        let flags = if wait {
            0
        } else {
            D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32
        };
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: the staging buffer is ours and holds the output tensor's values.
        let values = unsafe {
            match context.Map(&slot.buffer, 0, D3D11_MAP_READ, flags, Some(&mut mapped)) {
                Ok(()) => {}
                Err(e) if e.code() == DXGI_ERROR_WAS_STILL_DRAWING => return Ok(None),
                Err(source) => return Err(GpuError::Map { source }),
            }
            let len = self.plan.tensors[self.plan.output.0];
            let values = std::slice::from_raw_parts(mapped.pData as *const f32, len).to_vec();
            context.Unmap(&slot.buffer, 0);
            values
        };
        slot.inference = None;
        Ok(Some(Readback {
            output: self.output_of(&values),
            inference,
        }))
    }

    fn output_of(&self, values: &[f32]) -> EgoPoserOutput {
        let part = |at: usize, len: usize| &values[at..at + len];
        EgoPoserOutput {
            root_orient: part(ROOT_ORIENT_AT, 6).try_into().expect("six values"),
            pose_body: part(POSE_BODY_AT, POSE_BODY)
                .try_into()
                .expect("POSE_BODY values"),
            betas: self
                .estimates_shape
                .then(|| part(BETAS_AT, BETAS).try_into().expect("BETAS values")),
        }
    }
}

/// How many inferences can be in flight before the oldest uncollected one is dropped.
const RING: usize = 3;

/// The shader resource slots a step binds: the most any kernel reads.
const SRV_SLOTS: usize = 3;

const LINEAR_F32_DXBC: &[u8] = include_bytes!("../../shaders/egoposer_linear_f32.dxbc");
const LINEAR_F16_DXBC: &[u8] = include_bytes!("../../shaders/egoposer_linear_f16.dxbc");
const LAYER_NORM_DXBC: &[u8] = include_bytes!("../../shaders/egoposer_layer_norm.dxbc");
const ATTENTION_DXBC: &[u8] = include_bytes!("../../shaders/egoposer_attention.dxbc");

/// One dispatch, ready to record.
struct Step {
    shader: ID3D11ComputeShader,
    constants: Option<ID3D11Buffer>,
    inputs: [Option<ID3D11ShaderResourceView>; SRV_SLOTS],
    output: Option<ID3D11UnorderedAccessView>,
    groups: [u32; 2],
}

/// An activation tensor's buffer, readable and writable as raw 32-bit words.
struct Tensor {
    buffer: ID3D11Buffer,
    srv: ID3D11ShaderResourceView,
    uav: ID3D11UnorderedAccessView,
}

struct Slot {
    buffer: ID3D11Buffer,
    /// The inference whose estimate the buffer holds or will hold, until it is collected.
    inference: Option<u64>,
}

/// The compute state [`GpuEgoPoser::submit`] changes, taken before and put back after.
struct SavedState {
    shader: Option<ID3D11ComputeShader>,
    constants: [Option<ID3D11Buffer>; 1],
    inputs: [Option<ID3D11ShaderResourceView>; SRV_SLOTS],
    output: [Option<ID3D11UnorderedAccessView>; 1],
}

impl SavedState {
    /// # Safety
    ///
    /// The caller owns `context` for the duration of the call.
    unsafe fn take(context: &ID3D11DeviceContext) -> SavedState {
        let mut saved = SavedState {
            shader: None,
            constants: Default::default(),
            inputs: Default::default(),
            output: Default::default(),
        };
        // SAFETY: as the function's contract.
        unsafe {
            context.CSGetShader(&mut saved.shader, None, None);
            context.CSGetConstantBuffers(0, Some(&mut saved.constants));
            context.CSGetShaderResources(0, Some(&mut saved.inputs));
            context.CSGetUnorderedAccessViews(0, Some(&mut saved.output));
        }
        saved
    }

    /// # Safety
    ///
    /// As [`SavedState::take`].
    unsafe fn restore(self, context: &ID3D11DeviceContext) {
        // SAFETY: as the function's contract. The last step's output is unbound before the saved
        // inputs go back, in case one of them is the same buffer.
        unsafe {
            context.CSSetUnorderedAccessViews(0, 1, Some(&None), None);
            context.CSSetShaderResources(0, Some(&self.inputs));
            context.CSSetUnorderedAccessViews(0, 1, Some(self.output.as_ptr()), None);
            context.CSSetConstantBuffers(0, Some(&self.constants));
            context.CSSetShader(self.shader.as_ref(), None);
        }
    }
}

/// Resource creation on one device, with the errors named.
struct Resources<'a> {
    device: &'a ID3D11Device,
}

impl Resources<'_> {
    /// An immutable buffer of `bytes`, readable as raw words.
    fn weights(
        &self,
        name: &'static str,
        bytes: &[u8],
    ) -> Result<ID3D11ShaderResourceView, GpuError> {
        let buffer = self.buffer(
            name,
            &D3D11_BUFFER_DESC {
                ByteWidth: byte_width(bytes.len()),
                Usage: D3D11_USAGE_IMMUTABLE,
                BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                MiscFlags: D3D11_RESOURCE_MISC_BUFFER_ALLOW_RAW_VIEWS.0 as u32,
                ..Default::default()
            },
            Some(bytes),
        )?;
        self.srv(name, &buffer, bytes.len())
    }

    /// A zeroed buffer of `values` 32-bit words, readable and writable as raw words.
    fn tensor(&self, values: usize) -> Result<Tensor, GpuError> {
        let name = "activation";
        let buffer = self.buffer(
            name,
            &D3D11_BUFFER_DESC {
                ByteWidth: byte_width(values * 4),
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_UNORDERED_ACCESS.0) as u32,
                MiscFlags: D3D11_RESOURCE_MISC_BUFFER_ALLOW_RAW_VIEWS.0 as u32,
                ..Default::default()
            },
            Some(&vec![0; values * 4]),
        )?;
        let srv = self.srv(name, &buffer, values * 4)?;
        let mut uav = None;
        // SAFETY: the buffer is live and allows raw views; the descriptor covers it exactly.
        unsafe {
            self.device.CreateUnorderedAccessView(
                &buffer,
                Some(&D3D11_UNORDERED_ACCESS_VIEW_DESC {
                    Format: DXGI_FORMAT_R32_TYPELESS,
                    ViewDimension: D3D11_UAV_DIMENSION_BUFFER,
                    Anonymous: D3D11_UNORDERED_ACCESS_VIEW_DESC_0 {
                        Buffer: D3D11_BUFFER_UAV {
                            FirstElement: 0,
                            NumElements: byte_width(values),
                            Flags: D3D11_BUFFER_UAV_FLAG_RAW.0 as u32,
                        },
                    },
                }),
                Some(&mut uav),
            )
        }
        .map_err(|source| GpuError::CreateView {
            buffer: name,
            source,
        })?;
        Ok(Tensor {
            buffer,
            srv,
            uav: uav.expect("a created view is returned"),
        })
    }

    /// A staging buffer of `bytes` for the CPU to read.
    fn staging(&self, name: &'static str, bytes: usize) -> Result<ID3D11Buffer, GpuError> {
        self.buffer(
            name,
            &D3D11_BUFFER_DESC {
                ByteWidth: byte_width(bytes),
                Usage: D3D11_USAGE_STAGING,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                ..Default::default()
            },
            None,
        )
    }

    /// An immutable constant buffer holding `words`.
    fn constants(&self, words: &[u32; 8]) -> Result<ID3D11Buffer, GpuError> {
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        self.buffer(
            "constant",
            &D3D11_BUFFER_DESC {
                ByteWidth: byte_width(bytes.len()),
                Usage: D3D11_USAGE_IMMUTABLE,
                BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                ..Default::default()
            },
            Some(&bytes),
        )
    }

    fn shader(&self, name: &'static str, bytecode: &[u8]) -> Result<ID3D11ComputeShader, GpuError> {
        let mut shader = None;
        // SAFETY: the bytecode is a complete compiled compute shader.
        unsafe {
            self.device
                .CreateComputeShader(bytecode, None, Some(&mut shader))
        }
        .map_err(|source| GpuError::CreateShader {
            shader: name,
            source,
        })?;
        Ok(shader.expect("a created shader is returned"))
    }

    fn buffer(
        &self,
        name: &'static str,
        desc: &D3D11_BUFFER_DESC,
        initial: Option<&[u8]>,
    ) -> Result<ID3D11Buffer, GpuError> {
        let data = initial.map(|bytes| D3D11_SUBRESOURCE_DATA {
            pSysMem: bytes.as_ptr().cast(),
            ..Default::default()
        });
        let mut buffer = None;
        // SAFETY: the descriptor is valid and the initial data, when given, spans its byte width.
        unsafe {
            self.device.CreateBuffer(
                desc,
                data.as_ref().map(|d| d as *const _),
                Some(&mut buffer),
            )
        }
        .map_err(|source| GpuError::CreateBuffer {
            buffer: name,
            source,
        })?;
        Ok(buffer.expect("a created buffer is returned"))
    }

    fn srv(
        &self,
        name: &'static str,
        buffer: &ID3D11Buffer,
        bytes: usize,
    ) -> Result<ID3D11ShaderResourceView, GpuError> {
        let mut srv = None;
        // SAFETY: the buffer is live and allows raw views; the descriptor covers it exactly.
        unsafe {
            self.device.CreateShaderResourceView(
                buffer,
                Some(&D3D11_SHADER_RESOURCE_VIEW_DESC {
                    Format: DXGI_FORMAT_R32_TYPELESS,
                    ViewDimension: D3D11_SRV_DIMENSION_BUFFEREX,
                    Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                        BufferEx: D3D11_BUFFEREX_SRV {
                            FirstElement: 0,
                            NumElements: byte_width(bytes / 4),
                            Flags: D3D11_BUFFEREX_SRV_FLAG_RAW.0 as u32,
                        },
                    },
                }),
                Some(&mut srv),
            )
        }
        .map_err(|source| GpuError::CreateView {
            buffer: name,
            source,
        })?;
        Ok(srv.expect("a created view is returned"))
    }
}

fn byte_width(bytes: usize) -> u32 {
    u32::try_from(bytes).expect("the network's buffers fit a 32-bit size")
}

#[cfg(test)]
mod tests;
