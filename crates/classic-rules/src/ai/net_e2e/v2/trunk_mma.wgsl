// ------------------------------------------------------------------ gemm_mma
// `gemm` with the products on the subgroup matrix units (`wgpu_cooperative_matrix`,
// 8 × 8 f32; gpu.rs appends this kernel only where the adapter has it): the
// same tile, loads, RMSNorm and epilogue; each of the four subgroups multiplies
// a 16 × 32 quarter of the tile, a K-slice of 16 at a time.

@compute @workgroup_size(128)
fn gemm_mma(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_index) t: u32) {
    let m = pu(0u);
    let k = pu(1u);
    let n = pu(2u);
    let w_off = pu(5u);
    let in_map = pu(9u);
    let rms = pu(14u);
    let row0 = wg.y * 32u;
    let col0 = wg.x * 64u;
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
    // This subgroup's quarter: rows 16 (s / 2), columns 32 (s % 2).
    let s = t / 32u;
    let qr = (s / 2u) * 16u;
    let qc = (s % 2u) * 32u;
    var c00 = coop_mat8x8<f32, C>();
    var c01 = coop_mat8x8<f32, C>();
    var c02 = coop_mat8x8<f32, C>();
    var c03 = coop_mat8x8<f32, C>();
    var c10 = coop_mat8x8<f32, C>();
    var c11 = coop_mat8x8<f32, C>();
    var c12 = coop_mat8x8<f32, C>();
    var c13 = coop_mat8x8<f32, C>();
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
        for (var kk = 0u; kk < 16u; kk += 8u) {
            // gA is [k][row]: an A block is column-major with stride 32; gB is [k][column].
            let a0 = coopLoad<coop_mat8x8<f32, A>>(&gA[kk * 32u + qr], 32u);
            let a1 = coopLoad<coop_mat8x8<f32, A>>(&gA[kk * 32u + qr + 8u], 32u);
            let b0 = coopLoadT<coop_mat8x8<f32, B>>(&gB[kk * 64u + qc], 64u);
            let b1 = coopLoadT<coop_mat8x8<f32, B>>(&gB[kk * 64u + qc + 8u], 64u);
            let b2 = coopLoadT<coop_mat8x8<f32, B>>(&gB[kk * 64u + qc + 16u], 64u);
            let b3 = coopLoadT<coop_mat8x8<f32, B>>(&gB[kk * 64u + qc + 24u], 64u);
            c00 = coopMultiplyAdd(a0, b0, c00);
            c01 = coopMultiplyAdd(a0, b1, c01);
            c02 = coopMultiplyAdd(a0, b2, c02);
            c03 = coopMultiplyAdd(a0, b3, c03);
            c10 = coopMultiplyAdd(a1, b0, c10);
            c11 = coopMultiplyAdd(a1, b1, c11);
            c12 = coopMultiplyAdd(a1, b2, c12);
            c13 = coopMultiplyAdd(a1, b3, c13);
        }
        workgroupBarrier();
    }
    coopStoreT(c00, &gT[qr * 64u + qc], 64u);
    coopStoreT(c01, &gT[qr * 64u + qc + 8u], 64u);
    coopStoreT(c02, &gT[qr * 64u + qc + 16u], 64u);
    coopStoreT(c03, &gT[qr * 64u + qc + 24u], 64u);
    coopStoreT(c10, &gT[(qr + 8u) * 64u + qc], 64u);
    coopStoreT(c11, &gT[(qr + 8u) * 64u + qc + 8u], 64u);
    coopStoreT(c12, &gT[(qr + 8u) * 64u + qc + 16u], 64u);
    coopStoreT(c13, &gT[(qr + 8u) * 64u + qc + 24u], 64u);
    gemm_epilogue(t, row0, col0, ssq);
}

// ------------------------------------------------------------------ gemm_mmad
// `gemm_mma` reading x and W straight from the buffers (no shared staging, no
// barrier per K-slice), for a GEMM without a row map and with K a multiple of
// 8 (gpu.rs picks it): a tile's rows past m and columns past n read whatever
// follows (both buffers are padded), and only those rows' and columns'
// results, never written, depend on it. Each subgroup multiplies a 16 × 32
// quarter of the 32 × 64 tile over all of K; the RMSNorm's sums by
// invocation t < 32 over row t.

@compute @workgroup_size(128)
fn gemm_mmad(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_index) t: u32) {
    let m = pu(0u);
    let k = pu(1u);
    let n = pu(2u);
    let x_off = pu(3u);
    let x_stride = pu(4u);
    let w_off = pu(5u);
    let rms = pu(14u);
    let row0 = wg.y * 32u;
    let col0 = wg.x * 64u;
    let s = t / 32u;
    let qr = (s / 2u) * 16u;
    let qc = (s % 2u) * 32u;
    let xa = x_off + (row0 + qr) * x_stride;
    let xb = xa + 8u * x_stride;
    let wb = w_off + col0 + qc;
    var c00 = coop_mat8x8<f32, C>();
    var c01 = coop_mat8x8<f32, C>();
    var c02 = coop_mat8x8<f32, C>();
    var c03 = coop_mat8x8<f32, C>();
    var c10 = coop_mat8x8<f32, C>();
    var c11 = coop_mat8x8<f32, C>();
    var c12 = coop_mat8x8<f32, C>();
    var c13 = coop_mat8x8<f32, C>();
    for (var kk = 0u; kk < k; kk += 8u) {
        let a0 = coopLoadT<coop_mat8x8<f32, A>>(&AR[xa + kk], x_stride);
        let a1 = coopLoadT<coop_mat8x8<f32, A>>(&AR[xb + kk], x_stride);
        let b0 = coopLoadT<coop_mat8x8<f32, B>>(&W[wb + kk * n + 0u], n);
        let b1 = coopLoadT<coop_mat8x8<f32, B>>(&W[wb + kk * n + 8u], n);
        let b2 = coopLoadT<coop_mat8x8<f32, B>>(&W[wb + kk * n + 16u], n);
        let b3 = coopLoadT<coop_mat8x8<f32, B>>(&W[wb + kk * n + 24u], n);
        c00 = coopMultiplyAdd(a0, b0, c00);
        c01 = coopMultiplyAdd(a0, b1, c01);
        c02 = coopMultiplyAdd(a0, b2, c02);
        c03 = coopMultiplyAdd(a0, b3, c03);
        c10 = coopMultiplyAdd(a1, b0, c10);
        c11 = coopMultiplyAdd(a1, b1, c11);
        c12 = coopMultiplyAdd(a1, b2, c12);
        c13 = coopMultiplyAdd(a1, b3, c13);
    }
    var ssq = 0.0;
    if (rms != 0u && t < 32u && row0 + t < m) {
        let base = x_off + (row0 + t) * x_stride;
        for (var kk = 0u; kk < k; kk++) {
            let v = AR[base + kk];
            ssq = fma(v, v, ssq);
        }
    }
    coopStoreT(c00, &gT[(qr + 0u) * 64u + qc + 0u], 64u);
    coopStoreT(c01, &gT[(qr + 0u) * 64u + qc + 8u], 64u);
    coopStoreT(c02, &gT[(qr + 0u) * 64u + qc + 16u], 64u);
    coopStoreT(c03, &gT[(qr + 0u) * 64u + qc + 24u], 64u);
    coopStoreT(c10, &gT[(qr + 8u) * 64u + qc + 0u], 64u);
    coopStoreT(c11, &gT[(qr + 8u) * 64u + qc + 8u], 64u);
    coopStoreT(c12, &gT[(qr + 8u) * 64u + qc + 16u], 64u);
    coopStoreT(c13, &gT[(qr + 8u) * 64u + qc + 24u], 64u);
    gemm_epilogue(t, row0, col0, ssq);
}

// ------------------------------------------------------------------ attn_mma
// `attn` (no QK-norm on the way in) on the subgroup matrix units, the same
// floats: a workgroup is one subgroup of 32 and takes 8 query rows (half a
// block of 16) and one head; the keys go by tiles of 16 as in `attn` (the
// running max, the correction, the sum in the same order), the products
// Q · Kᵀ and P · V as 8 × 8 matrix multiply-adds, which sum in the order of
// `attn`'s fused loops (the same bits; v2_check's logits hash). Key and value
// rows past the turn's keys are read (the arena is padded) and weigh 0. No
// branch around a matrix operation: naga wants them in uniform control flow
// and takes a loop counter as varying.
// p: as `attn` (14 and 15 NONE); the grid is blocks × 2 × heads.

var<workgroup> mQ: array<f32, 512>;
var<workgroup> mS: array<f32, 128>;
var<workgroup> mO: array<f32, 512>;
var<workgroup> mRow: array<f32, 72>;
var<workgroup> mB: array<u32, 4>;
var<workgroup> mG: u32;
var<workgroup> mFlag: u32;

@compute @workgroup_size(32)
fn attn_mma(@builtin(workgroup_id) wg: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>, @builtin(local_invocation_index) t: u32) {
    let heads = pu(13u);
    if (t == 0u) {
        mG = lin(wg, nwg);
    }
    let g = workgroupUniformLoad(&mG);
    let h = g % heads;
    let half = (g / heads) % 2u;
    let blk = g / (2u * heads);
    // No early return: a workgroup past the blocks, or past its block's rows, has no rows and no keys.
    let live = blk < pu(0u);
    if (t < 4u) {
        mB[t] = I[pu(8u) + 4u * select(0u, blk, live) + t];
    }
    let qs0 = workgroupUniformLoad(&mB[0]);
    let qc0 = select(0u, workgroupUniformLoad(&mB[1]), live);
    let ks = workgroupUniformLoad(&mB[2]);
    let has = qc0 > 8u * half;
    let kc = select(0u, workgroupUniformLoad(&mB[3]), has);
    let qs = qs0 + 8u * half;
    let qc = select(0u, min(8u, qc0 - 8u * half), has);
    let kv_stride = pu(5u);
    let k_off = pu(3u) + h * DH;
    let v_off = pu(4u) + h * DH;
    let mode = pu(9u);
    let pos = pu(10u);
    let scale = pf(12u);
    for (var e = t; e < 512u; e += 32u) {
        let r = e / 64u;
        let ok = r < qc;
        mQ[e] = select(0.0, AR[pu(1u) + select(qs, qs + r, ok) * pu(2u) + h * DH + e % 64u], ok);
    }
    // Per row: the running max (0 … 7), sum (8 … 15), the tile's correction (16 … 23);
    // the row's query position (24 + 3 r); the head's distance biases (48 …); the tile's max (64 …).
    if (t < 8u) {
        mRow[t] = -3.0e38;
        mRow[8u + t] = 0.0;
    }
    if (mode == 1u && t < 24u) {
        let r = t / 3u;
        mRow[24u + t] = select(0.0, AR[pos + select(qs, qs + r, r < qc) * 4u + t % 3u], r < qc);
    }
    if (mode == 1u && t < 14u) {
        mRow[48u + t] = W[pu(11u) + h * 14u + t];
    }
    workgroupBarrier();
    let q0 = coopLoadT<coop_mat8x8<f32, A>>(&mQ[0u], 64u);
    let q1 = coopLoadT<coop_mat8x8<f32, A>>(&mQ[8u], 64u);
    let q2 = coopLoadT<coop_mat8x8<f32, A>>(&mQ[16u], 64u);
    let q3 = coopLoadT<coop_mat8x8<f32, A>>(&mQ[24u], 64u);
    let q4 = coopLoadT<coop_mat8x8<f32, A>>(&mQ[32u], 64u);
    let q5 = coopLoadT<coop_mat8x8<f32, A>>(&mQ[40u], 64u);
    let q6 = coopLoadT<coop_mat8x8<f32, A>>(&mQ[48u], 64u);
    let q7 = coopLoadT<coop_mat8x8<f32, A>>(&mQ[56u], 64u);
    var o0 = coop_mat8x8<f32, C>();
    var o1 = coop_mat8x8<f32, C>();
    var o2 = coop_mat8x8<f32, C>();
    var o3 = coop_mat8x8<f32, C>();
    var o4 = coop_mat8x8<f32, C>();
    var o5 = coop_mat8x8<f32, C>();
    var o6 = coop_mat8x8<f32, C>();
    var o7 = coop_mat8x8<f32, C>();
    for (var kt = 0u; kt < kc; kt += 16u) {
        // The scores of the tile's two blocks of 8 keys: mS [8 rows × 16].
        let kb = k_off + (ks + kt) * kv_stride;
        var s0 = coop_mat8x8<f32, C>();
        s0 = coopMultiplyAdd(q0, coopLoad<coop_mat8x8<f32, B>>(&AR[kb + 0u], kv_stride), s0);
        s0 = coopMultiplyAdd(q1, coopLoad<coop_mat8x8<f32, B>>(&AR[kb + 8u], kv_stride), s0);
        s0 = coopMultiplyAdd(q2, coopLoad<coop_mat8x8<f32, B>>(&AR[kb + 16u], kv_stride), s0);
        s0 = coopMultiplyAdd(q3, coopLoad<coop_mat8x8<f32, B>>(&AR[kb + 24u], kv_stride), s0);
        s0 = coopMultiplyAdd(q4, coopLoad<coop_mat8x8<f32, B>>(&AR[kb + 32u], kv_stride), s0);
        s0 = coopMultiplyAdd(q5, coopLoad<coop_mat8x8<f32, B>>(&AR[kb + 40u], kv_stride), s0);
        s0 = coopMultiplyAdd(q6, coopLoad<coop_mat8x8<f32, B>>(&AR[kb + 48u], kv_stride), s0);
        s0 = coopMultiplyAdd(q7, coopLoad<coop_mat8x8<f32, B>>(&AR[kb + 56u], kv_stride), s0);
        var s1 = coop_mat8x8<f32, C>();
        s1 = coopMultiplyAdd(q0, coopLoad<coop_mat8x8<f32, B>>(&AR[kb + 8u * kv_stride + 0u], kv_stride), s1);
        s1 = coopMultiplyAdd(q1, coopLoad<coop_mat8x8<f32, B>>(&AR[kb + 8u * kv_stride + 8u], kv_stride), s1);
        s1 = coopMultiplyAdd(q2, coopLoad<coop_mat8x8<f32, B>>(&AR[kb + 8u * kv_stride + 16u], kv_stride), s1);
        s1 = coopMultiplyAdd(q3, coopLoad<coop_mat8x8<f32, B>>(&AR[kb + 8u * kv_stride + 24u], kv_stride), s1);
        s1 = coopMultiplyAdd(q4, coopLoad<coop_mat8x8<f32, B>>(&AR[kb + 8u * kv_stride + 32u], kv_stride), s1);
        s1 = coopMultiplyAdd(q5, coopLoad<coop_mat8x8<f32, B>>(&AR[kb + 8u * kv_stride + 40u], kv_stride), s1);
        s1 = coopMultiplyAdd(q6, coopLoad<coop_mat8x8<f32, B>>(&AR[kb + 8u * kv_stride + 48u], kv_stride), s1);
        s1 = coopMultiplyAdd(q7, coopLoad<coop_mat8x8<f32, B>>(&AR[kb + 8u * kv_stride + 56u], kv_stride), s1);
        coopStoreT(s0, &mS[0u], 16u);
        coopStoreT(s1, &mS[8u], 16u);
        workgroupBarrier();
        // Per row (as `attn`): the scale and bias (4 invocations a row, 4 keys each), the
        // tile's max, the correction, the weights (4 a row) and their sum in key order (1 a row).
        let n = min(16u, kc - kt);
        let sr = t / 4u;
        let sp = t % 4u;
        for (var jj = 0u; jj < 4u; jj++) {
            let j = 4u * sp + jj;
            if (j < n) {
                var v = mS[sr * 16u + j] * scale;
                if (mode == 1u) {
                    let kr = ks + kt + j;
                    let bk = pos_bucket(mRow[24u + 3u * sr], mRow[25u + 3u * sr], mRow[26u + 3u * sr], AR[pos + kr * 4u], AR[pos + kr * 4u + 1u], AR[pos + kr * 4u + 2u]);
                    v = v + mRow[48u + bk];
                }
                mS[sr * 16u + j] = v;
            }
        }
        workgroupBarrier();
        if (t < 8u) {
            var mt = mRow[t];
            for (var j = 0u; j < n; j++) {
                mt = max(mt, mS[t * 16u + j]);
            }
            mRow[64u + t] = mt;
        }
        workgroupBarrier();
        let mt = mRow[64u + sr];
        for (var jj = 0u; jj < 4u; jj++) {
            let j = 4u * sp + jj;
            var p = 0.0;
            if (j < n) {
                p = exp(mS[sr * 16u + j] - mt);
            }
            mS[sr * 16u + j] = p;
        }
        workgroupBarrier();
        if (t < 8u) {
            let corr = exp(mRow[t] - mRow[64u + t]);
            var l = mRow[8u + t] * corr;
            for (var j = 0u; j < 16u; j++) {
                l = l + mS[t * 16u + j];
            }
            mRow[t] = mRow[64u + t];
            mRow[8u + t] = l;
            mRow[16u + t] = corr;
        }
        workgroupBarrier();
        // The output's rows times the correction (skipped when it is 1 for every row:
        // o · 1 = o; and on the first tile, where o is 0 and so is o · corr).
        if (t == 0u) {
            var f = 0u;
            for (var r = 0u; r < 8u; r++) {
                if (mRow[16u + r] != 1.0) {
                    f = 1u;
                }
            }
            mFlag = select(0u, f, kt > 0u);
        }
        if (workgroupUniformLoad(&mFlag) != 0u) {
            coopStoreT(o0, &mO[0u], 64u);
            coopStoreT(o1, &mO[8u], 64u);
            coopStoreT(o2, &mO[16u], 64u);
            coopStoreT(o3, &mO[24u], 64u);
            coopStoreT(o4, &mO[32u], 64u);
            coopStoreT(o5, &mO[40u], 64u);
            coopStoreT(o6, &mO[48u], 64u);
            coopStoreT(o7, &mO[56u], 64u);
            workgroupBarrier();
            for (var e = t; e < 512u; e += 32u) {
                mO[e] = mO[e] * mRow[16u + e / 64u];
            }
            workgroupBarrier();
            o0 = coopLoadT<coop_mat8x8<f32, C>>(&mO[0u], 64u);
            o1 = coopLoadT<coop_mat8x8<f32, C>>(&mO[8u], 64u);
            o2 = coopLoadT<coop_mat8x8<f32, C>>(&mO[16u], 64u);
            o3 = coopLoadT<coop_mat8x8<f32, C>>(&mO[24u], 64u);
            o4 = coopLoadT<coop_mat8x8<f32, C>>(&mO[32u], 64u);
            o5 = coopLoadT<coop_mat8x8<f32, C>>(&mO[40u], 64u);
            o6 = coopLoadT<coop_mat8x8<f32, C>>(&mO[48u], 64u);
            o7 = coopLoadT<coop_mat8x8<f32, C>>(&mO[56u], 64u);
        }
        let p0 = coopLoadT<coop_mat8x8<f32, A>>(&mS[0u], 16u);
        let p1 = coopLoadT<coop_mat8x8<f32, A>>(&mS[8u], 16u);
        let vb = v_off + (ks + kt) * kv_stride;
        o0 = coopMultiplyAdd(p0, coopLoadT<coop_mat8x8<f32, B>>(&AR[vb + 0u], kv_stride), o0);
        o1 = coopMultiplyAdd(p0, coopLoadT<coop_mat8x8<f32, B>>(&AR[vb + 8u], kv_stride), o1);
        o2 = coopMultiplyAdd(p0, coopLoadT<coop_mat8x8<f32, B>>(&AR[vb + 16u], kv_stride), o2);
        o3 = coopMultiplyAdd(p0, coopLoadT<coop_mat8x8<f32, B>>(&AR[vb + 24u], kv_stride), o3);
        o4 = coopMultiplyAdd(p0, coopLoadT<coop_mat8x8<f32, B>>(&AR[vb + 32u], kv_stride), o4);
        o5 = coopMultiplyAdd(p0, coopLoadT<coop_mat8x8<f32, B>>(&AR[vb + 40u], kv_stride), o5);
        o6 = coopMultiplyAdd(p0, coopLoadT<coop_mat8x8<f32, B>>(&AR[vb + 48u], kv_stride), o6);
        o7 = coopMultiplyAdd(p0, coopLoadT<coop_mat8x8<f32, B>>(&AR[vb + 56u], kv_stride), o7);
        o0 = coopMultiplyAdd(p1, coopLoadT<coop_mat8x8<f32, B>>(&AR[vb + 8u * kv_stride + 0u], kv_stride), o0);
        o1 = coopMultiplyAdd(p1, coopLoadT<coop_mat8x8<f32, B>>(&AR[vb + 8u * kv_stride + 8u], kv_stride), o1);
        o2 = coopMultiplyAdd(p1, coopLoadT<coop_mat8x8<f32, B>>(&AR[vb + 8u * kv_stride + 16u], kv_stride), o2);
        o3 = coopMultiplyAdd(p1, coopLoadT<coop_mat8x8<f32, B>>(&AR[vb + 8u * kv_stride + 24u], kv_stride), o3);
        o4 = coopMultiplyAdd(p1, coopLoadT<coop_mat8x8<f32, B>>(&AR[vb + 8u * kv_stride + 32u], kv_stride), o4);
        o5 = coopMultiplyAdd(p1, coopLoadT<coop_mat8x8<f32, B>>(&AR[vb + 8u * kv_stride + 40u], kv_stride), o5);
        o6 = coopMultiplyAdd(p1, coopLoadT<coop_mat8x8<f32, B>>(&AR[vb + 8u * kv_stride + 48u], kv_stride), o6);
        o7 = coopMultiplyAdd(p1, coopLoadT<coop_mat8x8<f32, B>>(&AR[vb + 8u * kv_stride + 56u], kv_stride), o7);
        workgroupBarrier();
    }
        coopStoreT(o0, &mO[0u], 64u);
        coopStoreT(o1, &mO[8u], 64u);
        coopStoreT(o2, &mO[16u], 64u);
        coopStoreT(o3, &mO[24u], 64u);
        coopStoreT(o4, &mO[32u], 64u);
        coopStoreT(o5, &mO[40u], 64u);
        coopStoreT(o6, &mO[48u], 64u);
        coopStoreT(o7, &mO[56u], 64u);
    workgroupBarrier();
    for (var e = t; e < 512u; e += 32u) {
        let r = e / 64u;
        if (r < qc) {
            let l = mRow[8u + r];
            var v = 0.0;
            if (l > 0.0) {
                v = mO[e] / l;
            }
            AR[pu(6u) + (qs + r) * pu(7u) + h * DH + e % 64u] = v;
        }
    }
}
