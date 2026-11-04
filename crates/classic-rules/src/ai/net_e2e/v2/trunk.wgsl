// The v2 trunk's compute kernels (gpu.rs; classic/docs/ai-net-model.md,
// «v2 in-process (GPU)»). WebGPU-compatible WGSL only: no subgroups, no f16,
// no atomics. Every output is computed by one invocation, or by one workgroup
// through a fixed reduction tree, with the same order of operations whatever
// the batch: a run repeats byte for byte on the same machine. Where the
// adapter has the cooperative matrix, gpu.rs appends trunk_mma.wgsl (the GEMM
// and the attention on the subgroup matrix units) and uses those instead.
//
// One arena of activations (AR), the weights (W), an index arena (I) and the
// dispatch's parameters (P, a dynamic offset into one uniform buffer). The
// constants DT, DH, CH and CW are prepended by gpu.rs.

@group(0) @binding(0) var<storage, read_write> AR: array<f32>;
@group(0) @binding(1) var<storage, read> W: array<f32>;
@group(0) @binding(2) var<storage, read> I: array<u32>;
struct Params { p: array<vec4<u32>, 8> }
@group(0) @binding(3) var<uniform> P: Params;

const NONE: u32 = 0xffffffffu;

fn pu(i: u32) -> u32 {
    return P.p[i / 4u][i % 4u];
}

fn pf(i: u32) -> f32 {
    return bitcast<f32>(pu(i));
}

// A workgroup's index in a 2-D grid (more than 65 535 workgroups).
fn lin(wg: vec3<u32>, nwg: vec3<u32>) -> u32 {
    return wg.x + wg.y * nwg.x;
}

// ------------------------------------------------------------------ gemm
// out[r][c] = act(s_r · Σ_k x[r][k] · W[k][c] + b[c]) (+ out[add_map[r]][c]) (+ out[r][c]),
// s_r = 1 or, with `rms`, rsqrt(mean_k x[r][k]² + 1e-6) (an RMSNorm before the
// layer, its gain folded into W). A workgroup computes a 32 × 64 tile; the
// next K-slice is loaded while the current one is multiplied. With a head
// norm (QK-norm) the 64 columns of the tile are one head: each row's head is
// RMSNormed after the bias (q: columns below `hn_split`, k: from it to twice it).
// p: 0 m, 1 k, 2 n, 3 x_off, 4 x_stride, 5 w_off, 6 b_off, 7 out_off,
//    8 out_stride, 9 in_map, 10 out_map, 11 add_map, 12 act (0, 1 relu, 2 relu²),
//    13 accumulate, 14 rms, 15 q head-norm gain (W or NONE), 16 k head-norm gain, 17 hn_split

var<workgroup> gA: array<f32, 512>;
var<workgroup> gB: array<f32, 1024>;
var<workgroup> gR: array<f32, 32>;
var<workgroup> gP: array<f32, 32>;
var<workgroup> gT: array<f32, 2048>;

@compute @workgroup_size(128)
fn gemm(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_index) t: u32) {
    let m = pu(0u);
    let k = pu(1u);
    let n = pu(2u);
    let w_off = pu(5u);
    let in_map = pu(9u);
    let rms = pu(14u);
    let row0 = wg.y * 32u;
    let col0 = wg.x * 64u;
    let tx = t % 16u;
    let ty = t / 16u;
    // This invocation's loads: 4 of the x slice (rows t % 32, k offsets t / 32 + 4i),
    // 8 of the W slice (column t % 64, k offsets t / 64 + 2i); row bases found once.
    let mm = t % 32u;
    let r = row0 + mm;
    let r_ok = r < m;
    var xr = r;
    if (r_ok && in_map != NONE) {
        xr = I[in_map + r];
    }
    let x_base = pu(3u) + select(0u, xr, r_ok) * pu(4u);
    let c_ld = col0 + t % 64u;
    let c_ok = c_ld < n;
    var ra: array<f32, 4>;
    var rb: array<f32, 8>;
    for (var i = 0u; i < 4u; i++) {
        let kk = t / 32u + 4u * i;
        let ok = r_ok && kk < k;
        ra[i] = select(0.0, AR[x_base + select(0u, kk, ok)], ok);
    }
    for (var i = 0u; i < 8u; i++) {
        let kk = t / 64u + 2u * i;
        let ok = c_ok && kk < k;
        rb[i] = select(0.0, W[select(0u, w_off + kk * n + c_ld, ok)], ok);
    }
    var acc: array<array<f32, 4>, 4>;
    for (var i = 0u; i < 4u; i++) {
        for (var j = 0u; j < 4u; j++) {
            acc[i][j] = 0.0;
        }
    }
    var ssq = 0.0;
    for (var kt = 0u; kt < k; kt += 16u) {
        for (var i = 0u; i < 4u; i++) {
            gA[(t / 32u + 4u * i) * 32u + mm] = ra[i];
        }
        for (var i = 0u; i < 8u; i++) {
            gB[(t / 64u + 2u * i) * 64u + t % 64u] = rb[i];
        }
        workgroupBarrier();
        let nk = kt + 16u;
        if (nk < k) {
            for (var i = 0u; i < 4u; i++) {
                let kk = nk + t / 32u + 4u * i;
                let ok = r_ok && kk < k;
                ra[i] = select(0.0, AR[x_base + select(0u, kk, ok)], ok);
            }
            for (var i = 0u; i < 8u; i++) {
                let kk = nk + t / 64u + 2u * i;
                let ok = c_ok && kk < k;
                rb[i] = select(0.0, W[select(0u, w_off + kk * n + c_ld, ok)], ok);
            }
        }
        if (rms != 0u && t < 32u) {
            for (var kk = 0u; kk < 16u; kk++) {
                let v = gA[kk * 32u + t];
                ssq = fma(v, v, ssq);
            }
        }
        for (var kk = 0u; kk < 16u; kk++) {
            var a: array<f32, 4>;
            var b: array<f32, 4>;
            for (var i = 0u; i < 4u; i++) {
                a[i] = gA[kk * 32u + ty + 8u * i];
                b[i] = gB[kk * 64u + tx + 16u * i];
            }
            for (var i = 0u; i < 4u; i++) {
                for (var j = 0u; j < 4u; j++) {
                    acc[i][j] = fma(a[i], b[j], acc[i][j]);
                }
            }
        }
        workgroupBarrier();
    }
    for (var i = 0u; i < 4u; i++) {
        for (var j = 0u; j < 4u; j++) {
            gT[(ty + 8u * i) * 64u + tx + 16u * j] = acc[i][j];
        }
    }
    gemm_epilogue(t, row0, col0, ssq);
}

// The GEMM's epilogue (128 invocations; the tile's sums in gT, row t's sum of
// squares in invocation t < 32) through shared memory, every step one code
// path per row or per element (so a row's result does not depend on which
// invocation, or which unrolled lane, holds it: the backend compiler may
// reassociate).
fn gemm_epilogue(t: u32, row0: u32, col0: u32, ssq: f32) {
    let m = pu(0u);
    let k = pu(1u);
    let n = pu(2u);
    let rms = pu(14u);
    if (t < 32u) {
        gR[t] = select(1.0, 1.0 / sqrt(ssq / f32(k) + 1e-6), rms != 0u);
    }
    workgroupBarrier();
    let b_off = pu(6u);
    let act = pu(12u);
    for (var e = t; e < 2048u; e += 128u) {
        let c = col0 + e % 64u;
        var v = gT[e] * gR[e / 64u];
        if (b_off != NONE && c < n) {
            v = v + W[b_off + c];
        }
        if (act == 1u) {
            v = max(v, 0.0);
        } else if (act == 2u) {
            let q = max(v, 0.0);
            v = q * q;
        }
        gT[e] = v;
    }
    // QK-norm of the tile's head.
    let split = pu(17u);
    var hn = NONE;
    if (pu(15u) != NONE && col0 < split) {
        hn = pu(15u);
    } else if (pu(16u) != NONE && col0 >= split && col0 < 2u * split) {
        hn = pu(16u);
    }
    if (hn != NONE) {
        workgroupBarrier();
        if (t < 32u) {
            var s = 0.0;
            for (var c = 0u; c < 64u; c++) {
                let v = gT[t * 64u + c];
                s = fma(v, v, s);
            }
            gP[t] = 1.0 / sqrt(s / 64.0 + 1e-6);
        }
        workgroupBarrier();
        for (var e = t; e < 2048u; e += 128u) {
            gT[e] = gT[e] * gP[e / 64u];
        }
        workgroupBarrier();
        for (var e = t; e < 2048u; e += 128u) {
            gT[e] = gT[e] * W[hn + e % 64u];
        }
    }
    workgroupBarrier();
    let out_off = pu(7u);
    let out_stride = pu(8u);
    let out_map = pu(10u);
    let add_map = pu(11u);
    let accumulate = pu(13u);
    for (var e = t; e < 2048u; e += 128u) {
        let r = row0 + e / 64u;
        let c = col0 + e % 64u;
        if (r >= m || c >= n) {
            continue;
        }
        var orow = r;
        if (out_map != NONE) {
            orow = I[out_map + r];
        }
        var v = gT[e];
        let at = out_off + orow * out_stride + c;
        if (add_map != NONE) {
            v = v + AR[out_off + I[add_map + r] * out_stride + c];
        }
        if (accumulate != 0u) {
            v = AR[at] + v;
        }
        AR[at] = v;
    }
}

// ------------------------------------------------------------------ hexconv
// The depthwise 7-tap convolution of a map block at its dilation, one value
// per invocation: y[t][c] = Σ_tap x[idx[t][tap]][c] · dw[tap][c] + b[c] (the
// edge row where the tap is off the map); RMSNorm, pw1 and pw2 are GEMMs.
// p: 0 tiles, 1 in_off, 2 out_off, 3 idx_off (I: [tiles × 7], NONE = the edge row),
//    4 dw, 5 dw_bias, 6 edge

@compute @workgroup_size(64)
fn hexconv(@builtin(workgroup_id) wg: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>, @builtin(local_invocation_index) t: u32) {
    let e = lin(wg, nwg) * 64u + t;
    if (e >= pu(0u) * DT) {
        return;
    }
    let tile = e / DT;
    let c = e % DT;
    var y = 0.0;
    for (var tap = 0u; tap < 7u; tap++) {
        let src = I[pu(3u) + tile * 7u + tap];
        let g = select(AR[pu(1u) + select(src, 0u, src == NONE) * DT + c], W[pu(6u) + c], src == NONE);
        y = fma(g, W[pu(4u) + tap * DT + c], y);
    }
    AR[pu(2u) + e] = y + W[pu(5u) + c];
}

// ------------------------------------------------------------------ norm
// RMSNorm (mode 0: x · rsqrt(mean x² + 1e-6) · w) or LayerNorm without affine
// parameters (mode 1, eps 1e-5) of `nseg` segments of `width` per row.
// p: 0 rows, 1 in_off, 2 in_stride, 3 width, 4 nseg, 5 w_off, 6 out_off,
//    7 out_stride, 8 mode, 9 in_map

var<workgroup> nRed: array<f32, 64>;

fn reduce_sum(t: u32, v: f32) -> f32 {
    nRed[t] = v;
    workgroupBarrier();
    for (var s = 32u; s > 0u; s = s / 2u) {
        if (t < s) {
            nRed[t] = nRed[t] + nRed[t + s];
        }
        workgroupBarrier();
    }
    let r = nRed[0];
    workgroupBarrier();
    return r;
}

fn reduce_max(t: u32, v: f32) -> f32 {
    nRed[t] = v;
    workgroupBarrier();
    for (var s = 32u; s > 0u; s = s / 2u) {
        if (t < s) {
            nRed[t] = max(nRed[t], nRed[t + s]);
        }
        workgroupBarrier();
    }
    let r = nRed[0];
    workgroupBarrier();
    return r;
}

@compute @workgroup_size(64)
fn norm(@builtin(workgroup_id) wg: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>, @builtin(local_invocation_index) t: u32) {
    let rows = pu(0u);
    let nseg = pu(4u);
    let g = lin(wg, nwg);
    if (g >= rows * nseg) {
        return;
    }
    let row = g / nseg;
    let seg = g % nseg;
    let width = pu(3u);
    var src = row;
    if (pu(9u) != NONE) {
        src = I[pu(9u) + row];
    }
    let base = pu(1u) + src * pu(2u) + seg * width;
    let obase = pu(6u) + row * pu(7u) + seg * width;
    let mode = pu(8u);
    var mean = 0.0;
    if (mode == 1u) {
        var s = 0.0;
        for (var c = t; c < width; c += 64u) {
            s = s + AR[base + c];
        }
        mean = reduce_sum(t, s) / f32(width);
    }
    var q = 0.0;
    for (var c = t; c < width; c += 64u) {
        let v = AR[base + c] - mean;
        q = fma(v, v, q);
    }
    let ms = reduce_sum(t, q) / f32(width);
    var r: f32;
    if (mode == 1u) {
        r = 1.0 / sqrt(ms + 1e-5);
    } else {
        r = 1.0 / sqrt(ms + 1e-6);
    }
    let w_off = pu(5u);
    for (var c = t; c < width; c += 64u) {
        var v = (AR[base + c] - mean) * r;
        if (w_off != NONE) {
            v = v * W[w_off + c];
        }
        AR[obase + c] = v;
    }
}

// ------------------------------------------------------------------ pool
// Per region: the mean and the max (with a zero) of its tiles' states.
// p: 0 regions, 1 tile_off, 2 start (I: [regions + 1] into the tile list), 3 tiles (I), 4 out_off

@compute @workgroup_size(DT)
fn pool(@builtin(workgroup_id) wg: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>, @builtin(local_invocation_index) c: u32) {
    let g = lin(wg, nwg);
    if (g >= pu(0u)) {
        return;
    }
    let s0 = I[pu(2u) + g];
    let s1 = I[pu(2u) + g + 1u];
    var sum = 0.0;
    var mx = 0.0;
    for (var i = s0; i < s1; i++) {
        let v = AR[pu(1u) + I[pu(3u) + i] * DT + c];
        sum = sum + v;
        mx = max(mx, v);
    }
    let o = pu(4u) + g * 2u * DT;
    AR[o + c] = sum / f32(max(s1 - s0, 1u));
    AR[o + DT + c] = mx;
}

// ------------------------------------------------------------------ gather
// dst[r][0 .. width] = src[idx[r]] (0 when NONE); dst[r][has_col] = 1 or 0.
// p: 0 rows, 1 src_off, 2 src_stride, 3 width, 4 idx (I), 5 dst_off, 6 dst_stride, 7 has_col

@compute @workgroup_size(64)
fn gather(@builtin(workgroup_id) wg: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>, @builtin(local_invocation_index) t: u32) {
    let r = lin(wg, nwg);
    if (r >= pu(0u)) {
        return;
    }
    let src = I[pu(4u) + r];
    let width = pu(3u);
    let dst = pu(5u) + r * pu(6u);
    for (var c = t; c < width; c += 64u) {
        var v = 0.0;
        if (src != NONE) {
            v = AR[pu(1u) + src * pu(2u) + c];
        }
        AR[dst + c] = v;
    }
    if (t == 0u && pu(7u) != NONE) {
        AR[dst + pu(7u)] = select(1.0, 0.0, src == NONE);
    }
}

// ------------------------------------------------------------------ attn
// softmax(q · kᵀ · scale + bias) · v for a block of QB query rows and one head
// a workgroup, the keys in tiles of KB with a running max and sum (the order
// of the tiles fixed); the bias by the hex distance bucket of the two
// tokens' positions (mode 1) or none (mode 0); q and k optionally RMSNormed
// per head on the way in (QK-norm).
// p: 0 blocks, 1 q_off, 2 q_stride, 3 k_off, 4 v_off, 5 kv_stride, 6 o_off, 7 o_stride,
//    8 blocks (I: per block first query row, query count, first key row, key count),
//    9 bias mode, 10 pos_off (AR: [rows × 4], queries and keys by their row), 11 bias_w (W: [heads × 14]),
//    12 scale (f32), 13 heads, 14 qn (W or NONE), 15 kn (W or NONE)

const QB: u32 = 16u;
const KB: u32 = 16u;
// Rows padded to DH + 1 floats: the score loop reads a column of 16 rows (no bank conflicts).
const PS: u32 = DH + 1u;
var<workgroup> fQ: array<f32, QB * PS>;
var<workgroup> fK: array<f32, KB * PS>;
var<workgroup> fV: array<vec4<f32>, KB * DH / 4u>;
var<workgroup> fS: array<f32, QB * KB>;
var<workgroup> fM: array<f32, QB>;
var<workgroup> fL: array<f32, QB>;
var<workgroup> fC: array<f32, QB>;
var<workgroup> fR: array<f32, QB + KB>;
var<workgroup> fB: array<u32, 4>;

fn bucket(d: f32) -> u32 {
    let k = u32(clamp(d, 0.0, 63.0));
    if (k < 8u) {
        return k;
    }
    if (k < 12u) {
        return 8u;
    }
    if (k < 16u) {
        return 9u;
    }
    if (k < 24u) {
        return 10u;
    }
    if (k < 32u) {
        return 11u;
    }
    return 12u;
}


var<workgroup> fPq: array<f32, QB * 3u>;
var<workgroup> fPk: array<f32, KB * 3u>;

// The bucket of two positions (q, r, has a position).
fn pos_bucket(qa: f32, ra: f32, oa: f32, qb: f32, rb: f32, ob: f32) -> u32 {
    if (oa == 0.0 || ob == 0.0) {
        return 13u;
    }
    let dq = qa - qb;
    let dr = ra - rb;
    return bucket(round((abs(dq) + abs(dr) + abs(dq + dr)) * 0.5));
}

@compute @workgroup_size(256)
fn attn(@builtin(workgroup_id) wg: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>, @builtin(local_invocation_index) t: u32) {
    let heads = pu(13u);
    let g = lin(wg, nwg);
    let blk = g / heads;
    let h = g % heads;
    if (blk >= pu(0u)) {
        return;
    }
    if (t < 4u) {
        fB[t] = I[pu(8u) + 4u * blk + t];
    }
    let qs = workgroupUniformLoad(&fB[0]);
    let qc = workgroupUniformLoad(&fB[1]);
    let ks = workgroupUniformLoad(&fB[2]);
    let kc = workgroupUniformLoad(&fB[3]);
    let qn = pu(14u);
    let kn = pu(15u);
    let kv_stride = pu(5u);
    let mode = pu(9u);
    let pos = pu(10u);
    for (var e = t; e < QB * DH; e += 256u) {
        let r = e / DH;
        let ok = r < qc;
        fQ[r * PS + e % DH] = select(0.0, AR[pu(1u) + select(qs, qs + r, ok) * pu(2u) + h * DH + e % DH], ok);
    }
    if (mode == 1u && t < QB * 3u) {
        let r = t / 3u;
        fPq[t] = select(0.0, AR[pos + select(qs, qs + r, r < qc) * 4u + t % 3u], r < qc);
    }
    workgroupBarrier();
    if (qn != NONE) {
        if (t < QB) {
            var s = 0.0;
            for (var c = 0u; c < DH; c++) {
                let v = fQ[t * PS + c];
                s = fma(v, v, s);
            }
            fR[t] = 1.0 / sqrt(s / f32(DH) + 1e-6);
        }
        workgroupBarrier();
        for (var e = t; e < QB * DH; e += 256u) {
            let at = (e / DH) * PS + e % DH;
            fQ[at] = fQ[at] * fR[e / DH] * W[qn + e % DH];
        }
    }
    if (t < QB) {
        fM[t] = -3.0e38;
        fL[t] = 0.0;
    }
    workgroupBarrier();
    let r = t / 16u;
    let d0 = (t % 16u) * 4u;
    var o = vec4<f32>(0.0);
    let scale = pf(12u);
    // This invocation's K and V loads of a tile (rows t / 64 + 4i, column t % 64), the next tile's ahead.
    var pk: array<f32, 4>;
    var pv = vec4<f32>(0.0);
    var pp = 0.0;
    for (var i = 0u; i < 4u; i++) {
        let j = t / DH + 4u * i;
        let ok = j < kc;
        let row = select(ks, ks + j, ok);
        pk[i] = select(0.0, AR[pu(3u) + row * kv_stride + h * DH + t % DH], ok);
    }
    {
        // The values: one vec4 an invocation (row t / 16, columns 4 (t % 16) …).
        let j = t / 16u;
        let ok = j < kc;
        let at = pu(4u) + select(ks, ks + j, ok) * kv_stride + h * DH + (t % 16u) * 4u;
        pv = select(vec4<f32>(0.0), vec4<f32>(AR[at], AR[at + 1u], AR[at + 2u], AR[at + 3u]), ok);
    }
    if (mode == 1u && t < KB * 3u) {
        let j = t / 3u;
        pp = select(0.0, AR[pos + select(ks, ks + j, j < kc) * 4u + t % 3u], j < kc);
    }
    for (var kt = 0u; kt < kc; kt += KB) {
        for (var i = 0u; i < 4u; i++) {
            fK[(t / DH + 4u * i) * PS + t % DH] = pk[i];
        }
        fV[t] = pv;
        if (t < KB * 3u) {
            fPk[t] = pp;
        }
        workgroupBarrier();
        let nk = kt + KB;
        if (nk < kc) {
            for (var i = 0u; i < 4u; i++) {
                let j = nk + t / DH + 4u * i;
                let ok = j < kc;
                let row = select(ks, ks + j, ok);
                pk[i] = select(0.0, AR[pu(3u) + row * kv_stride + h * DH + t % DH], ok);
            }
            {
                let j = nk + t / 16u;
                let ok = j < kc;
                let at = pu(4u) + select(ks, ks + j, ok) * kv_stride + h * DH + (t % 16u) * 4u;
                pv = select(vec4<f32>(0.0), vec4<f32>(AR[at], AR[at + 1u], AR[at + 2u], AR[at + 3u]), ok);
            }
            if (mode == 1u && t < KB * 3u) {
                let j = nk + t / 3u;
                pp = select(0.0, AR[pos + select(ks, ks + j, j < kc) * 4u + t % 3u], j < kc);
            }
        }
        if (kn != NONE) {
            if (t < KB) {
                var s = 0.0;
                for (var c = 0u; c < DH; c++) {
                    let v = fK[t * PS + c];
                    s = fma(v, v, s);
                }
                fR[QB + t] = 1.0 / sqrt(s / f32(DH) + 1e-6);
            }
            workgroupBarrier();
            for (var e = t; e < KB * DH; e += 256u) {
                let at = (e / DH) * PS + e % DH;
                fK[at] = fK[at] * fR[QB + e / DH] * W[kn + e % DH];
            }
            workgroupBarrier();
        }
        // The scores: one per invocation (QB × KB = 256).
        {
            let qr = t / KB;
            let j = t % KB;
            var s = 0.0;
            for (var c = 0u; c < DH; c++) {
                s = fma(fQ[qr * PS + c], fK[j * PS + c], s);
            }
            s = s * scale;
            if (mode == 1u) {
                let b = pos_bucket(fPq[qr * 3u], fPq[qr * 3u + 1u], fPq[qr * 3u + 2u], fPk[j * 3u], fPk[j * 3u + 1u], fPk[j * 3u + 2u]);
                s = s + W[pu(11u) + h * 14u + b];
            }
            fS[t] = s;
        }
        workgroupBarrier();
        // Per query row: the tile's max, the running max and sum.
        if (t < QB) {
            let n = min(KB, kc - kt);
            var mt = fM[t];
            for (var j = 0u; j < n; j++) {
                mt = max(mt, fS[t * KB + j]);
            }
            let corr = exp(fM[t] - mt);
            var l = fL[t] * corr;
            for (var j = 0u; j < KB; j++) {
                var p = 0.0;
                if (j < n) {
                    p = exp(fS[t * KB + j] - mt);
                }
                fS[t * KB + j] = p;
                l = l + p;
            }
            fM[t] = mt;
            fL[t] = l;
            fC[t] = corr;
        }
        workgroupBarrier();
        o = o * fC[r];
        for (var j = 0u; j < KB; j++) {
            let p = fS[r * KB + j];
            let v = fV[j * (DH / 4u) + d0 / 4u];
            o = fma(vec4<f32>(p), v, o);
        }
        workgroupBarrier();
    }
    if (r < qc) {
        let l = fL[r];
        var res = vec4<f32>(0.0);
        if (l > 0.0) {
            res = o / l;
        }
        let at = pu(6u) + (qs + r) * pu(7u) + h * DH + d0;
        AR[at] = res.x;
        AR[at + 1u] = res.y;
        AR[at + 2u] = res.z;
        AR[at + 3u] = res.w;
    }
}

// ------------------------------------------------------------------ gru
// h' = (1 − z) · n + z · h from the input's and the state's gate rows.
// p: 0 rows, 1 width H, 2 gi_off, 3 gh_off, 4 h_off, 5 out_off

fn sigm(x: f32) -> f32 {
    return 1.0 / (1.0 + exp(-x));
}

@compute @workgroup_size(64)
fn gru(@builtin(workgroup_id) wg: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>, @builtin(local_invocation_index) t: u32) {
    let hw = pu(1u);
    let e = lin(wg, nwg) * 64u + t;
    if (e >= pu(0u) * hw) {
        return;
    }
    let r_ = e / hw;
    let j = e % hw;
    let gi = pu(2u) + r_ * 3u * hw;
    let gh = pu(3u) + r_ * 3u * hw;
    let r = sigm(AR[gi + j] + AR[gh + j]);
    let z = sigm(AR[gi + hw + j] + AR[gh + hw + j]);
    let n = tanh(AR[gi + 2u * hw + j] + r * AR[gh + 2u * hw + j]);
    AR[pu(5u) + e] = (1.0 - z) * n + z * AR[pu(4u) + e];
}

// ------------------------------------------------------------------ combat
// The combat module's blow of an attacker (a workgroup) on each entity of its
// turn, four defenders at a time: relu(att[i] + dfd[j]) → l2 (an output per
// invocation) → relu → head[0] (a fixed reduction tree), clamped to 0 … 1.
// p: 0 entities, 1 att_off, 2 dfd_off, 3 table (I: per entity: first entity of
//    its turn, the turn's count, its dealt base, flags), 4 l2w, 5 l2b, 6 hw, 7 hb

const JB: u32 = 4u;
var<workgroup> cH: array<f32, JB * CH>;
var<workgroup> cP: array<f32, JB * CW>;
var<workgroup> cT: array<u32, 4>;

@compute @workgroup_size(CW)
fn combat(@builtin(workgroup_id) wg: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>, @builtin(local_invocation_index) t: u32) {
    let i = lin(wg, nwg);
    if (i >= pu(0u)) {
        return;
    }
    if (t < 4u) {
        cT[t] = I[pu(3u) + 4u * i + t];
    }
    let start = workgroupUniformLoad(&cT[0]);
    let count = workgroupUniformLoad(&cT[1]);
    let flag = workgroupUniformLoad(&cT[3]);
    if (flag == 0u) {
        return;
    }
    let base = cT[2] + (i - start) * count;
    for (var j0 = 0u; j0 < count; j0 += JB) {
        for (var e = t; e < JB * CH; e += CW) {
            let j = j0 + e / CH;
            let k = e % CH;
            var v = 0.0;
            if (j < count) {
                v = max(AR[pu(1u) + i * CH + k] + AR[pu(2u) + (start + j) * CH + k], 0.0);
            }
            cH[e] = v;
        }
        workgroupBarrier();
        var a: array<f32, JB>;
        let b = W[pu(5u) + t];
        for (var jj = 0u; jj < JB; jj++) {
            a[jj] = b;
        }
        for (var k = 0u; k < CH; k++) {
            let w = W[pu(4u) + k * CW + t];
            for (var jj = 0u; jj < JB; jj++) {
                a[jj] = fma(cH[jj * CH + k], w, a[jj]);
            }
        }
        let hw = W[pu(6u) + t * 4u];
        for (var jj = 0u; jj < JB; jj++) {
            cP[jj * CW + t] = max(a[jj], 0.0) * hw;
        }
        workgroupBarrier();
        for (var s = CW / 2u; s > 0u; s = s / 2u) {
            if (t < s) {
                for (var jj = 0u; jj < JB; jj++) {
                    cP[jj * CW + t] = cP[jj * CW + t] + cP[jj * CW + t + s];
                }
            }
            workgroupBarrier();
        }
        if (t < JB && j0 + t < count) {
            AR[base + j0 + t] = clamp(cP[t * CW] + W[pu(7u)], 0.0, 1.0);
        }
        workgroupBarrier();
    }
}
