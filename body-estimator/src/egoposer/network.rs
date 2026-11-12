//! EgoPoser's forward pass (`networks/egoposer.py`), in the order the reference runs it:
//!
//! 1. Spatial normalisation: the head's horizontal position is taken off its own and both wrists',
//!    keeping the heights.
//! 2. The window-start deltas: each joint's normalised horizontal position less the window's first
//!    frame's, appended to every frame (54 values become 60).
//! 3. The hand masks: a hand's rotations, rates, positions, and steps are zeroed in the frames it
//!    was not visible. The deltas are left as they are, as the reference leaves them.
//! 4. The slow-fast tokens: the even frames (the slow stream, 40 of 80) and the last 40 frames (the
//!    fast stream) go through the same embedding, and the two are summed token by token. There is
//!    no positional encoding.
//! 5. The encoder: post-norm transformer layers, each self-attention then a ReLU feed-forward, each
//!    added back and layer-normalised.
//! 6. The decoders read the last token.
//!
//! Only the last token reaches the decoders, so the last encoder layer computes its output alone:
//! the keys and values still come from every token, but the queries, the attention's output, the
//! residuals, the norms, and the feed-forward are the last token's only. Every token's row is
//! computed independently of the others' after the attention, so this is exact, not an
//! approximation, and it skips most of the last layer's work.

use crate::{
    egoposer::{
        EgoPoserOutput, Trace,
        weights::{Decoder, EncoderLayer, WINDOW_DELTAS, Weights},
    },
    features::{FEATURES, SparseWindow, WINDOW_FRAMES},
    ops::{Product, layer_norm, relu, softmax_rows},
};

/// The attention heads of each encoder layer, the model's configuration (`nhead`).
pub(crate) const HEADS: usize = 8;

/// The values in one frame of the network's input, after the window-start deltas.
pub(crate) const INPUT: usize = FEATURES + WINDOW_DELTAS;

/// The tokens the encoder sees: half the window's frames.
pub(crate) const TOKENS: usize = WINDOW_FRAMES / 2;

/// Steps 1 to 3: the network's input, `WINDOW_FRAMES × INPUT` values.
pub(crate) fn network_input(window: &SparseWindow) -> Vec<f32> {
    let mut x = vec![0.0f32; WINDOW_FRAMES * INPUT];
    for (row, features) in x
        .as_chunks_mut::<INPUT>()
        .0
        .iter_mut()
        .zip(&window.features)
    {
        row[..FEATURES].copy_from_slice(features);
        let head = [features[HEAD_POSITION], features[HEAD_POSITION + 1]];
        for joint in JOINT_POSITIONS {
            row[joint] -= head[0];
            row[joint + 1] -= head[1];
        }
    }
    let first: Vec<f32> = x[..INPUT].to_vec();
    for row in x.as_chunks_mut::<INPUT>().0 {
        for (i, joint) in JOINT_POSITIONS.into_iter().enumerate() {
            row[FEATURES + 2 * i] = row[joint] - first[joint];
            row[FEATURES + 2 * i + 1] = row[joint + 1] - first[joint + 1];
        }
    }
    for (frame, row) in x.as_chunks_mut::<INPUT>().0.iter_mut().enumerate() {
        if !window.visibility.left[frame] {
            zero(row, &LEFT_HAND);
        }
        if !window.visibility.right[frame] {
            zero(row, &RIGHT_HAND);
        }
    }
    x
}

/// The embedding's input for each token, `TOKENS × INPUT` values: the slow stream's frame plus the
/// fast stream's. The embedding is linear, so embedding this sum and adding the bias twice gives
/// the summed embeddings of step 4, up to rounding.
#[cfg(all(windows, feature = "d3d11"))]
pub(crate) fn token_inputs(window: &SparseWindow) -> Vec<f32> {
    let input = network_input(window);
    let (frames, _) = input.as_chunks::<INPUT>();
    let (slow, fast) = (frames.iter().step_by(2), &frames[TOKENS..]);
    slow.zip(fast)
        .flat_map(|(s, f)| s.iter().zip(f).map(|(a, b)| a + b))
        .collect()
}

/// The whole pass, recording the intermediate tensors in `trace` when given one.
pub(crate) fn forward(
    weights: &Weights,
    window: &SparseWindow,
    mut trace: Option<&mut Trace>,
) -> EgoPoserOutput {
    let width = weights.width;
    let input = network_input(window);
    let slow: Vec<f32> = input
        .as_chunks::<INPUT>()
        .0
        .iter()
        .step_by(2)
        .flatten()
        .copied()
        .collect();
    let fast = &input[TOKENS * INPUT..];
    let mut x = weights.embedding.forward(&slow, TOKENS);
    for (a, b) in x.iter_mut().zip(weights.embedding.forward(fast, TOKENS)) {
        *a += b;
    }
    if let Some(trace) = trace.as_deref_mut() {
        trace.normalized = input.clone();
        trace.embedded = x.clone();
    }
    let last_layer = weights.layers.len() - 1;
    for (i, layer) in weights.layers.iter().enumerate() {
        let outputs = if i == last_layer { 1 } else { TOKENS };
        x = encoder_layer(layer, &x, width, outputs);
        if let Some(trace) = trace.as_deref_mut() {
            trace.layers.push(x.clone());
        }
    }
    let last = &x[x.len() - width..];
    let root_orient = decode(&weights.root_orient, last);
    let pose_body = decode(&weights.pose_body, last);
    EgoPoserOutput {
        root_orient: root_orient
            .try_into()
            .expect("the root decoder has 6 outputs"),
        pose_body: pose_body
            .try_into()
            .expect("the pose decoder has POSE_BODY outputs"),
        betas: weights.betas.as_ref().map(|head| {
            decode(head, last)
                .try_into()
                .expect("the shape decoder has BETAS outputs")
        }),
    }
}

/// Where the head's position starts in a frame of the sparse input.
const HEAD_POSITION: usize = 36;

/// Where each joint's position starts: head, left wrist, right wrist.
const JOINT_POSITIONS: [usize; 3] = [36, 39, 42];

/// The left hand's inputs the mask zeroes: its rotation, rate, position, and step.
const LEFT_HAND: [std::ops::Range<usize>; 4] = [6..12, 24..30, 39..42, 48..51];

/// The right hand's.
const RIGHT_HAND: [std::ops::Range<usize>; 4] = [12..18, 30..36, 42..45, 51..54];

fn zero(row: &mut [f32], ranges: &[std::ops::Range<usize>]) {
    for range in ranges {
        row[range.clone()].fill(0.0);
    }
}

/// One `nn.TransformerEncoderLayer` in its default post-norm form, on `TOKENS` rows of `width`,
/// computing the output of the last `outputs` tokens only (every token attends to all of them).
fn encoder_layer(layer: &EncoderLayer, x: &[f32], width: usize, outputs: usize) -> Vec<f32> {
    let first = (TOKENS - outputs) * width;
    let mut y = x[first..].to_vec();
    for (v, a) in y.iter_mut().zip(self_attention(layer, x, width, outputs)) {
        *v += a;
    }
    layer_norm(&mut y, width, &layer.norm1.gamma, &layer.norm1.beta);
    let mut hidden = layer.feed_forward1.forward(&y, outputs);
    relu(&mut hidden);
    for (v, f) in y
        .iter_mut()
        .zip(layer.feed_forward2.forward(&hidden, outputs))
    {
        *v += f;
    }
    layer_norm(&mut y, width, &layer.norm2.gamma, &layer.norm2.beta);
    y
}

/// Multi-head scaled dot-product self-attention with no mask: the last `queries` tokens attend to
/// every token, through the attention's output projection.
fn self_attention(layer: &EncoderLayer, x: &[f32], width: usize, queries: usize) -> Vec<f32> {
    let query = layer
        .query
        .forward(&x[(TOKENS - queries) * width..], queries);
    let key = layer.key.forward(x, TOKENS);
    let value = layer.value.forward(x, TOKENS);
    let context = attend(&query, &key, &value, queries, width);
    layer.attention_out.forward(&context, queries)
}

/// The attention's heads before the output projection: `queries` rows of `query` against the
/// `TOKENS` rows of `key` and `value`, all `width` wide with each head's columns side by side.
pub(crate) fn attend(
    query: &[f32],
    key: &[f32],
    value: &[f32],
    queries: usize,
    width: usize,
) -> Vec<f32> {
    let head_width = width / HEADS;
    let scale = 1.0 / (head_width as f32).sqrt();
    let mut context = vec![0.0f32; queries * width];
    let mut scores = vec![0.0f32; queries * TOKENS];
    for head in 0..HEADS {
        let offset = head * head_width;
        // scores = q_h · k_hᵀ / sqrt(head width)
        Product {
            a: (&query[offset..], width, 1),
            b: (&key[offset..], 1, width),
            m: queries,
            k: head_width,
            n: TOKENS,
        }
        .accumulate(scale, 0.0, &mut scores, TOKENS);
        softmax_rows(&mut scores, TOKENS);
        // context_h = softmax(scores) · v_h, into the head's columns.
        Product {
            a: (&scores, TOKENS, 1),
            b: (&value[offset..], width, 1),
            m: queries,
            k: TOKENS,
            n: head_width,
        }
        .accumulate(1.0, 0.0, &mut context[offset..], width);
    }
    context
}

fn decode(decoder: &Decoder, token: &[f32]) -> Vec<f32> {
    let mut hidden = decoder.hidden.forward(token, 1);
    relu(&mut hidden);
    decoder.out.forward(&hidden, 1)
}
