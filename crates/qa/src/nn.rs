//! A small multilayer perceptron trained in plain Rust: f32, one thread, its
//! own random numbers, so the same samples and configuration give the same
//! weights bit for bit. Every learned net of the game is trained with it
//! (the danger net by `ai-train-danger`, the site net by `ai-train-site`)
//! and runs in the game through the one integer inference
//! `classic_rules::ai::net`.
//!
//! The net: inputs clamped to the training range and standardised by the
//! training statistics → ReLU layers (`Config::hidden`) → one linear output
//! a head (`Config::heads`), each head fitting one target column with its
//! own loss: a regression (Huber), a probability (logistic) or an order
//! (pairwise logistic within the rows' groups, RankNet). Adam over shuffled
//! mini-batches (with a ranking head the batches are made of whole
//! groups); the weights of the epoch with the lowest validation loss are
//! kept, training stops after `patience` epochs without a better one.
//!
//! `quantise` turns a net into the game's integer weights (`ai::net`'s fixed
//! point); `weight_arrays` and `net_statics` write them as generated Rust.

use std::fmt::Write as _;

use classic_rules::ai::net::{Activation, Layer, ACT};

/// What a head fits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Loss {
    /// A regression with the Huber loss bending at this distance (target
    /// units; large: squared error).
    Huber(f32),
    /// A logit of a 0/1 target (log loss).
    Logistic,
    /// An order: for every pair of rows of one group with different
    /// targets, the log loss of the higher target scoring higher.
    Rank,
}

/// One output of the net: its loss and its share of the total loss.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Head {
    pub loss: Loss,
    pub weight: f32,
}

/// Training rows: inputs, one target a head, and a group (the rows a
/// ranking head compares; unused otherwise).
#[derive(Debug, Clone, Default)]
pub struct Samples {
    pub inputs: usize,
    pub targets: usize,
    /// Row-major, `inputs` values a row.
    pub x: Vec<f32>,
    /// Row-major, `targets` values a row.
    pub y: Vec<f32>,
    pub group: Vec<u64>,
}

impl Samples {
    pub fn new(inputs: usize, targets: usize) -> Self {
        Self { inputs, targets, ..Self::default() }
    }

    pub fn push(&mut self, x: &[f32], y: &[f32], group: u64) {
        assert_eq!(x.len(), self.inputs, "a row has `inputs` values");
        assert_eq!(y.len(), self.targets, "a row has `targets` targets");
        self.x.extend_from_slice(x);
        self.y.extend_from_slice(y);
        self.group.push(group);
    }

    pub fn len(&self) -> usize {
        self.group.len()
    }

    pub fn is_empty(&self) -> bool {
        self.group.is_empty()
    }

    pub fn row(&self, i: usize) -> &[f32] {
        &self.x[i * self.inputs..(i + 1) * self.inputs]
    }

    pub fn target(&self, i: usize, head: usize) -> f32 {
        self.y[i * self.targets + head]
    }

    /// The rows by group, each group in its rows' order, the groups in the
    /// order of their first rows.
    pub fn groups(&self) -> Vec<Vec<usize>> {
        let mut at: std::collections::HashMap<u64, usize> = std::collections::HashMap::new();
        let mut out: Vec<Vec<usize>> = Vec::new();
        for (r, g) in self.group.iter().enumerate() {
            let slot = *at.entry(*g).or_insert_with(|| {
                out.push(Vec::new());
                out.len() - 1
            });
            out[slot].push(r);
        }
        out
    }
}

/// How the net is trained.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// The widths of the hidden (ReLU) layers.
    pub hidden: Vec<usize>,
    /// The outputs, one a target column.
    pub heads: Vec<Head>,
    pub learning_rate: f32,
    pub batch: usize,
    pub max_epochs: usize,
    /// Epochs without a better validation loss before training stops.
    pub patience: usize,
    /// The smallest scale of an input: a nearly constant input is not
    /// blown up (and its folded integer weight stays small).
    pub min_scale: f32,
    pub seed: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self { hidden: vec![32, 16], heads: Vec::new(), learning_rate: 1e-3, batch: 256, max_epochs: 200, patience: 12, min_scale: 0.5, seed: 1 }
    }
}

impl Config {
    /// Reads a training option common to every net (`--hidden H1,H2,..`,
    /// `--epochs`, `--patience`, `--lr`, `--batch`, `--seed`); false: not one.
    pub fn parse_flag(&mut self, flag: &str, value: &mut dyn FnMut() -> Result<String, String>) -> Result<bool, String> {
        match flag {
            "--hidden" => {
                let text = value()?;
                let sizes: Vec<usize> = text.split(',').map(|s| s.parse().map_err(|e| format!("--hidden {text}: {e}"))).collect::<Result<_, _>>()?;
                if sizes.is_empty() || sizes.contains(&0) {
                    return Err(format!("--hidden {text}: positive sizes"));
                }
                self.hidden = sizes;
            }
            "--epochs" => self.max_epochs = value()?.parse().map_err(|e| format!("--epochs: {e}"))?,
            "--patience" => self.patience = value()?.parse().map_err(|e| format!("--patience: {e}"))?,
            "--lr" => self.learning_rate = value()?.parse().map_err(|e| format!("--lr: {e}"))?,
            "--batch" => self.batch = value()?.parse().map_err(|e| format!("--batch: {e}"))?,
            "--seed" => self.seed = value()?.parse().map_err(|e| format!("--seed: {e}"))?,
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// The common options as `parse_flag` reads them.
    pub fn flags(&self) -> String {
        let hidden: Vec<String> = self.hidden.iter().map(|h| h.to_string()).collect();
        format!("--hidden {} --epochs {} --patience {} --lr {} --batch {}", hidden.join(","), self.max_epochs, self.patience, self.learning_rate, self.batch)
    }

    fn ranks(&self) -> bool {
        self.heads.iter().any(|h| h.loss == Loss::Rank)
    }
}

/// A fully connected layer: `w[j * inputs + i]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Dense {
    pub inputs: usize,
    pub outputs: usize,
    pub w: Vec<f32>,
    pub b: Vec<f32>,
}

impl Dense {
    fn new(inputs: usize, outputs: usize, bound: f32, rng: &mut Rng) -> Self {
        let w = (0..inputs * outputs).map(|_| (rng.unit() * 2.0 - 1.0) * bound).collect();
        Self { inputs, outputs, w, b: vec![0.0; outputs] }
    }

    fn forward(&self, input: &[f32], out: &mut [f32], relu: bool) {
        for (j, slot) in out.iter_mut().enumerate() {
            let row = &self.w[j * self.inputs..(j + 1) * self.inputs];
            let sum = row.iter().zip(input).fold(self.b[j], |acc, (w, x)| acc + w * x);
            *slot = if relu { sum.max(0.0) } else { sum };
        }
    }

    /// Adds the gradient of the layer for `grad_out` at `input` to `grad`,
    /// and writes the gradient of the input to `grad_in` (if any).
    fn backward(&self, input: &[f32], grad_out: &[f32], grad: &mut Dense, grad_in: Option<&mut [f32]>) {
        for (j, g) in grad_out.iter().enumerate() {
            if *g == 0.0 {
                continue;
            }
            grad.b[j] += g;
            let row = &mut grad.w[j * self.inputs..(j + 1) * self.inputs];
            for (w, x) in row.iter_mut().zip(input) {
                *w += g * x;
            }
        }
        if let Some(grad_in) = grad_in {
            for (i, slot) in grad_in.iter_mut().enumerate() {
                *slot = grad_out.iter().enumerate().map(|(j, g)| g * self.w[j * self.inputs + i]).sum();
            }
        }
    }

    fn zero(&mut self) {
        self.w.fill(0.0);
        self.b.fill(0.0);
    }
}

/// The trained net with its input statistics.
#[derive(Debug, Clone, PartialEq)]
pub struct Net {
    /// The training range of every input (inputs are clamped to it).
    pub low: Vec<f32>,
    pub high: Vec<f32>,
    pub mean: Vec<f32>,
    pub scale: Vec<f32>,
    /// The hidden layers (ReLU), then the output layer (one unit a head).
    pub layers: Vec<Dense>,
}

pub fn sigmoid(z: f32) -> f32 {
    1.0 / (1.0 + (-z).exp())
}

impl Net {
    pub fn inputs(&self) -> usize {
        self.low.len()
    }

    pub fn outputs(&self) -> usize {
        self.layers.last().map_or(0, |l| l.outputs)
    }

    /// The clamped and standardised row.
    pub fn standardise(&self, x: &[f32], out: &mut [f32]) {
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = (x[i].clamp(self.low[i], self.high[i]) - self.mean[i]) / self.scale[i];
        }
    }

    /// The heads' outputs for a row of raw inputs.
    pub fn forward(&self, x: &[f32]) -> Vec<f32> {
        let mut buffers = Buffers::new(self);
        self.standardise(x, &mut buffers.acts[0]);
        self.forward_standard(&mut buffers);
        buffers.acts.last().expect("an output layer").clone()
    }

    /// The layers on `b.acts[0]` (standardised), every activation kept.
    fn forward_standard(&self, b: &mut Buffers) {
        let last = self.layers.len() - 1;
        for (k, layer) in self.layers.iter().enumerate() {
            let (before, after) = b.acts.split_at_mut(k + 1);
            layer.forward(&before[k], &mut after[0], k < last);
        }
    }
}

/// The activations of one row: the standardised input, then every layer's.
#[derive(Debug, Clone)]
struct Buffers {
    acts: Vec<Vec<f32>>,
}

impl Buffers {
    fn new(net: &Net) -> Self {
        let mut acts = vec![vec![0.0; net.inputs()]];
        acts.extend(net.layers.iter().map(|l| vec![0.0; l.outputs]));
        Self { acts }
    }
}

/// xorshift64*: the trainer's random numbers.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in [0, 1).
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Uniform in 0..n.
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    /// The items in a random order (Fisher–Yates).
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            items.swap(i, self.below(i + 1));
        }
    }
}

/// One epoch of training.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Epoch {
    pub train_loss: f32,
    pub valid_loss: f32,
}

/// `log(1 + exp(z)) − t·z`, stable for both signs.
fn log_loss(z: f32, t: f32) -> f32 {
    z.max(0.0) - z * t + (-z.abs()).exp().ln_1p()
}

/// The loss of one row's pointwise heads (a ranking head adds nothing
/// here: `pair_loss`).
fn row_loss(out: &[f32], targets: &[f32], heads: &[Head]) -> f32 {
    let mut total = 0f32;
    for (k, head) in heads.iter().enumerate() {
        let loss = match head.loss {
            Loss::Huber(delta) => {
                let e = (out[k] - targets[k]).abs();
                if e <= delta {
                    0.5 * e * e
                } else {
                    delta * (e - 0.5 * delta)
                }
            }
            Loss::Logistic => log_loss(out[k], targets[k]),
            Loss::Rank => continue,
        };
        total += head.weight * loss;
    }
    total
}

/// The gradient of `row_loss` by the outputs, times `share`.
fn row_gradient(out: &[f32], targets: &[f32], heads: &[Head], share: f32, grad: &mut [f32]) {
    for (k, head) in heads.iter().enumerate() {
        grad[k] = match head.loss {
            Loss::Huber(delta) => head.weight * (out[k] - targets[k]).clamp(-delta, delta) * share,
            Loss::Logistic => head.weight * (sigmoid(out[k]) - targets[k]) * share,
            Loss::Rank => 0.0,
        };
    }
}

/// The pairs a ranking head compares among `rows` (one group): those with
/// different targets, the higher first.
fn pairs(rows: &[usize], target: impl Fn(usize) -> f32) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (a, &i) in rows.iter().enumerate() {
        for &j in &rows[a + 1..] {
            let (ti, tj) = (target(i), target(j));
            if ti > tj {
                out.push((i, j));
            } else if tj > ti {
                out.push((j, i));
            }
        }
    }
    out
}

/// The mean loss of the net on the samples: the pointwise heads by row, a
/// ranking head by pair of its groups.
pub fn mean_loss(net: &Net, samples: &Samples, config: &Config) -> f32 {
    let mut buffers = Buffers::new(net);
    let mut total = 0f64;
    let mut outputs = vec![0f32; samples.len() * net.outputs()];
    let n = net.outputs();
    for i in 0..samples.len() {
        net.standardise(samples.row(i), &mut buffers.acts[0]);
        net.forward_standard(&mut buffers);
        let out = buffers.acts.last().expect("an output layer");
        total += f64::from(row_loss(out, &samples.y[i * samples.targets..(i + 1) * samples.targets], &config.heads));
        outputs[i * n..(i + 1) * n].copy_from_slice(out);
    }
    let mut loss = total / samples.len().max(1) as f64;
    if config.ranks() {
        let groups = samples.groups();
        for (k, head) in config.heads.iter().enumerate().filter(|(_, h)| h.loss == Loss::Rank) {
            let (mut sum, mut count) = (0f64, 0usize);
            for rows in &groups {
                for (hi, lo) in pairs(rows, |r| samples.target(r, k)) {
                    sum += f64::from(log_loss(outputs[lo * n + k] - outputs[hi * n + k], 0.0));
                    count += 1;
                }
            }
            loss += f64::from(head.weight) * sum / count.max(1) as f64;
        }
    }
    loss as f32
}

/// The input statistics of the training rows: range, mean and scale
/// (the standard deviation, at least `min_scale`).
fn statistics(samples: &Samples, min_scale: f32) -> [Vec<f32>; 4] {
    let n = samples.inputs;
    let mut low = vec![f32::INFINITY; n];
    let mut high = vec![f32::NEG_INFINITY; n];
    let mut sum = vec![0f64; n];
    let mut squares = vec![0f64; n];
    for r in 0..samples.len() {
        for (i, v) in samples.row(r).iter().enumerate() {
            low[i] = low[i].min(*v);
            high[i] = high[i].max(*v);
            sum[i] += f64::from(*v);
            squares[i] += f64::from(*v) * f64::from(*v);
        }
    }
    let count = samples.len().max(1) as f64;
    let mean: Vec<f32> = sum.iter().map(|s| (s / count) as f32).collect();
    let scale = (0..n).map(|i| (((squares[i] / count) - (sum[i] / count).powi(2)).max(0.0).sqrt() as f32).max(min_scale)).collect();
    [low, high, mean, scale]
}

/// Adam's moments of one layer.
struct Moments {
    m: Dense,
    v: Dense,
}

/// Trains a net on `train`, keeping the weights of the epoch with the lowest
/// loss on `valid`; returns it and the log of the epochs.
pub fn train(train: &Samples, valid: &Samples, config: &Config) -> (Net, Vec<Epoch>) {
    assert!(!train.is_empty() && !valid.is_empty(), "training needs rows in both sets");
    assert!(!config.heads.is_empty() && config.heads.len() == train.targets && train.targets == valid.targets, "a head a target");
    let n = train.inputs;
    let outputs = config.heads.len();
    let mut rng = Rng::new(config.seed);
    let [low, high, mean, scale] = statistics(train, config.min_scale);
    let mut layers = Vec::new();
    let mut width = n;
    for &h in &config.hidden {
        layers.push(Dense::new(width, h, (6.0 / width as f32).sqrt(), &mut rng));
        width = h;
    }
    layers.push(Dense::new(width, outputs, (6.0 / (width + outputs) as f32).sqrt(), &mut rng));
    // The heads start at the training means: a regression's mean, a
    // probability's base rate as a logit; an order at 0.
    let count = train.len() as f32;
    let column = |k: usize| (0..train.len()).map(move |r| train.target(r, k));
    let last = layers.last_mut().expect("an output layer");
    for (k, head) in config.heads.iter().enumerate() {
        last.b[k] = match head.loss {
            Loss::Huber(_) => column(k).sum::<f32>() / count,
            Loss::Logistic => {
                let rate = (column(k).sum::<f32>() / count).clamp(1e-4, 1.0 - 1e-4);
                (rate / (1.0 - rate)).ln()
            }
            Loss::Rank => 0.0,
        };
    }
    let mut net = Net { low, high, mean, scale, layers };

    // The standardised training inputs, once.
    let mut standard = vec![0f32; train.x.len()];
    for r in 0..train.len() {
        net.standardise(train.row(r), &mut standard[r * n..(r + 1) * n]);
    }

    let mut grads = net.layers.clone();
    let mut moments: Vec<Moments> = net
        .layers
        .iter()
        .map(|l| {
            let mut zero = l.clone();
            zero.zero();
            Moments { m: zero.clone(), v: zero }
        })
        .collect();
    let (beta1, beta2, eps) = (0.9f32, 0.999f32, 1e-8f32);
    let (mut beta1_t, mut beta2_t) = (1f32, 1f32);

    // The shuffled units: rows, or whole groups with a ranking head.
    let mut units: Vec<Vec<usize>> = if config.ranks() { train.groups() } else { (0..train.len()).map(|r| vec![r]).collect() };
    let rank_heads: Vec<usize> = (0..outputs).filter(|k| config.heads[*k].loss == Loss::Rank).collect();
    let mut acts: Vec<Buffers> = Vec::new();
    let mut g_out: Vec<f32> = Vec::new();
    // The gradient by every layer's outputs, of one row.
    let mut g_layers: Vec<Vec<f32>> = net.layers.iter().map(|l| vec![0f32; l.outputs]).collect();
    let mut best = (mean_loss(&net, valid, config), net.clone());
    let mut epochs = Vec::new();
    let mut stale = 0;
    let mut batch: Vec<usize> = Vec::new();
    let mut groups_of_batch: Vec<std::ops::Range<usize>> = Vec::new();
    for _ in 0..config.max_epochs {
        rng.shuffle(&mut units);
        let mut total = 0f64;
        let mut next = 0;
        while next < units.len() {
            batch.clear();
            groups_of_batch.clear();
            while next < units.len() && batch.len() < config.batch {
                let start = batch.len();
                batch.extend_from_slice(&units[next]);
                groups_of_batch.push(start..batch.len());
                next += 1;
            }
            grads.iter_mut().for_each(Dense::zero);
            let share = 1.0 / batch.len() as f32;
            // Forward every row of the batch, then the heads' gradients.
            while acts.len() < batch.len() {
                acts.push(Buffers::new(&net));
            }
            g_out.clear();
            g_out.resize(batch.len() * outputs, 0.0);
            for (at, &r) in batch.iter().enumerate() {
                let b = &mut acts[at];
                b.acts[0].copy_from_slice(&standard[r * n..(r + 1) * n]);
                net.forward_standard(b);
                let out = b.acts.last().expect("an output layer");
                let targets = &train.y[r * outputs..(r + 1) * outputs];
                total += f64::from(row_loss(out, targets, &config.heads));
                row_gradient(out, targets, &config.heads, share, &mut g_out[at * outputs..(at + 1) * outputs]);
            }
            for &k in &rank_heads {
                let weight = config.heads[k].weight;
                let mut found: Vec<(usize, usize)> = Vec::new();
                for range in &groups_of_batch {
                    let local: Vec<usize> = range.clone().collect();
                    found.extend(pairs(&local, |at| train.target(batch[at], k)));
                }
                let pair_share = weight / found.len().max(1) as f32;
                let mut pair_total = 0f64;
                for (hi, lo) in &found {
                    let score = |at: usize| acts[at].acts.last().expect("an output layer")[k];
                    let z = score(*lo) - score(*hi);
                    pair_total += f64::from(log_loss(z, 0.0));
                    let g = sigmoid(z) * pair_share;
                    g_out[lo * outputs + k] += g;
                    g_out[hi * outputs + k] -= g;
                }
                // The log's training loss: the pairs' mean loss counted once a row.
                total += f64::from(weight) * pair_total / found.len().max(1) as f64 * batch.len() as f64;
            }
            // Backward every row in the batch's order.
            let top = net.layers.len() - 1;
            for (at, b) in acts.iter().enumerate().take(batch.len()) {
                g_layers[top].copy_from_slice(&g_out[at * outputs..(at + 1) * outputs]);
                for l in (1..=top).rev() {
                    let (below, above) = g_layers.split_at_mut(l);
                    let grad_in = &mut below[l - 1];
                    net.layers[l].backward(&b.acts[l], &above[0], &mut grads[l], Some(grad_in));
                    for (g, a) in grad_in.iter_mut().zip(&b.acts[l]) {
                        if *a <= 0.0 {
                            *g = 0.0;
                        }
                    }
                }
                net.layers[0].backward(&b.acts[0], &g_layers[0], &mut grads[0], None);
            }
            beta1_t *= beta1;
            beta2_t *= beta2;
            let step = config.learning_rate * (1.0 - beta2_t).sqrt() / (1.0 - beta1_t);
            for ((layer, grad), moment) in net.layers.iter_mut().zip(&grads).zip(&mut moments) {
                let params = layer.w.iter_mut().chain(layer.b.iter_mut());
                let gs = grad.w.iter().chain(&grad.b);
                let ms = moment.m.w.iter_mut().chain(moment.m.b.iter_mut());
                let vs = moment.v.w.iter_mut().chain(moment.v.b.iter_mut());
                for (((p, g), m), v) in params.zip(gs).zip(ms).zip(vs) {
                    *m = beta1 * *m + (1.0 - beta1) * g;
                    *v = beta2 * *v + (1.0 - beta2) * g * g;
                    *p -= step * *m / (v.sqrt() + eps);
                }
            }
        }
        let valid_loss = mean_loss(&net, valid, config);
        epochs.push(Epoch { train_loss: (total / train.len() as f64) as f32, valid_loss });
        if valid_loss < best.0 {
            best = (valid_loss, net.clone());
            stale = 0;
        } else {
            stale += 1;
            if stale >= config.patience {
                break;
            }
        }
    }
    (best.1, epochs)
}

/// One layer of a net in the game's fixed point (`ai::net::Layer`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuantisedLayer {
    pub w: Vec<i32>,
    pub b: Vec<i64>,
    pub shift: u32,
    pub activation: Activation,
}

/// A net's weights in the game's fixed point (`ai::net`): the inputs are
/// integers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quantised {
    pub low: Vec<i32>,
    pub high: Vec<i32>,
    pub layers: Vec<QuantisedLayer>,
}

impl Quantised {
    /// The layers as `ai::net` takes them; `net` makes the net of them.
    pub fn layers(&self) -> Vec<Layer<'_>> {
        self.layers.iter().map(|l| Layer { w: &l.w, b: &l.b, shift: l.shift, activation: l.activation }).collect()
    }

    pub fn net<'q>(&'q self, layers: &'q [Layer<'q>]) -> classic_rules::ai::net::Net<'q> {
        classic_rules::ai::net::Net { low: &self.low, high: &self.high, layers }
    }

    pub fn parameters(&self) -> usize {
        self.layers.iter().map(|l| l.w.len() + l.b.len()).sum()
    }
}

/// The largest shift, at most `cap`, that keeps `max_abs × 2^(extra + shift)`
/// within 2^30 (an i32 with room).
fn shift_for(max_abs: f64, extra: u32, cap: u32) -> u32 {
    (0..=cap).rev().find(|s| max_abs * 2f64.powi((extra + s) as i32) <= 2f64.powi(30)).expect("the weights fit an i32 at shift 0")
}

/// A layer whose input carries `input_bits` fraction bits: weights at
/// `2^(ACT − input_bits + shift)`, biases at `2^(ACT + shift)`, so the sum
/// drops `shift` bits to Q16 (`ai::net`).
fn quantise_layer(w: &[f64], b: &[f64], input_bits: u32, cap: u32, activation: Activation) -> QuantisedLayer {
    let max_abs = w.iter().fold(0f64, |m, v| m.max(v.abs()));
    let shift = shift_for(max_abs, ACT - input_bits, cap);
    let w_unit = 2f64.powi((ACT - input_bits + shift) as i32);
    let b_unit = 2f64.powi((ACT + shift) as i32);
    QuantisedLayer {
        w: w.iter().map(|v| (v * w_unit).round() as i32).collect(),
        b: b.iter().map(|v| (v * b_unit).round() as i64).collect(),
        shift,
        activation,
    }
}

/// The net in the game's fixed point: the standardisation folded into the
/// first layer (`ai::net`). The net's inputs must be integers (their range
/// is kept as integers).
pub fn quantise(net: &Net) -> Quantised {
    let first = &net.layers[0];
    let n = first.inputs;
    let mut w1 = vec![0f64; first.w.len()];
    let mut b1 = vec![0f64; first.outputs];
    for j in 0..first.outputs {
        let mut bias = f64::from(first.b[j]);
        for i in 0..n {
            let w = f64::from(first.w[j * n + i]) / f64::from(net.scale[i]);
            w1[j * n + i] = w;
            bias -= w * f64::from(net.mean[i]);
        }
        b1[j] = bias;
    }
    let widen = |v: &[f32]| v.iter().map(|x| f64::from(*x)).collect::<Vec<f64>>();
    let last = net.layers.len() - 1;
    let activation = |k: usize| if k < last { Activation::Relu } else { Activation::Linear };
    let mut layers = vec![quantise_layer(&w1, &b1, 0, 12, activation(0))];
    for (k, layer) in net.layers.iter().enumerate().skip(1) {
        layers.push(quantise_layer(&widen(&layer.w), &widen(&layer.b), ACT, 20, activation(k)));
    }
    Quantised { low: net.low.iter().map(|v| *v as i32).collect(), high: net.high.iter().map(|v| *v as i32).collect(), layers }
}

/// Values as Rust array items, wrapped.
pub fn items<T: std::fmt::Display>(values: &[T]) -> String {
    let mut out = String::new();
    for chunk in values.chunks(16) {
        let line: Vec<String> = chunk.iter().map(|v| v.to_string()).collect();
        let _ = writeln!(out, "    {},", line.join(", "));
    }
    out
}

/// The quantised net as generated Rust statics: `LOW`, `HIGH` and every
/// layer's `W{k}` and `B{k}` from 1. `widths` names the inputs' and every
/// layer's width as the generated file knows them (`["FEATURES", "H1",
/// "H2", "2"]`).
pub fn weight_arrays(q: &Quantised, widths: &[&str]) -> String {
    assert_eq!(widths.len(), q.layers.len() + 1, "a width for the inputs and every layer");
    let mut out = String::new();
    let array = |w: &mut String, name: &str, ty: &str, len: &str, body: String| {
        let _ = writeln!(w, "static {name}: [{ty}; {len}] = [\n{body}];\n");
    };
    array(&mut out, "LOW", "i32", widths[0], items(&q.low));
    array(&mut out, "HIGH", "i32", widths[0], items(&q.high));
    for (k, layer) in q.layers.iter().enumerate() {
        array(&mut out, &format!("W{}", k + 1), "i32", &format!("{} * {}", widths[k + 1], widths[k]), items(&layer.w));
        array(&mut out, &format!("B{}", k + 1), "i64", widths[k + 1], items(&layer.b));
    }
    out
}

/// The layers and the net of `weight_arrays` as `ai::net` statics: `LAYERS`
/// and `pub static {name}: Net`.
pub fn net_statics(q: &Quantised, name: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "static LAYERS: [Layer<'static>; {}] = [", q.layers.len());
    for (k, layer) in q.layers.iter().enumerate() {
        let _ = writeln!(out, "    Layer {{ w: &W{0}, b: &B{0}, shift: {1}, activation: Activation::{2:?} }},", k + 1, layer.shift, layer.activation);
    }
    let _ = writeln!(out, "];\n");
    let _ = writeln!(out, "pub static {name}: Net<'static> = Net {{ low: &LOW, high: &HIGH, layers: &LAYERS }};");
    out
}
