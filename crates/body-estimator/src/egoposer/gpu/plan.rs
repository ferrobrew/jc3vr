//! The GPU pass laid out as data, with nothing of Direct3D in it: the weights packed into two
//! buffers, the activations' buffers, and the dispatches that run the network, each with its
//! kernel, constants, and bindings. The executor creates the resources and records the dispatches.
//!
//! The pass follows the CPU reference stage by stage, with three rearrangements that change nothing
//! but rounding:
//!
//! - The slow and the fast streams are embedded as their sum, the bias added twice (see
//!   `token_inputs`), so the embedding is one dispatch.
//! - Each layer's query, key, and value projections stay fused, as the checkpoint stores them, so
//!   they are one dispatch.
//! - The decoders' hidden layers are stacked into one dispatch, each decoder's output layer then
//!   reading its own part.
//!
//! The last encoder layer is trimmed as the CPU reference trims it: its projections cover every
//! token, but only the last token is queried, and everything after the attention is that token's.

use crate::{
    egoposer::{
        network::{HEADS, INPUT, TOKENS},
        weights::{EncoderLayer, LayerNorm, Weights},
    },
    ops::Linear,
};

/// The model width the kernels are written for: their thread counts and groupshared arrays.
pub(crate) const WIDTH: usize = 256;

/// Where each estimate starts in the output tensor: the root orientation, the body pose, and the
/// shape coefficients, in that order.
pub(crate) const ROOT_ORIENT_AT: usize = 0;
pub(crate) const POSE_BODY_AT: usize = 6;
pub(crate) const BETAS_AT: usize = POSE_BODY_AT + 126;

/// The plan of one network's pass.
pub(crate) struct Plan {
    /// Every weight matrix, each starting on a multiple of four values.
    pub matrices: Vec<f32>,
    /// The biases and the layer norms' scales and shifts.
    pub parameters: Vec<f32>,
    /// Each activation tensor's length in values, indexed by [`TensorId`].
    pub tensors: Vec<usize>,
    /// The tokens' inputs, `TOKENS × INPUT`, uploaded for each inference.
    pub tokens: TensorId,
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the tests read every stage back")
    )]
    pub embedded: TensorId,
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the tests read every stage back")
    )]
    pub layers: Vec<LayerTensors>,
    /// The estimates: the root orientation, the body pose, and the shape where the checkpoint has
    /// a shape head.
    pub output: TensorId,
    pub dispatches: Vec<Dispatch>,
}

/// One activation tensor: an index into [`Plan::tensors`].
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct TensorId(pub usize);

/// One encoder layer's tensors. The last layer's have one row from the context on.
#[derive(Debug, Copy, Clone)]
pub(crate) struct LayerTensors {
    /// Each token's query, key, and value, side by side: `TOKENS × 3·WIDTH`.
    pub projections: TensorId,
    /// The heads' attention before the output projection.
    pub context: TensorId,
    /// The attention's output projection.
    pub attended: TensorId,
    /// The first residual and layer norm.
    pub normed: TensorId,
    /// The feed-forward's hidden layer, after the ReLU.
    pub hidden: TensorId,
    /// The feed-forward's output.
    pub fed_forward: TensorId,
    /// The second residual and layer norm: the layer's output.
    pub output: TensorId,
    /// The rows from the context on: every token's, or the last token's alone.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the tests read every stage back")
    )]
    pub queries: usize,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Kernel {
    Linear,
    LayerNorm,
    Attention,
}

/// What a shader resource slot reads.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Binding {
    Matrices,
    Parameters,
    Tensor(TensorId),
}

/// One dispatch of a kernel.
#[derive(Debug, Clone)]
pub(crate) struct Dispatch {
    pub kernel: Kernel,
    /// The kernel's constant buffer as 32-bit words, the unused ones zero.
    pub constants: [u32; 8],
    /// The shader resources, in register order from `t0`.
    pub inputs: Vec<Binding>,
    pub output: TensorId,
    /// The thread groups along x and y.
    pub groups: [u32; 2],
}

impl Plan {
    /// The plan for `weights`, whose width must be [`WIDTH`].
    pub fn new(weights: &Weights) -> Plan {
        assert_eq!(
            weights.width, WIDTH,
            "the kernels are written for one width"
        );
        let mut builder = Builder::default();
        let tokens = builder.tensor(TOKENS * INPUT);
        let embedded = builder.tensor(TOKENS * WIDTH);
        let doubled_bias: Vec<f32> = weights.embedding.bias.iter().map(|b| 2.0 * b).collect();
        builder.linear(LinearStep {
            weight: &weights.embedding.weight,
            bias: &doubled_bias,
            inputs: INPUT,
            outputs: WIDTH,
            rows: TOKENS,
            input: (tokens, 0),
            output: (embedded, 0),
            relu: false,
        });

        let mut layers = Vec::new();
        let mut x = embedded;
        for (i, layer) in weights.layers.iter().enumerate() {
            let queries = if i + 1 == weights.layers.len() {
                1
            } else {
                TOKENS
            };
            let tensors = builder.encoder_layer(layer, x, queries);
            x = tensors.output;
            layers.push(tensors);
        }

        let decoders: Vec<_> = [Some(&weights.root_orient), Some(&weights.pose_body)]
            .into_iter()
            .chain([weights.betas.as_ref()])
            .flatten()
            .collect();
        let hidden = builder.tensor(decoders.len() * WIDTH);
        let stacked_weight: Vec<f32> = decoders
            .iter()
            .flat_map(|d| d.hidden.weight.iter().copied())
            .collect();
        let stacked_bias: Vec<f32> = decoders
            .iter()
            .flat_map(|d| d.hidden.bias.iter().copied())
            .collect();
        builder.linear(LinearStep {
            weight: &stacked_weight,
            bias: &stacked_bias,
            inputs: WIDTH,
            outputs: decoders.len() * WIDTH,
            rows: 1,
            input: (x, 0),
            output: (hidden, 0),
            relu: true,
        });
        let outputs: usize = decoders.iter().map(|d| d.out.outputs).sum();
        let output = builder.tensor(outputs);
        let mut at = ROOT_ORIENT_AT;
        for (i, decoder) in decoders.iter().enumerate() {
            builder.linear(LinearStep::of(
                &decoder.out,
                1,
                (hidden, i * WIDTH),
                (output, at),
                false,
            ));
            at += decoder.out.outputs;
        }

        Plan {
            matrices: builder.matrices,
            parameters: builder.parameters,
            tensors: builder.tensors,
            tokens,
            embedded,
            layers,
            output,
            dispatches: builder.dispatches,
        }
    }
}

#[derive(Default)]
struct Builder {
    matrices: Vec<f32>,
    parameters: Vec<f32>,
    tensors: Vec<usize>,
    dispatches: Vec<Dispatch>,
}

/// One linear layer's dispatch: `rows` rows of `inputs` values from `input` (a tensor and the
/// offset of its first row) to `outputs` values each in `output`.
struct LinearStep<'a> {
    weight: &'a [f32],
    bias: &'a [f32],
    inputs: usize,
    outputs: usize,
    rows: usize,
    input: (TensorId, usize),
    output: (TensorId, usize),
    relu: bool,
}

impl<'a> LinearStep<'a> {
    fn of(
        layer: &'a Linear,
        rows: usize,
        input: (TensorId, usize),
        output: (TensorId, usize),
        relu: bool,
    ) -> LinearStep<'a> {
        LinearStep {
            weight: &layer.weight,
            bias: &layer.bias,
            inputs: layer.inputs,
            outputs: layer.outputs,
            rows,
            input,
            output,
            relu,
        }
    }
}

impl Builder {
    fn tensor(&mut self, values: usize) -> TensorId {
        self.tensors.push(values);
        TensorId(self.tensors.len() - 1)
    }

    fn matrix(&mut self, values: &[f32]) -> u32 {
        // The kernel reads four weights at a time, as one 16-byte load in 32 bits or one 8-byte
        // load in 16 bits.
        debug_assert_eq!(self.matrices.len() % 4, 0);
        let at = self.matrices.len();
        self.matrices.extend_from_slice(values);
        self.matrices
            .resize(self.matrices.len().next_multiple_of(4), 0.0);
        word(at)
    }

    fn parameter(&mut self, values: &[f32]) -> u32 {
        let at = self.parameters.len();
        self.parameters.extend_from_slice(values);
        word(at)
    }

    fn linear(&mut self, step: LinearStep<'_>) {
        assert_eq!(step.weight.len(), step.inputs * step.outputs);
        assert_eq!(step.bias.len(), step.outputs);
        assert!(
            step.inputs.is_multiple_of(4),
            "the kernel reads four inputs at a time"
        );
        let weights = self.matrix(step.weight);
        let biases = self.parameter(step.bias);
        self.dispatches.push(Dispatch {
            kernel: Kernel::Linear,
            constants: [
                word(step.rows),
                word(step.inputs),
                word(step.outputs),
                u32::from(step.relu),
                word(step.input.1),
                word(step.output.1),
                weights,
                biases,
            ],
            inputs: vec![
                Binding::Matrices,
                Binding::Parameters,
                Binding::Tensor(step.input.0),
            ],
            output: step.output.0,
            groups: [
                word(step.outputs),
                word(step.rows.div_ceil(LINEAR_ROWS_PER_GROUP)),
            ],
        });
    }

    /// `y = LayerNorm(input + residual)`, over `rows` rows of the residual from `residual.1` on.
    fn layer_norm(
        &mut self,
        input: TensorId,
        residual: (TensorId, usize),
        norm: &LayerNorm,
        output: TensorId,
        rows: usize,
    ) {
        let gamma = self.parameter(&norm.gamma);
        let beta = self.parameter(&norm.beta);
        self.dispatches.push(Dispatch {
            kernel: Kernel::LayerNorm,
            constants: [word(residual.1), gamma, beta, 0, 0, 0, 0, 0],
            inputs: vec![
                Binding::Tensor(input),
                Binding::Tensor(residual.0),
                Binding::Parameters,
            ],
            output,
            groups: [word(rows), 1],
        });
    }

    /// One post-norm encoder layer on `x`, computing the output of its last `queries` tokens.
    fn encoder_layer(&mut self, layer: &EncoderLayer, x: TensorId, queries: usize) -> LayerTensors {
        let tensors = LayerTensors {
            projections: self.tensor(TOKENS * 3 * WIDTH),
            context: self.tensor(queries * WIDTH),
            attended: self.tensor(queries * WIDTH),
            normed: self.tensor(queries * WIDTH),
            hidden: self.tensor(queries * layer.feed_forward1.outputs),
            fed_forward: self.tensor(queries * WIDTH),
            output: self.tensor(queries * WIDTH),
            queries,
        };
        let projection_weight: Vec<f32> = [&layer.query, &layer.key, &layer.value]
            .into_iter()
            .flat_map(|l| l.weight.iter().copied())
            .collect();
        let projection_bias: Vec<f32> = [&layer.query, &layer.key, &layer.value]
            .into_iter()
            .flat_map(|l| l.bias.iter().copied())
            .collect();
        self.linear(LinearStep {
            weight: &projection_weight,
            bias: &projection_bias,
            inputs: WIDTH,
            outputs: 3 * WIDTH,
            rows: TOKENS,
            input: (x, 0),
            output: (tensors.projections, 0),
            relu: false,
        });
        let scale = 1.0 / ((WIDTH / HEADS) as f32).sqrt();
        self.dispatches.push(Dispatch {
            kernel: Kernel::Attention,
            constants: [word(queries), scale.to_bits(), 0, 0, 0, 0, 0, 0],
            inputs: vec![Binding::Tensor(tensors.projections)],
            output: tensors.context,
            groups: [word(HEADS), word(queries)],
        });
        self.linear(LinearStep::of(
            &layer.attention_out,
            queries,
            (tensors.context, 0),
            (tensors.attended, 0),
            false,
        ));
        let first_query = (TOKENS - queries) * WIDTH;
        self.layer_norm(
            tensors.attended,
            (x, first_query),
            &layer.norm1,
            tensors.normed,
            queries,
        );
        self.linear(LinearStep::of(
            &layer.feed_forward1,
            queries,
            (tensors.normed, 0),
            (tensors.hidden, 0),
            true,
        ));
        self.linear(LinearStep::of(
            &layer.feed_forward2,
            queries,
            (tensors.hidden, 0),
            (tensors.fed_forward, 0),
            false,
        ));
        self.layer_norm(
            tensors.fed_forward,
            (tensors.normed, 0),
            &layer.norm2,
            tensors.output,
            queries,
        );
        tensors
    }
}

/// The rows each of the linear kernel's thread groups computes (`ROWS_PER_GROUP` in the HLSL).
const LINEAR_ROWS_PER_GROUP: usize = 8;

/// A count or an offset as a constant buffer word.
fn word(value: usize) -> u32 {
    u32::try_from(value).expect("the network's sizes fit a 32-bit word")
}
