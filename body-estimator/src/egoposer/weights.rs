//! EgoPoser's weights from a safetensors file, under PyTorch's own parameter names (the state dict
//! of the original `.pth`, converted without renaming).
//!
//! The attention's input projection is stored fused, as `nn.MultiheadAttention` keeps it
//! (`in_proj_weight` and `in_proj_bias`, the query, key, and value rows stacked), and is split
//! here into three layers.

use std::{
    fmt, io,
    path::{Path, PathBuf},
};

use safetensors::{Dtype, SafeTensorError, SafeTensors};

use crate::{features::FEATURES, ops::Linear};

/// Why the weights could not be loaded.
#[derive(Debug)]
pub struct WeightsError {
    /// Where the weights came from: their path, when loaded from a file.
    pub path: Option<PathBuf>,
    pub kind: WeightsErrorKind,
}

#[derive(Debug)]
pub enum WeightsErrorKind {
    /// The file could not be read.
    Read(io::Error),
    /// The bytes are not a safetensors file.
    Parse(SafeTensorError),
    /// A parameter the model needs is not in the file.
    Missing { name: String },
    /// A parameter is not stored as 32-bit floats.
    Dtype { name: String, found: Dtype },
    /// A parameter's shape is not the one the model needs.
    Shape {
        name: String,
        expected: Vec<usize>,
        found: Vec<usize>,
    },
}

impl fmt::Display for WeightsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.path {
            Some(path) => write!(f, "egoposer weights at {}: {}", path.display(), self.kind),
            None => write!(f, "egoposer weights: {}", self.kind),
        }
    }
}

impl fmt::Display for WeightsErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WeightsErrorKind::Read(error) => write!(f, "could not read the file: {error}"),
            WeightsErrorKind::Parse(error) => write!(f, "not a safetensors file: {error}"),
            WeightsErrorKind::Missing { name } => write!(f, "the parameter `{name}` is missing"),
            WeightsErrorKind::Dtype { name, found } => {
                write!(f, "the parameter `{name}` is {found:?}, not F32")
            }
            WeightsErrorKind::Shape {
                name,
                expected,
                found,
            } => write!(
                f,
                "the parameter `{name}` has the shape {found:?}, not {expected:?}"
            ),
        }
    }
}

impl std::error::Error for WeightsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            WeightsErrorKind::Read(error) => Some(error),
            WeightsErrorKind::Parse(error) => Some(error),
            _ => None,
        }
    }
}

/// One post-norm transformer encoder layer (`nn.TransformerEncoderLayer`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EncoderLayer {
    pub query: Linear,
    pub key: Linear,
    pub value: Linear,
    pub attention_out: Linear,
    pub norm1: LayerNorm,
    pub feed_forward1: Linear,
    pub feed_forward2: Linear,
    pub norm2: LayerNorm,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LayerNorm {
    pub gamma: Vec<f32>,
    pub beta: Vec<f32>,
}

/// A decoder head: `Linear`, ReLU, `Linear`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Decoder {
    pub hidden: Linear,
    pub out: Linear,
}

/// Every parameter of the network.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Weights {
    /// The model width, `d_model`.
    pub width: usize,
    /// The input embedding the slow and the fast token streams share.
    pub embedding: Linear,
    pub layers: Vec<EncoderLayer>,
    pub root_orient: Decoder,
    pub pose_body: Decoder,
    /// The body-shape head, in a checkpoint trained to estimate shape.
    pub betas: Option<Decoder>,
}

impl Weights {
    pub fn from_file(path: &Path) -> Result<Weights, WeightsError> {
        let error = |kind| WeightsError {
            path: Some(path.to_owned()),
            kind,
        };
        let bytes = std::fs::read(path).map_err(|e| error(WeightsErrorKind::Read(e)))?;
        Weights::parse(&bytes).map_err(error)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Weights, WeightsError> {
        Weights::parse(bytes).map_err(|kind| WeightsError { path: None, kind })
    }

    fn parse(bytes: &[u8]) -> Result<Weights, WeightsErrorKind> {
        let file = SafeTensors::deserialize(bytes).map_err(WeightsErrorKind::Parse)?;
        let embedding_shape = shape_of(&file, "linear_embedding.weight")?;
        let width = embedding_shape[0];
        let embedding = linear(&file, "linear_embedding", FEATURES + WINDOW_DELTAS, width)?;
        let mut layers = Vec::new();
        while file
            .names()
            .iter()
            .any(|name| name.starts_with(&format!("transformer_encoder.layers.{}.", layers.len())))
        {
            layers.push(encoder_layer(&file, layers.len(), width)?);
        }
        if layers.is_empty() {
            return Err(WeightsErrorKind::Missing {
                name: "transformer_encoder.layers.0".to_owned(),
            });
        }
        let betas = if file
            .names()
            .iter()
            .any(|name| name.starts_with("shape_decoder."))
        {
            Some(decoder(&file, "shape_decoder", width, BETAS)?)
        } else {
            None
        };
        Ok(Weights {
            width,
            embedding,
            layers,
            root_orient: decoder(&file, "global_orientation_decoder", width, 6)?,
            pose_body: decoder(&file, "joint_rotation_decoder", width, POSE_BODY)?,
            betas,
        })
    }
}

/// The values the window-start deltas add to each frame: the head's and each wrist's horizontal
/// position less the window's first frame's.
pub(crate) const WINDOW_DELTAS: usize = 6;

/// The 21 non-root joints' rotations, 6D each.
pub(crate) const POSE_BODY: usize = 21 * 6;

/// The body-shape coefficients a shape head estimates.
pub(crate) const BETAS: usize = 16;

/// The feed-forward width: `nn.TransformerEncoderLayer`'s default `dim_feedforward`.
const FEED_FORWARD: usize = 2048;

fn encoder_layer(
    file: &SafeTensors,
    index: usize,
    width: usize,
) -> Result<EncoderLayer, WeightsErrorKind> {
    let prefix = format!("transformer_encoder.layers.{index}");
    let in_weight = tensor(
        file,
        &format!("{prefix}.self_attn.in_proj_weight"),
        &[3 * width, width],
    )?;
    let in_bias = tensor(
        file,
        &format!("{prefix}.self_attn.in_proj_bias"),
        &[3 * width],
    )?;
    let part = |i: usize| Linear {
        weight: in_weight[i * width * width..(i + 1) * width * width].to_vec(),
        bias: in_bias[i * width..(i + 1) * width].to_vec(),
        inputs: width,
        outputs: width,
    };
    Ok(EncoderLayer {
        query: part(0),
        key: part(1),
        value: part(2),
        attention_out: linear(file, &format!("{prefix}.self_attn.out_proj"), width, width)?,
        norm1: layer_norm(file, &format!("{prefix}.norm1"), width)?,
        feed_forward1: linear(file, &format!("{prefix}.linear1"), width, FEED_FORWARD)?,
        feed_forward2: linear(file, &format!("{prefix}.linear2"), FEED_FORWARD, width)?,
        norm2: layer_norm(file, &format!("{prefix}.norm2"), width)?,
    })
}

/// A decoder head stored as an `nn.Sequential` of `Linear`, ReLU, `Linear` (indices 0 and 2).
fn decoder(
    file: &SafeTensors,
    prefix: &str,
    width: usize,
    outputs: usize,
) -> Result<Decoder, WeightsErrorKind> {
    Ok(Decoder {
        hidden: linear(file, &format!("{prefix}.0"), width, width)?,
        out: linear(file, &format!("{prefix}.2"), width, outputs)?,
    })
}

fn linear(
    file: &SafeTensors,
    prefix: &str,
    inputs: usize,
    outputs: usize,
) -> Result<Linear, WeightsErrorKind> {
    Ok(Linear {
        weight: tensor(file, &format!("{prefix}.weight"), &[outputs, inputs])?,
        bias: tensor(file, &format!("{prefix}.bias"), &[outputs])?,
        inputs,
        outputs,
    })
}

fn layer_norm(
    file: &SafeTensors,
    prefix: &str,
    width: usize,
) -> Result<LayerNorm, WeightsErrorKind> {
    Ok(LayerNorm {
        gamma: tensor(file, &format!("{prefix}.weight"), &[width])?,
        beta: tensor(file, &format!("{prefix}.bias"), &[width])?,
    })
}

fn shape_of(file: &SafeTensors, name: &str) -> Result<Vec<usize>, WeightsErrorKind> {
    file.tensor(name)
        .map(|view| view.shape().to_vec())
        .map_err(|_| WeightsErrorKind::Missing {
            name: name.to_owned(),
        })
}

/// The parameter `name` as little-endian f32 values, checked to have `shape`.
fn tensor(file: &SafeTensors, name: &str, shape: &[usize]) -> Result<Vec<f32>, WeightsErrorKind> {
    let view = file.tensor(name).map_err(|_| WeightsErrorKind::Missing {
        name: name.to_owned(),
    })?;
    if view.dtype() != Dtype::F32 {
        return Err(WeightsErrorKind::Dtype {
            name: name.to_owned(),
            found: view.dtype(),
        });
    }
    if view.shape() != shape {
        return Err(WeightsErrorKind::Shape {
            name: name.to_owned(),
            expected: shape.to_vec(),
            found: view.shape().to_vec(),
        });
    }
    Ok(view
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| f32::from_le_bytes(*bytes))
        .collect())
}
