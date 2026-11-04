//! Integer inference of the AI's small learned nets (multilayer
//! perceptrons trained offline by `aurelia_qa::nn`): every net the AI
//! consults (`danger_net`, `site_net`) runs through this one function, its
//! shape, scales and activations being data of the generated weights.
//!
//! Integer only, so the game gets the same answer on every platform:
//!
//! - inputs: the row's raw integers, each clamped to the range seen in
//!   training (`Net::low`, `Net::high`);
//! - the first layer has the training standardisation `(x − mean) / scale`
//!   folded into it: weights `w / scale` as i32 and biases
//!   `b − Σ w·mean/scale` as i64, both at `2^(ACT + shift)`;
//! - every later layer takes Q16 inputs: weights i32 at `2^shift`, biases
//!   i64 at `2^(ACT + shift)`;
//! - each layer's sum (an i64) is brought back to Q16 by a rounded shift of
//!   its `shift` bits (the shifts are generated with the weights: the
//!   largest that keep every weight in an i32), then its activation;
//! - the outputs: Q16 (`ACT`, value × 65536); what they mean is the
//!   model's (`sigmoid_ppm` for a logit).
//!
//! The trainer runs this same function on its quantised weights, so its
//! reports of the integer nets are the game's numbers.

/// The fraction bits of the activations between the layers (Q16).
pub const ACT: u32 = 16;

/// The widest layer, inputs included, the inference takes (stack buffers).
pub const MAX_WIDTH: usize = 256;

/// What a layer does to its sums.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activation {
    /// max(0, x).
    Relu,
    /// x: an output layer.
    Linear,
}

/// One fully connected layer in the fixed point (the module docs). Rows
/// are by output: `w[j * inputs + i]` is the weight of input `i` in unit
/// `j`; the layer's width is `b.len()`.
#[derive(Debug, Clone, Copy)]
pub struct Layer<'w> {
    pub w: &'w [i32],
    pub b: &'w [i64],
    pub shift: u32,
    pub activation: Activation,
}

/// A net's quantised weights: the inputs' training range and the layers.
#[derive(Debug, Clone, Copy)]
pub struct Net<'w> {
    pub low: &'w [i32],
    pub high: &'w [i32],
    pub layers: &'w [Layer<'w>],
}

impl Net<'_> {
    pub fn inputs(&self) -> usize {
        self.low.len()
    }

    pub fn outputs(&self) -> usize {
        self.layers.last().map_or(0, |l| l.b.len())
    }
}

/// The net's outputs (Q16) for a row of `net.inputs()` integers, written to
/// `out` (`net.outputs()` values).
pub fn forward(net: &Net<'_>, features: &[i32], out: &mut [i64]) {
    let inputs = net.inputs();
    assert!(features.len() == inputs && inputs <= MAX_WIDTH && out.len() == net.outputs(), "the row and the net agree in shape");
    // Two buffers, the layers' input and output by turns.
    let mut buffers = [[0i64; MAX_WIDTH]; 2];
    for (i, value) in features.iter().enumerate() {
        buffers[0][i] = i64::from((*value).clamp(net.low[i], net.high[i]));
    }
    let mut width = inputs;
    for (k, layer) in net.layers.iter().enumerate() {
        let next = layer.b.len();
        assert!(next <= MAX_WIDTH && layer.w.len() == next * width, "the layers agree in shape");
        let [first, second] = &mut buffers;
        let (input, output) = if k % 2 == 0 { (first, second) } else { (second, first) };
        dense(&input[..width], layer, &mut output[..next]);
        width = next;
    }
    out.copy_from_slice(&buffers[net.layers.len() % 2][..width]);
}

/// One layer: `out[j] = Σ w[j][i] · input[i] + b[j]`, brought to Q16 by a
/// rounded shift, then the activation.
fn dense(input: &[i64], layer: &Layer<'_>, out: &mut [i64]) {
    let n = input.len();
    for (j, slot) in out.iter_mut().enumerate() {
        let row = &layer.w[j * n..(j + 1) * n];
        let sum = row.iter().zip(input).fold(layer.b[j], |acc, (w, x)| acc + i64::from(*w) * x);
        let value = round_shift(sum, layer.shift);
        *slot = match layer.activation {
            Activation::Relu => value.max(0),
            Activation::Linear => value,
        };
    }
}

/// `value / 2^shift`, rounded half up.
pub fn round_shift(value: i64, shift: u32) -> i64 {
    if shift == 0 {
        return value;
    }
    (value + (1 << (shift - 1))) >> shift
}

/// The logistic function in millionths at the logits k/4, k in −32..=32.
const SIGMOID_PPM: [i64; 65] = [
    335, 431, 553, 710, 911, 1170, 1501, 1927, 2473, 3173, 4070, 5220, 6693, 8577, 10987, 14064, 17986, 22977, 29312, 37327, 47426, 60087,
    75858, 95349, 119203, 148047, 182426, 222700, 268941, 320821, 377541, 437823, 500000, 562177, 622459, 679179, 731059, 777300, 817574,
    851953, 880797, 904651, 924142, 939913, 952574, 962673, 970688, 977023, 982014, 985936, 989013, 991423, 993307, 994780, 995930, 996827,
    997527, 998073, 998499, 998830, 999089, 999290, 999447, 999569, 999665,
];

/// The logistic of a Q16 logit in millionths: piecewise linear between the
/// points of `SIGMOID_PPM` (logits k/4, k in −32..=32; beyond: the ends;
/// error under 1 permille).
pub fn sigmoid_ppm(logit: i64) -> i64 {
    // The position on the table in Q16 steps of 1/4.
    let at = logit * 4 + (32 << ACT);
    if at <= 0 {
        return SIGMOID_PPM[0];
    }
    let last = SIGMOID_PPM.len() - 1;
    if at >= (last as i64) << ACT {
        return SIGMOID_PPM[last];
    }
    let i = (at >> ACT) as usize;
    let frac = at & ((1 << ACT) - 1);
    SIGMOID_PPM[i] + (((SIGMOID_PPM[i + 1] - SIGMOID_PPM[i]) * frac) >> ACT)
}
