//! The few tensor operations the forward pass needs, on row-major `f32` slices: a linear layer
//! (PyTorch's `nn.Linear`), a strided matrix product, layer normalisation, ReLU, and a softmax.
//! The products go through `matrixmultiply`'s `sgemm`; everything else is plain loops.

/// A row-major matrix product with arbitrary strides: `c = a · b + beta · c`, `a` being
/// `m × k` and `b` being `k × n`, each given as its slice with its row and column strides.
pub(crate) struct Product<'a> {
    pub a: (&'a [f32], usize, usize),
    pub b: (&'a [f32], usize, usize),
    pub m: usize,
    pub k: usize,
    pub n: usize,
}

impl Product<'_> {
    /// Accumulate the product into `c` (row stride `c_row`, unit column stride), scaled by `alpha`,
    /// keeping `beta` times what `c` held.
    pub fn accumulate(&self, alpha: f32, beta: f32, c: &mut [f32], c_row: usize) {
        let (a, a_row, a_col) = self.a;
        let (b, b_row, b_col) = self.b;
        assert!(
            a.len() >= extent(self.m, self.k, a_row, a_col),
            "a is too short"
        );
        assert!(
            b.len() >= extent(self.k, self.n, b_row, b_col),
            "b is too short"
        );
        assert!(
            c.len() >= extent(self.m, self.n, c_row, 1),
            "c is too short"
        );
        // SAFETY: the asserts above keep every element sgemm reads or writes within the slices.
        unsafe {
            matrixmultiply::sgemm(
                self.m,
                self.k,
                self.n,
                alpha,
                a.as_ptr(),
                a_row as isize,
                a_col as isize,
                b.as_ptr(),
                b_row as isize,
                b_col as isize,
                beta,
                c.as_mut_ptr(),
                c_row as isize,
                1,
            );
        }
    }
}

/// A fully connected layer, `y = x · Wᵀ + b`, with `W` stored as PyTorch stores it: one row of
/// `inputs` weights per output.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Linear {
    pub weight: Vec<f32>,
    pub bias: Vec<f32>,
    pub inputs: usize,
    pub outputs: usize,
}

impl Linear {
    /// The layer applied to each of `x`'s rows (`x` holds `rows × inputs` values).
    pub fn forward(&self, x: &[f32], rows: usize) -> Vec<f32> {
        debug_assert_eq!(x.len(), rows * self.inputs);
        let mut y: Vec<f32> = (0..rows).flat_map(|_| self.bias.iter().copied()).collect();
        Product {
            a: (x, self.inputs, 1),
            // Wᵀ: element (i, j) is W[j][i].
            b: (&self.weight, 1, self.inputs),
            m: rows,
            k: self.inputs,
            n: self.outputs,
        }
        .accumulate(1.0, 1.0, &mut y, self.outputs);
        y
    }
}

/// PyTorch's `nn.LayerNorm` over each row of `width` values, in place: the biased variance and the
/// default epsilon.
pub(crate) fn layer_norm(x: &mut [f32], width: usize, gamma: &[f32], beta: &[f32]) {
    for row in x.chunks_exact_mut(width) {
        let mean = row.iter().sum::<f32>() / width as f32;
        let variance = row.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / width as f32;
        let scale = 1.0 / (variance + LAYER_NORM_EPS).sqrt();
        for ((v, g), b) in row.iter_mut().zip(gamma).zip(beta) {
            *v = (*v - mean) * scale * g + b;
        }
    }
}

pub(crate) fn relu(x: &mut [f32]) {
    for v in x {
        *v = v.max(0.0);
    }
}

/// A softmax over each row of `width` values, in place.
pub(crate) fn softmax_rows(x: &mut [f32], width: usize) {
    for row in x.chunks_exact_mut(width) {
        let max = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let mut sum = 0.0;
        for v in row.iter_mut() {
            *v = (*v - max).exp();
            sum += *v;
        }
        for v in row.iter_mut() {
            *v /= sum;
        }
    }
}

/// How many elements a strided `rows × cols` matrix spans, from its first element to its last.
fn extent(rows: usize, cols: usize, row_stride: usize, col_stride: usize) -> usize {
    if rows == 0 || cols == 0 {
        0
    } else {
        (rows - 1) * row_stride + (cols - 1) * col_stride + 1
    }
}

/// `nn.LayerNorm`'s default epsilon.
const LAYER_NORM_EPS: f32 = 1e-5;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_matches_a_hand_computed_product() {
        let layer = Linear {
            weight: vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
            bias: vec![0.5, -1.0],
            inputs: 3,
            outputs: 2,
        };
        let y = layer.forward(&[1.0, 0.0, -1.0, 2.0, 1.0, 0.0], 2);
        assert_eq!(
            y,
            vec![
                1.0 - 3.0 + 0.5,
                4.0 - 6.0 - 1.0,
                2.0 + 2.0 + 0.5,
                8.0 + 5.0 - 1.0
            ]
        );
    }

    #[test]
    fn layer_norm_centres_and_scales() {
        let mut x = vec![1.0, 2.0, 3.0, 4.0];
        layer_norm(&mut x, 4, &[1.0; 4], &[0.0; 4]);
        let mean: f32 = x.iter().sum::<f32>() / 4.0;
        let variance: f32 = x.iter().map(|v| v * v).sum::<f32>() / 4.0;
        assert!(mean.abs() < 1e-6 && (variance - 1.0).abs() < 1e-4);
    }

    #[test]
    fn softmax_rows_sum_to_one() {
        let mut x = vec![1.0, 2.0, 3.0, 1000.0, 1000.0, 1000.0];
        softmax_rows(&mut x, 3);
        assert!((x[..3].iter().sum::<f32>() - 1.0).abs() < 1e-6);
        assert!(x[3..].iter().all(|v| (v - 1.0 / 3.0).abs() < 1e-6));
    }
}
