//! The army net's forward pass in f32 on one CPU core (DESIGN.md, «Body»), the same arithmetic as
//! `nn/aurelia_nn/army/model.py` (`tests/army_net.rs` and `army/parity.py` hold the logits equal).
//!
//! Weights: `"ARMW" u32 | json_len u32 | json {config, tensors: [{name, shape, offset}]} | f32 data`
//! (`army/export.py`). A matrix is stored `[in, out]` (MLX's `x @ W`).

use std::collections::BTreeMap;

use super::observe::{Candidate, Tokens, D_MAX, F_ACT, F_TOK};

pub const WEIGHTS_MAGIC: u32 = u32::from_le_bytes(*b"ARMW");

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Config {
    pub d: usize,
    pub heads: usize,
    pub layers: usize,
    pub ff: usize,
    pub f_tok: usize,
    pub f_act: usize,
    pub promos: usize,
    pub dmax: usize,
}

#[derive(Debug, serde::Deserialize)]
struct Entry {
    name: String,
    shape: Vec<usize>,
    offset: usize,
}

#[derive(Debug, serde::Deserialize)]
struct Header {
    config: Config,
    tensors: Vec<Entry>,
}

/// A dense layer `y = x W + b`, W stored transposed (`[out][in]`) for contiguous dot products.
#[derive(Debug, Clone)]
struct Dense {
    n_in: usize,
    n_out: usize,
    wt: Vec<f32>,
    b: Vec<f32>,
}

impl Dense {
    fn new(w: &[f32], shape: &[usize], b: &[f32]) -> Result<Dense, String> {
        let [n_in, n_out] = shape else { return Err(format!("a dense weight is 2-D, not {shape:?}")) };
        let (n_in, n_out) = (*n_in, *n_out);
        if b.len() != n_out {
            return Err(format!("bias {} for {n_out} outputs", b.len()));
        }
        let mut wt = vec![0.0; n_in * n_out];
        for i in 0..n_in {
            for o in 0..n_out {
                wt[o * n_in + i] = w[i * n_out + o];
            }
        }
        Ok(Dense { n_in, n_out, wt, b: b.to_vec() })
    }

    fn apply(&self, x: &[f32], y: &mut [f32]) {
        for o in 0..self.n_out {
            y[o] = self.b[o] + dot(&self.wt[o * self.n_in..(o + 1) * self.n_in], x);
        }
    }

    fn rows(&self, x: &[f32], n: usize) -> Vec<f32> {
        let mut y = vec![0.0; n * self.n_out];
        for r in 0..n {
            self.apply(&x[r * self.n_in..(r + 1) * self.n_in], &mut y[r * self.n_out..(r + 1) * self.n_out]);
        }
        y
    }
}

/// A dot product in eight lanes (the order MLX does not fix either; parity is within 1e-4).
#[inline]
fn dot(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    let mut acc = [0f32; 8];
    let chunks = n / 8;
    for c in 0..chunks {
        let (x, y) = (&a[c * 8..c * 8 + 8], &b[c * 8..c * 8 + 8]);
        for l in 0..8 {
            acc[l] += x[l] * y[l];
        }
    }
    let mut s = (acc[0] + acc[4]) + (acc[1] + acc[5]) + (acc[2] + acc[6]) + (acc[3] + acc[7]);
    for i in chunks * 8..n {
        s += a[i] * b[i];
    }
    s
}

#[derive(Debug, Clone)]
struct Norm {
    g: Vec<f32>,
    b: Vec<f32>,
}

impl Norm {
    fn apply(&self, x: &[f32], y: &mut [f32]) {
        let n = x.len() as f32;
        let mean = x.iter().sum::<f32>() / n;
        let var = x.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / n;
        let inv = 1.0 / (var + 1e-5).sqrt();
        for i in 0..x.len() {
            y[i] = (x[i] - mean) * inv * self.g[i] + self.b[i];
        }
    }
}

#[derive(Debug, Clone)]
struct Layer {
    norm1: Norm,
    q: Dense,
    k: Dense,
    v: Dense,
    o: Dense,
    /// `[heads][dmax + 1]`.
    bias: Vec<f32>,
    norm2: Norm,
    f1: Dense,
    f2: Dense,
}

/// The loaded net.
#[derive(Debug, Clone)]
pub struct Net {
    pub config: Config,
    e1: Dense,
    e2: Dense,
    layers: Vec<Layer>,
    norm: Norm,
    pointer: Dense,
    end: Dense,
    c1: Dense,
    promo: Vec<f32>,
    c2: Dense,
    s1: Dense,
    s2: Dense,
    v1: Dense,
    v2: Dense,
}

/// The token states after the body (`[T][d]`).
pub struct Body {
    pub h: Vec<f32>,
    pub n: usize,
}

fn relu(v: &mut [f32]) {
    for x in v.iter_mut() {
        *x = x.max(0.0);
    }
}

impl Net {
    pub fn load(path: &std::path::Path) -> Result<Net, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Net::from_bytes(&bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Net, String> {
        let u32_at = |i: usize| bytes.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        if u32_at(0) != Some(WEIGHTS_MAGIC) {
            return Err("not army-net weights".into());
        }
        let len = u32_at(4).ok_or("short header")? as usize;
        let header: Header = serde_json::from_slice(bytes.get(8..8 + len).ok_or("short header")?).map_err(|e| e.to_string())?;
        let data_at = 8 + len;
        let floats: Vec<f32> = bytes[data_at..].chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
        let mut t: BTreeMap<String, (Vec<usize>, Vec<f32>)> = BTreeMap::new();
        for e in header.tensors {
            let n: usize = e.shape.iter().product();
            let v = floats.get(e.offset..e.offset + n).ok_or(format!("tensor {} out of the data", e.name))?.to_vec();
            t.insert(e.name, (e.shape, v));
        }
        let c = header.config;
        if c.f_tok != F_TOK || c.f_act != F_ACT || c.dmax != usize::from(D_MAX) {
            return Err(format!("weights for f_tok {} f_act {} dmax {}, the code has {F_TOK} {F_ACT} {D_MAX}", c.f_tok, c.f_act, c.dmax));
        }
        let get = |name: &str| t.get(name).ok_or(format!("no tensor {name}"));
        let dense = |name: &str| -> Result<Dense, String> {
            let (shape, w) = get(&format!("{name}.weight"))?;
            let (_, b) = get(&format!("{name}.bias"))?;
            Dense::new(w, shape, b)
        };
        let norm = |name: &str| -> Result<Norm, String> { Ok(Norm { g: get(&format!("{name}.weight"))?.1.clone(), b: get(&format!("{name}.bias"))?.1.clone() }) };
        let mut layers = Vec::new();
        for l in 0..c.layers {
            let p = format!("layers.{l}");
            layers.push(Layer {
                norm1: norm(&format!("{p}.norm1"))?,
                q: dense(&format!("{p}.q"))?,
                k: dense(&format!("{p}.k"))?,
                v: dense(&format!("{p}.v"))?,
                o: dense(&format!("{p}.o"))?,
                bias: get(&format!("{p}.dist_bias"))?.1.clone(),
                norm2: norm(&format!("{p}.norm2"))?,
                f1: dense(&format!("{p}.f1"))?,
                f2: dense(&format!("{p}.f2"))?,
            });
        }
        Ok(Net {
            e1: dense("e1")?,
            e2: dense("e2")?,
            layers,
            norm: norm("norm")?,
            pointer: dense("pointer")?,
            end: dense("end")?,
            c1: dense("c1")?,
            promo: get("promo")?.1.clone(),
            c2: dense("c2")?,
            s1: dense("s1")?,
            s2: dense("s2")?,
            v1: dense("v1")?,
            v2: dense("v2")?,
            config: c,
        })
    }

    /// The token states: embedding, then the transformer layers with the distance bias.
    pub fn body(&self, tokens: &Tokens) -> Body {
        let n = tokens.len();
        let d = self.config.d;
        let flat: Vec<f32> = tokens.feat.iter().flat_map(|f| f.iter().copied()).collect();
        let mut e = self.e1.rows(&flat, n);
        relu(&mut e);
        let mut h = self.e2.rows(&e, n);
        let mut a = vec![0.0; n * d];
        for layer in &self.layers {
            self.attend(layer, &tokens.dist, n, &mut h, &mut a);
            self.feed(layer, n, &mut h, &mut a);
        }
        let mut out = vec![0.0; n * d];
        for i in 0..n {
            self.norm.apply(&h[i * d..(i + 1) * d], &mut out[i * d..(i + 1) * d]);
        }
        Body { h: out, n }
    }

    fn attend(&self, layer: &Layer, dist: &[u8], n: usize, h: &mut [f32], a: &mut [f32]) {
        let d = self.config.d;
        let heads = self.config.heads;
        let dh = d / heads;
        let nb = self.config.dmax + 1;
        for i in 0..n {
            layer.norm1.apply(&h[i * d..(i + 1) * d], &mut a[i * d..(i + 1) * d]);
        }
        let q = layer.q.rows(a, n);
        let k = layer.k.rows(a, n);
        let v = layer.v.rows(a, n);
        let scale = 1.0 / (dh as f32).sqrt();
        let mut ctx = vec![0.0; n * d];
        let mut s = vec![0.0; n];
        for hd in 0..heads {
            for i in 0..n {
                let qi = &q[i * d + hd * dh..i * d + hd * dh + dh];
                let mut max = f32::NEG_INFINITY;
                for j in 0..n {
                    let kj = &k[j * d + hd * dh..j * d + hd * dh + dh];
                    s[j] = dot(qi, kj) * scale + layer.bias[hd * nb + usize::from(dist[i * n + j])];
                    max = max.max(s[j]);
                }
                let mut sum = 0.0;
                for x in s.iter_mut().take(n) {
                    *x = (*x - max).exp();
                    sum += *x;
                }
                let out = &mut ctx[i * d + hd * dh..i * d + hd * dh + dh];
                for j in 0..n {
                    let w = s[j] / sum;
                    let vj = &v[j * d + hd * dh..j * d + hd * dh + dh];
                    for c in 0..dh {
                        out[c] += w * vj[c];
                    }
                }
            }
        }
        let o = layer.o.rows(&ctx, n);
        for (x, y) in h.iter_mut().zip(o) {
            *x += y;
        }
    }

    fn feed(&self, layer: &Layer, n: usize, h: &mut [f32], a: &mut [f32]) {
        let d = self.config.d;
        for i in 0..n {
            layer.norm2.apply(&h[i * d..(i + 1) * d], &mut a[i * d..(i + 1) * d]);
        }
        let mut f = layer.f1.rows(a, n);
        relu(&mut f);
        let g = layer.f2.rows(&f, n);
        for (x, y) in h.iter_mut().zip(g) {
            *x += y;
        }
    }

    /// The pointer's logits: one per token (None where not selectable), and the end's.
    pub fn pointer(&self, body: &Body, tokens: &Tokens) -> (Vec<Option<f32>>, f32) {
        let d = self.config.d;
        let mut y = [0f32; 1];
        let mut out = Vec::with_capacity(body.n);
        for i in 0..body.n {
            if tokens.selectable[i] {
                self.pointer.apply(&body.h[i * d..(i + 1) * d], &mut y);
                out.push(Some(y[0]));
            } else {
                out.push(None);
            }
        }
        self.end.apply(&body.h[0..d], &mut y);
        (out, y[0])
    }

    /// The action scorer's logits for the unit of token `unit` over its candidates.
    pub fn actions(&self, body: &Body, unit: usize, candidates: &[Candidate]) -> Vec<f32> {
        let d = self.config.d;
        let hu = &body.h[unit * d..(unit + 1) * d];
        let h0 = &body.h[0..d];
        let mut c1 = vec![0.0; d];
        let mut c = vec![0.0; d];
        let mut joint = vec![0.0; 4 * d];
        let mut s1 = vec![0.0; d];
        let mut y = [0f32; 1];
        let mut out = Vec::with_capacity(candidates.len());
        for cand in candidates {
            self.c1.apply(&cand.feat, &mut c1);
            let p = usize::from(cand.promo);
            for i in 0..d {
                c1[i] = (c1[i] + self.promo[p * d + i]).max(0.0);
            }
            self.c2.apply(&c1, &mut c);
            joint[..d].copy_from_slice(hu);
            joint[d..2 * d].copy_from_slice(h0);
            joint[2 * d..3 * d].copy_from_slice(&c);
            for i in 0..d {
                joint[3 * d + i] = hu[i] * c[i];
            }
            self.s1.apply(&joint, &mut s1);
            relu(&mut s1);
            self.s2.apply(&s1, &mut y);
            out.push(y[0]);
        }
        out
    }

    /// The value head: (the objective's win logit, the expected share of the army lost).
    pub fn value(&self, body: &Body) -> (f32, f32) {
        let d = self.config.d;
        let mut v = vec![0.0; d];
        self.v1.apply(&body.h[0..d], &mut v);
        relu(&mut v);
        let mut y = [0f32; 2];
        self.v2.apply(&v, &mut y);
        (y[0], y[1])
    }
}
