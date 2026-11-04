//! The v2 trunk on the GPU (the `gpu` feature): wgpu compute with the WGSL
//! kernels of `trunk.wgsl`, so the same code runs on Metal, Vulkan, DX12 and
//! WebGPU (wasm32). It computes what [`trunk::run_cpu`] computes, for a batch
//! of turns at once (the turns of every game waiting at that moment): their
//! rows are concatenated, so a dispatch serves the whole batch, and the whole
//! trunk is one command buffer with one submit and one wait.
//!
//! - Memory: the weights in one storage buffer, uploaded once; the batch's
//!   activations in one arena (inputs, then the outputs read back, then the
//!   scratch), its index tables in another, each dispatch's parameters at a
//!   dynamic offset of one uniform buffer. The arena grows to the largest
//!   batch and stays.
//! - Determinism: no atomics; every output is one invocation's fixed-order
//!   sum or a fixed reduction tree, independent of the other rows of the
//!   batch, so a turn's outputs are the same bytes whatever it is batched
//!   with, and a run repeats on the same machine. (Floats differ across GPUs
//!   and drivers; the AI runs only where the game is authoritative.)
//! - Native: [`Batcher`] owns the device on a worker thread and runs the
//!   batches; wasm32 awaits [`Gpu::new`] and [`Gpu::run`] (no blocking).
//!   `AURELIA_V2_BATCH` picks how it forms a batch (`free`, the default:
//!   whoever waits; `window:US`; `lockstep[:MS]`: every game thread holding
//!   a scorer, as vectorized environments), `AURELIA_V2_CPU_SLOTS=C` lets at
//!   most C of those threads run on the CPU at a time (a turn waiting for
//!   the GPU gives its slot up). Neither changes a turn's outputs: they are
//!   the same bytes in any batch.
//! - Subgroup matrices: where the adapter has wgpu's (experimental, native)
//!   cooperative matrix (Metal's simdgroup 8 × 8 f32), the GEMMs and the
//!   attention run the kernels of `trunk_mma.wgsl` (`AURELIA_V2_GPU_NO_MMA`
//!   keeps the plain ones, which every other backend and the web use).

use std::borrow::Cow;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use super::trunk::{self, Job, TrunkOut};
use super::weights::{Lin, Net};

const NONE: u32 = u32::MAX;
/// Query rows of an attention workgroup (`trunk.wgsl` `QB`).
const QB: u32 = 16;
const PARAM_STRIDE: usize = 64; // u32 per dispatch (256 bytes, the uniform offset alignment)

#[derive(Clone, Copy, PartialEq, Eq)]
enum K {
    Gemm,
    Hex,
    Norm,
    Pool,
    Gather,
    Attn,
    Gru,
    Combat,
    /// `gemm_mmad`: the GEMM on the subgroup matrix units straight from the buffers (only with `mma`).
    GemmD,
}

const KERNELS: [(K, &str); 9] = [
    (K::Gemm, "gemm"),
    (K::Hex, "hexconv"),
    (K::Norm, "norm"),
    (K::Pool, "pool"),
    (K::Gather, "gather"),
    (K::Attn, "attn"),
    (K::Gru, "gru"),
    (K::Combat, "combat"),
    (K::GemmD, "gemm_mmad"),
];

/// Floats past the end of the weights and of the arena that `gemm_mmad` may
/// read (a tile's columns past n, rows past m; their results are dropped).
const PAD_W: usize = 64;
const PAD_A: usize = 32 * 1024;

/// A layer's place in the weights buffer (f32 offsets).
#[derive(Debug, Clone, Copy)]
struct L {
    w: u32,
    b: u32,
    inp: u32,
    out: u32,
}

#[derive(Debug, Default)]
struct Pack(Vec<f32>);

impl Pack {
    fn put(&mut self, v: &[f32]) -> u32 {
        let at = self.0.len() as u32;
        self.0.extend_from_slice(v);
        // 16-float alignment: aligned loads where the backend vectorises.
        while !self.0.len().is_multiple_of(16) {
            self.0.push(0.0);
        }
        at
    }
    fn lin(&mut self, l: &Lin, bias: bool) -> L {
        L { w: self.put(&l.wt), b: if bias { self.put(&l.b) } else { NONE }, inp: l.inp as u32, out: l.out as u32 }
    }
}

#[derive(Debug)]
struct HexW {
    dw: u32,
    dw_bias: u32,
    edge: u32,
    pw1: L,
    pw2: L,
    dilation: usize,
}

#[derive(Debug)]
struct BlockW {
    qkv: L,
    qn: u32,
    kn: u32,
    out: L,
    ff1: L,
    ff2: L,
    dist_bias: u32,
}

#[derive(Debug)]
struct Weights {
    tile_embed: L,
    map: Vec<HexW>,
    map_norm: u32,
    region: L,
    entity: L,
    glob: L,
    slot_player: L,
    slot_unit: L,
    slot_nation: L,
    gru: [(L, L); 3],
    event: L,
    xq: L,
    xkv: L,
    xout: L,
    blocks: Vec<BlockW>,
    final_norm: u32,
    value_hidden: L,
    value_out: L,
    ca_kv: L,
    cand_ent: L,
    att: L,
    dfd: L,
    l2: L,
    head: L,
}

/// A layer after an RMSNorm with gain `g`: `diag(g) · W` (the kernel scales
/// each row by its rsqrt(mean x² + ε) after the product).
fn fold(g: &[f32], l: &Lin) -> Lin {
    let mut f = l.clone();
    for (i, gi) in g.iter().enumerate() {
        for v in &mut f.wt[i * l.out..(i + 1) * l.out] {
            *v *= gi;
        }
    }
    f
}

fn pack(net: &Net) -> (Vec<f32>, Weights) {
    let mut p = Pack::default();
    let map = net
        .map
        .iter()
        .map(|b| HexW { dw: p.put(&b.dw), dw_bias: p.put(&b.dw_bias), edge: p.put(&b.edge), pw1: p.lin(&fold(&b.norm, &b.pw1), true), pw2: p.lin(&b.pw2, true), dilation: b.dilation })
        .collect();
    let blocks = net
        .blocks
        .iter()
        .map(|b| BlockW {
            qkv: p.lin(&fold(&b.norm1, &b.qkv), true),
            qn: p.put(&b.qn),
            kn: p.put(&b.kn),
            out: p.lin(&b.out, true),
            ff1: p.lin(&fold(&b.norm2, &b.ff1), true),
            ff2: p.lin(&b.ff2, true),
            dist_bias: p.put(&b.dist_bias),
        })
        .collect();
    let w = Weights {
        tile_embed: p.lin(&net.tile_embed, true),
        map,
        map_norm: p.put(&net.map_norm),
        region: p.lin(&net.region, true),
        entity: p.lin(&net.entity, true),
        glob: p.lin(&net.glob, true),
        slot_player: p.lin(&net.slot_player, true),
        slot_unit: p.lin(&net.slot_unit, true),
        slot_nation: p.lin(&net.slot_nation, true),
        gru: [
            (p.lin(&net.gru_player.ih, true), p.lin(&net.gru_player.hh, true)),
            (p.lin(&net.gru_unit.ih, true), p.lin(&net.gru_unit.hh, true)),
            (p.lin(&net.gru_nation.ih, true), p.lin(&net.gru_nation.hh, true)),
        ],
        event: p.lin(&net.event, true),
        xq: p.lin(&net.xq, true),
        xkv: p.lin(&net.xkv, true),
        xout: p.lin(&net.xout, true),
        blocks,
        final_norm: p.put(&net.final_norm),
        value_hidden: p.lin(&net.value_hidden, true),
        value_out: p.lin(&net.value_out, true),
        ca_kv: p.lin(&net.ca_kv, true),
        cand_ent: p.lin(&net.cand_ent, false),
        att: p.lin(&net.combat.att, true),
        dfd: p.lin(&net.combat.dfd, false),
        l2: p.lin(&net.combat.l2, true),
        head: p.lin(&net.combat.head, true),
    };
    let mut data = p.0;
    data.extend([0.0; PAD_W]);
    (data, w)
}

/// Counters of the GPU's batches.
#[derive(Debug, Default, Clone, Copy)]
pub struct Stats {
    pub batches: u64,
    pub turns: u64,
    pub cpu_turns: u64,
    /// Encoding and upload, the GPU's run to the read-back, the outputs' split.
    pub encode_ns: u64,
    pub wait_ns: u64,
    pub read_ns: u64,
    pub dispatches: u64,
    /// The largest arena (bytes) and the weights (bytes).
    pub arena_bytes: u64,
    pub weight_bytes: u64,
    pub largest_batch: u64,
    /// The batcher (native): the turns' time from the request to their
    /// batch's start, the worker's time in batches, the first batch's start
    /// and the last one's end (its busy share), batches by size (1, 2, 3–4,
    /// 5–8, 9–16, 17–32, 33–64).
    pub queue_ns: u64,
    pub busy_ns: u64,
    pub first_ns: u64,
    pub last_ns: u64,
    pub sizes: [u64; 7],
    /// `AURELIA_V2_GPU_PROFILE`: per kernel (`KERNELS` order), the time of its dispatches run alone, and their count.
    pub kernel_ns: [u64; 9],
    pub kernel_n: [u64; 9],
}

/// The kernels' names, in the order of [`Stats::kernel_ns`].
pub fn kernel_names() -> [&'static str; 9] {
    KERNELS.map(|(_, n)| n)
}

/// The device, the kernels and the buffers.
pub struct Gpu {
    net: Arc<Net>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    pipes: Vec<(K, wgpu::ComputePipeline)>,
    weights: wgpu::Buffer,
    w: Weights,
    arena: Option<wgpu::Buffer>,
    iarena: Option<wgpu::Buffer>,
    params: Option<wgpu::Buffer>,
    readback: Option<wgpu::Buffer>,
    bind: Option<wgpu::BindGroup>,
    max_bytes: u64,
    /// The GEMM and the attention on the subgroup matrix units.
    mma: bool,
    mma_attn: bool,
    pub adapter: String,
    pub stats: Stats,
}

/// The kernels' source; `mma` adds `gemm_mma` (the cooperative-matrix GEMM).
fn shader(net: &Net, mma: bool) -> String {
    let c = &net.cfg;
    format!(
        "{}const DT: u32 = {}u;\nconst DH: u32 = {}u;\nconst CH: u32 = {}u;\nconst CW: u32 = {}u;\n{}{}",
        if mma { "enable wgpu_cooperative_matrix;\n" } else { "" },
        c.d_tile,
        c.head_dim(),
        net.combat.att.out,
        net.combat.l2.out,
        include_str!("trunk.wgsl"),
        if mma { include_str!("trunk_mma.wgsl") } else { "" }
    )
}

/// The kernels' module. wgpu's loop bounding (a counter in every loop, which
/// keeps the backend compiler from unrolling and puts the kernels' small
/// arrays in memory) is off: the loops here are bounded by construction.
/// Bounds checks stay on unless `AURELIA_V2_GPU_UNCHECKED` is set.
fn make_module(device: &wgpu::Device, net: &Net, mma: bool) -> wgpu::ShaderModule {
    let desc = wgpu::ShaderModuleDescriptor { label: Some("v2 trunk"), source: wgpu::ShaderSource::Wgsl(Cow::Owned(shader(net, mma))) };
    let mut checks = wgpu::ShaderRuntimeChecks::checked();
    checks.force_loop_bounding = std::env::var_os("AURELIA_V2_GPU_LOOP_BOUNDING").is_some();
    checks.bounds_checks = std::env::var_os("AURELIA_V2_GPU_UNCHECKED").is_none();
    // SAFETY: every loop of trunk.wgsl ends (counts bounded by the dispatch's
    // parameters); the bounds checks are kept by default.
    unsafe { device.create_shader_module_trusted(desc, checks) }
}

/// The pipelines' options: no zeroing of workgroup memory (naga's prologue
/// has one invocation clear every shared array before the first barrier, a
/// serial cost at the start of every workgroup); every kernel here writes a
/// shared value before any invocation reads it.
fn compile_options() -> wgpu::PipelineCompilationOptions<'static> {
    wgpu::PipelineCompilationOptions { zero_initialize_workgroup_memory: false, ..Default::default() }
}

/// A test device with the cooperative matrix where the adapter has it (and whether it has).
#[cfg(test)]
async fn test_device() -> Option<(wgpu::Device, wgpu::Queue, bool)> {
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, ..Default::default() }).await.ok()?;
    let mma = adapter.features().contains(wgpu::Features::EXPERIMENTAL_COOPERATIVE_MATRIX);
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            required_limits: adapter.limits(),
            required_features: if mma { wgpu::Features::EXPERIMENTAL_COOPERATIVE_MATRIX } else { wgpu::Features::empty() },
            // SAFETY: as in `Gpu::new`.
            experimental_features: if mma { unsafe { wgpu::ExperimentalFeatures::enabled() } } else { wgpu::ExperimentalFeatures::disabled() },
            ..Default::default()
        })
        .await
        .ok()?;
    Some((device, queue, mma))
}

fn bytes(v: &[f32]) -> &[u8] {
    // SAFETY: f32 has no padding and u8 no alignment; the slice covers exactly the floats.
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), std::mem::size_of_val(v)) }
}

fn bytes_u32(v: &[u32]) -> &[u8] {
    // SAFETY: as `bytes`.
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), std::mem::size_of_val(v)) }
}

/// A one-shot future on a buffer's mapping.
/// A mapping's result and the waker of the task that awaits it.
type MapState = Arc<Mutex<(Option<Result<(), wgpu::BufferAsyncError>>, Option<Waker>)>>;

struct MapWait(MapState);

impl Future for MapWait {
    type Output = Result<(), wgpu::BufferAsyncError>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut g = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match g.0.take() {
            Some(r) => Poll::Ready(r),
            None => {
                g.1 = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

/// A turn's rows in the batch.
#[derive(Debug, Default, Clone, Copy)]
struct Span {
    tiles: u32,
    regions: u32,
    ents: u32,
    tokens: u32,
    units: u32,
    nations: u32,
    events: u32,
    dealt: u32,
}

/// Arena offsets (floats), bump-allocated.
#[derive(Default)]
struct Bump(u32);

impl Bump {
    fn take(&mut self, n: usize) -> u32 {
        let at = self.0;
        self.0 += n.next_multiple_of(16) as u32;
        at
    }
}

/// 2-D workgroup grid of `n` workgroups.
fn grid(n: u32) -> (u32, u32) {
    if n <= 65535 {
        (n.max(1), 1)
    } else {
        (65535, n.div_ceil(65535))
    }
}

struct Rec {
    calls: Vec<(K, (u32, u32))>,
    params: Vec<u32>,
    /// The kernels on the subgroup matrix units (`attn_mma` takes half a block a workgroup).
    mma: bool,
    mma_attn: bool,
}

impl Rec {
    fn push(&mut self, k: K, groups: (u32, u32), p: &[u32]) {
        if groups.0 == 0 || groups.1 == 0 {
            return;
        }
        let mut row = [0u32; PARAM_STRIDE];
        row[..p.len()].copy_from_slice(p);
        self.params.extend_from_slice(&row);
        self.calls.push((k, groups));
    }

    /// out[r] (·) = act(x[r] · W + b) …
    #[allow(clippy::too_many_arguments)]
    fn gemm(&mut self, m: usize, l: L, x_off: u32, x_stride: u32, out_off: u32, out_stride: u32, in_map: u32, out_map: u32, add_map: u32, act: u32, acc: u32) {
        self.gemm_rms(m, l, x_off, x_stride, out_off, out_stride, in_map, out_map, add_map, act, acc, 0);
    }

    /// `gemm` with the rows RMSNormed first (`rms` 1; the norm's gain folded
    /// into the layer) and the heads of q and k RMSNormed after (`hn`: the
    /// gains of q and k, NONE none; the q block's width).
    #[allow(clippy::too_many_arguments)]
    fn gemm_rms(&mut self, m: usize, l: L, x_off: u32, x_stride: u32, out_off: u32, out_stride: u32, in_map: u32, out_map: u32, add_map: u32, act: u32, acc: u32, rms: u32) {
        self.gemm_hn(m, l, x_off, x_stride, out_off, out_stride, in_map, out_map, add_map, act, acc, rms, (NONE, NONE, 0));
    }

    #[allow(clippy::too_many_arguments)]
    fn gemm_hn(&mut self, m: usize, l: L, x_off: u32, x_stride: u32, out_off: u32, out_stride: u32, in_map: u32, out_map: u32, add_map: u32, act: u32, acc: u32, rms: u32, hn: (u32, u32, u32)) {
        if m == 0 {
            return;
        }
        let g = (l.out.div_ceil(64), (m as u32).div_ceil(32));
        debug_assert!(x_stride <= 1024, "PAD_A covers 32 rows of 1024");
        let direct = self.mma && in_map == NONE && l.inp.is_multiple_of(8);
        self.push(if direct { K::GemmD } else { K::Gemm }, g, &[m as u32, l.inp, l.out, x_off, x_stride, l.w, l.b, out_off, out_stride, in_map, out_map, add_map, act, acc, rms, hn.0, hn.1, hn.2]);
    }

    /// The attention of `blocks` blocks of query rows, `heads` heads (p as `attn`; no QK-norm on the way in).
    fn attn(&mut self, blocks: u32, heads: u32, p: &[u32]) {
        debug_assert!(p[14] == NONE && p[15] == NONE, "attn_mma has no QK-norm");
        self.push(K::Attn, grid(blocks * heads * if self.mma_attn { 2 } else { 1 }), p);
    }

    #[allow(clippy::too_many_arguments)]
    fn norm(&mut self, rows: usize, in_off: u32, in_stride: u32, width: u32, nseg: u32, w: u32, out_off: u32, out_stride: u32, mode: u32, in_map: u32) {
        if rows == 0 {
            return;
        }
        self.push(K::Norm, grid(rows as u32 * nseg), &[rows as u32, in_off, in_stride, width, nseg, w, out_off, out_stride, mode, in_map]);
    }

    #[allow(clippy::too_many_arguments)]
    fn gather(&mut self, rows: usize, src_off: u32, src_stride: u32, width: u32, idx: u32, dst_off: u32, dst_stride: u32, has_col: u32) {
        if rows == 0 {
            return;
        }
        self.push(K::Gather, grid(rows as u32), &[rows as u32, src_off, src_stride, width, idx, dst_off, dst_stride, has_col]);
    }
}

impl Gpu {
    /// The first high-performance adapter, its device and the kernels.
    pub async fn new(net: Arc<Net>) -> Result<Gpu, String> {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, ..Default::default() })
            .await
            .map_err(|e| format!("wgpu: no adapter ({e})"))?;
        let info = adapter.get_info();
        if net.cfg.head_dim() != 64 || net.combat.l2.out != 64 {
            return Err("v2 gpu: the kernels want heads of 64 (a GEMM tile is a head) and a combat width of 64".into());
        }
        let limits = adapter.limits();
        // GPU timestamps only to measure (`AURELIA_V2_GPU_PROFILE`).
        let mut features = if std::env::var_os("AURELIA_V2_GPU_PROFILE").is_some() { adapter.features() & wgpu::Features::TIMESTAMP_QUERY } else { wgpu::Features::empty() };
        // The GEMMs on the subgroup matrix units where the adapter has them
        // (native only; `AURELIA_V2_GPU_NO_MMA` keeps the plain kernel).
        let mma = adapter.features().contains(wgpu::Features::EXPERIMENTAL_COOPERATIVE_MATRIX) && std::env::var_os("AURELIA_V2_GPU_NO_MMA").is_none();
        if mma {
            features |= wgpu::Features::EXPERIMENTAL_COOPERATIVE_MATRIX;
        }
        // `AURELIA_V2_GPU_NO_MMA_ATTN`: the GEMMs on the matrix units, the attention plain.
        let mma_attn = mma && std::env::var_os("AURELIA_V2_GPU_NO_MMA_ATTN").is_none();
        if limits.max_compute_workgroup_storage_size < 16384 || limits.max_compute_invocations_per_workgroup < 256 {
            return Err(format!("wgpu: {} lacks the WebGPU default compute limits", info.name));
        }
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("v2"),
                required_features: features,
                required_limits: limits.clone(),
                // SAFETY: the one experimental feature asked for is the cooperative matrix, used only by
                // `gemm_mma` (8 × 8 f32 loads, multiply-adds and stores within workgroup memory).
                experimental_features: if mma { unsafe { wgpu::ExperimentalFeatures::enabled() } } else { wgpu::ExperimentalFeatures::disabled() },
                ..Default::default()
            })
            .await
            .map_err(|e| format!("wgpu: device ({e})"))?;
        let module = make_module(&device, &net, mma);
        let storage = |binding: u32, read_only: bool| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only }, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("v2"),
            entries: &[
                storage(0, false),
                storage(1, true),
                storage(2, true),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: true, min_binding_size: wgpu::BufferSize::new(128) },
                    count: None,
                },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("v2"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipes = KERNELS
            .iter()
            .filter(|(k, _)| mma || *k != K::GemmD)
            .map(|(k, name)| {
                let p = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(name),
                    layout: Some(&pl),
                    module: &module,
                    entry_point: Some(match (k, mma) {
                        (K::Gemm, true) => "gemm_mma",
                        (K::Attn, true) if mma_attn => "attn_mma",
                        _ => name,
                    }),
                    compilation_options: compile_options(),
                    cache: None,
                });
                (*k, p)
            })
            .collect();
        let (data, w) = pack(&net);
        let weights = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("v2 weights"),
            size: (data.len() * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&weights, 0, bytes(&data));
        let max_bytes = limits.max_storage_buffer_binding_size.min(limits.max_buffer_size).min(2 << 30);
        let stats = Stats { weight_bytes: (data.len() * 4) as u64, ..Default::default() };
        Ok(Gpu {
            net,
            device,
            queue,
            layout,
            pipes,
            weights,
            w,
            arena: None,
            iarena: None,
            params: None,
            readback: None,
            bind: None,
            max_bytes,
            mma,
            mma_attn,
            adapter: format!("{} ({:?}{})", info.name, info.backend, if mma { ", mma" } else { "" }),
            stats,
        })
    }


    /// Buffers of at least these sizes (bytes), the bind group remade when one grows.
    fn ensure(&mut self, arena: u64, iarena: u64, params: u64, readback: u64) {
        let grow = |cur: &Option<wgpu::Buffer>, want: u64| cur.as_ref().is_none_or(|b| b.size() < want);
        let mk = |device: &wgpu::Device, label: &str, size: u64, usage: wgpu::BufferUsages| {
            device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: size.next_multiple_of(256), usage, mapped_at_creation: false })
        };
        let mut remade = false;
        let up = |want: u64| (want + want / 4).max(1 << 16);
        if grow(&self.arena, arena) {
            self.arena = Some(mk(&self.device, "v2 arena", up(arena).min(self.max_bytes), wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC));
            self.stats.arena_bytes = self.stats.arena_bytes.max(self.arena.as_ref().map_or(0, wgpu::Buffer::size));
            remade = true;
        }
        if grow(&self.iarena, iarena) {
            self.iarena = Some(mk(&self.device, "v2 index", up(iarena), wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST));
            remade = true;
        }
        if grow(&self.params, params) {
            self.params = Some(mk(&self.device, "v2 params", up(params), wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST));
            remade = true;
        }
        if grow(&self.readback, readback) {
            self.readback = Some(mk(&self.device, "v2 readback", up(readback), wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST));
        }
        if remade || self.bind.is_none() {
            fn buf(b: &Option<wgpu::Buffer>) -> wgpu::BindingResource<'_> {
                b.as_ref().expect("made above").as_entire_binding()
            }
            self.bind = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("v2"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: buf(&self.arena) },
                    wgpu::BindGroupEntry { binding: 1, resource: self.weights.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: buf(&self.iarena) },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: self.params.as_ref().expect("made above"), offset: 0, size: wgpu::BufferSize::new(128) }),
                    },
                ],
            }));
        }
    }

    /// `AURELIA_V2_GPU_PROFILE`: the batch's dispatches again, each in a pass
    /// of its own with GPU timestamps around it (a measurement, not the play
    /// path): the time per kernel into the stats.
    fn profile(&mut self, rec: &Rec) {
        let n = rec.calls.len() as u32;
        let qs = self.device.create_query_set(&wgpu::QuerySetDescriptor { label: None, ty: wgpu::QueryType::Timestamp, count: 2 * n });
        let size = u64::from(2 * n) * 8;
        let resolve = self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
        let read = self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        let bind = self.bind.as_ref().expect("ensured");
        for (i, (k, (gx, gy))) in rec.calls.iter().enumerate() {
            let tw = wgpu::ComputePassTimestampWrites { query_set: &qs, beginning_of_pass_write_index: Some(2 * i as u32), end_of_pass_write_index: Some(2 * i as u32 + 1) };
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: Some(tw) });
            pass.set_pipeline(&self.pipes.iter().find(|(p, _)| p == k).expect("a kernel").1);
            pass.set_bind_group(0, bind, &[(i * PARAM_STRIDE * 4) as u32]);
            pass.dispatch_workgroups(*gx, *gy, 1);
        }
        enc.resolve_query_set(&qs, 0..2 * n, &resolve, 0);
        enc.copy_buffer_to_buffer(&resolve, 0, &read, 0, size);
        self.queue.submit([enc.finish()]);
        read.map_async(wgpu::MapMode::Read, .., |r| r.expect("mapped"));
        #[cfg(not(target_arch = "wasm32"))]
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        let ts: Vec<u64> = read.get_mapped_range(..).expect("mapped").chunks_exact(8).map(|c| u64::from_le_bytes(c.try_into().expect("8"))).collect();
        let period = f64::from(self.queue.get_timestamp_period());
        let calls = std::env::var_os("AURELIA_V2_GPU_PROFILE_CALLS").is_some();
        for (i, (k, (gx, gy))) in rec.calls.iter().enumerate() {
            let ns = (ts[2 * i + 1].saturating_sub(ts[2 * i]) as f64 * period) as u64;
            let ki = KERNELS.iter().position(|(kk, _)| kk == k).expect("a kernel");
            self.stats.kernel_ns[ki] += ns;
            self.stats.kernel_n[ki] += 1;
            if calls {
                eprintln!("{} {:?} grid {gx}x{gy}: {:.3} ms", KERNELS[ki].1, &rec.params[i * PARAM_STRIDE..][..14], ns as f64 / 1e6);
            }
        }
    }

    /// The trunks of a batch of turns.
    pub async fn run(&mut self, jobs: &[&Job]) -> Vec<TrunkOut> {
        if jobs.is_empty() {
            return Vec::new();
        }
        let t0 = super::now_ns();
        let net = self.net.clone();
        let cfg = &net.cfg;
        let (dt, d) = (cfg.d_tile as u32, cfg.d_model as u32);
        let nh = cfg.heads as u32;
        let dh = d / nh;
        let ch = net.combat.att.out as u32;
        let w = &self.w;
        let b = jobs.len();
        // Spans and totals.
        let mut sp = Vec::with_capacity(b);
        let mut tot = Span::default();
        for j in jobs {
            let m = j.map();
            let s = Span {
                tiles: m.n_tiles as u32,
                regions: m.n_regions as u32,
                ents: j.n_ent as u32,
                tokens: j.tokens() as u32,
                units: j.slots.iter().filter(|s| s.unit).count() as u32,
                nations: j.slots.iter().filter(|s| !s.unit).count() as u32,
                events: j.n_ev as u32,
                dealt: (j.n_ent * j.n_ent) as u32,
            };
            tot.tiles += s.tiles;
            tot.regions += s.regions;
            tot.ents += s.ents;
            tot.tokens += s.tokens;
            tot.units += s.units;
            tot.nations += s.nations;
            tot.events += s.events;
            tot.dealt += s.dealt;
            sp.push(s);
        }
        let (tt, rt, et, nt, vt) = (tot.tiles as usize, tot.regions as usize, tot.ents as usize, tot.tokens as usize, tot.events as usize);
        let (ut, nat) = (tot.units as usize, tot.nations as usize);
        let wt = net.norm_tile.width();
        let ein = net.entity.inp;
        let wgl = net.norm_global.width();
        let vin = if cfg.events { net.event.inp } else { 0 };
        let nside = net.combat.att.inp;
        let (mp, mu, mn) = (cfg.mem_player, cfg.mem_unit, cfg.mem_nation);
        // The arena: inputs, outputs, scratch.
        let mut a = Bump::default();
        let a_tin = a.take(tt * wt);
        let a_ent_in = a.take(et * ein);
        let a_glob = a.take(b * wgl);
        let a_memp = a.take(b * mp);
        let a_memu = a.take(ut * mu);
        let a_memn = a.take(nat * mn);
        let a_ev_in = a.take(vt * vin);
        let a_side = a.take(et * nside);
        let a_pos = a.take(nt * 4);
        let in_end = a.0;
        let a_tile_h = a.take(tt * dt as usize);
        let a_tok = a.take(nt * d as usize);
        let a_val = a.take(b);
        let a_newp = a.take(b * mp);
        let a_newu = a.take(ut * mu);
        let a_newn = a.take(nat * mn);
        let a_kv = a.take(et * 2 * d as usize);
        let a_pent = a.take(et * d as usize);
        let a_att = a.take(et * ch as usize);
        let a_dfd = a.take(et * ch as usize);
        let a_dealt = a.take(tot.dealt as usize);
        let out_end = a.0;
        let a_h0 = a.take(tt * dt as usize);
        let a_h1 = a.take(tt * dt as usize);
        let a_mid = a.take(tt * net.map.first().map_or(0, |b| b.pw1.out));
        let a_pool = a.take(rt * 2 * dt as usize);
        let a_x = a.take(nt * d as usize);
        let a_qkv = a.take(nt * 3 * d as usize);
        let a_o = a.take(nt * d as usize);
        let a_ff = a.take(nt * cfg.d_ff);
        let a_ev = a.take(vt * d as usize);
        let a_evln = a.take(vt * d as usize);
        let a_evkv = a.take(vt * 2 * d as usize);
        let a_vh = a.take(b * cfg.d_head);
        let a_gi = a.take((b * mp + ut * mu + nat * mn) * 3);
        let a_gh = a.take((b * mp + ut * mu + nat * mn) * 3);
        // Event rows: the tokens of the turns that have events.
        let nev: usize = jobs.iter().zip(&sp).filter(|(j, _)| j.n_ev > 0).map(|(_, s)| s.tokens as usize).sum();
        let a_lx = a.take(nev * d as usize);
        let a_eq = a.take(nev * d as usize);
        let a_eo = a.take(nev * d as usize);
        let arena_end = (a.0 as usize + PAD_A) as u64;
        // Inputs and index tables on the host.
        let mut host = vec![0.0f32; in_end as usize];
        let mut ix: Vec<u32> = Vec::new();
        let put_ix = |v: Vec<u32>, ix: &mut Vec<u32>| {
            let at = ix.len() as u32;
            ix.extend(v);
            at
        };
        let dilations: Vec<usize> = {
            let mut v: Vec<usize> = w.map.iter().map(|h| h.dilation).collect();
            v.dedup();
            v.sort_unstable();
            v.dedup();
            v
        };
        let mut dil_ix = Vec::new();
        for &k in &dilations {
            let mut v = Vec::with_capacity(tt * 7);
            let mut toff = 0u32;
            for (j, s) in jobs.iter().zip(&sp) {
                let n = s.tiles;
                v.extend(j.map().dilated(k).iter().map(|&i| if i == n { NONE } else { toff + i }));
                toff += n;
            }
            dil_ix.push((k, put_ix(v, &mut ix)));
        }
        let (mut reg_start, mut reg_tiles, mut reg_rows, mut ent_rows, mut glob_rows, mut player_rows) = (vec![0u32], Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let (mut unit_rows, mut unit_ent, mut nation_rows, mut nation_ent, mut ent_tile) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let (mut ev_reg, mut ev_act, mut evq_rows, mut ev_range, mut tok_range, mut table) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut off = Span::default();
        for (j, s) in jobs.iter().zip(&sp) {
            let m = j.map();
            let (t0_, n0, e0, v0) = (off.tiles, off.tokens, off.ents, off.events);
            let r = s.regions;
            for g in 0..m.n_regions {
                for &t in &m.region_tiles[m.region_start[g] as usize..m.region_start[g + 1] as usize] {
                    reg_tiles.push(t0_ + t);
                }
                reg_start.push(reg_tiles.len() as u32);
                reg_rows.push(n0 + g as u32);
            }
            for e in 0..j.n_ent {
                ent_rows.push(n0 + r + e as u32);
                let et = j.ent_tile[e];
                ent_tile.push(if et >= 0 { t0_ + et as u32 } else { NONE });
                let flag = u32::from(j.ready[e] || j.hostile[e]);
                table.extend([e0, s.ents, a_dealt + off.dealt, flag]);
            }
            let o_glob = n0 + r + s.ents;
            glob_rows.push(o_glob);
            player_rows.push(o_glob + 1);
            for (k, slot) in j.slots.iter().enumerate() {
                let row = o_glob + 2 + k as u32;
                let ent = n0 + r + slot.entity as u32;
                if slot.unit {
                    unit_rows.push(row);
                    unit_ent.push(ent);
                } else {
                    nation_rows.push(row);
                    nation_ent.push(ent);
                }
            }
            for v in 0..j.n_ev {
                let g = j.ev_region[v];
                ev_reg.push(if g >= 0 { n0 + g as u32 } else { NONE });
                let ac = j.ev_actor[v];
                ev_act.push(if ac >= 0 { n0 + r + ac as u32 } else { NONE });
            }
            // Attention blocks of QB query rows: the turn's tokens over its tokens, and
            // (with events) its tokens' compact rows over its events.
            let mut q = 0;
            while q < s.tokens {
                let c = QB.min(s.tokens - q);
                tok_range.extend([n0 + q, c, n0, s.tokens]);
                if j.n_ev > 0 {
                    ev_range.extend([evq_rows.len() as u32 + q, c, v0, s.events]);
                }
                q += c;
            }
            if j.n_ev > 0 {
                evq_rows.extend(n0..n0 + s.tokens);
            }
            // Host inputs.
            host[(a_tin as usize + t0_ as usize * wt)..][..j.tiles.len()].copy_from_slice(&j.tiles);
            host[(a_ent_in as usize + e0 as usize * ein)..][..j.ent_in.len()].copy_from_slice(&j.ent_in);
            host[(a_side as usize + e0 as usize * nside)..][..j.side.len()].copy_from_slice(&j.side);
            host[(a_ev_in as usize + v0 as usize * vin)..][..j.ev_in.len()].copy_from_slice(&j.ev_in);
            for (k, p) in j.positions().into_iter().enumerate() {
                let at = a_pos as usize + (n0 as usize + k) * 4;
                host[at] = p.0;
                host[at + 1] = p.1;
                host[at + 2] = if p.2 { 1.0 } else { 0.0 };
            }
            off.tiles += s.tiles;
            off.regions += s.regions;
            off.ents += s.ents;
            off.tokens += s.tokens;
            off.units += s.units;
            off.nations += s.nations;
            off.events += s.events;
            off.dealt += s.dealt;
        }
        {
            let (mut u, mut n) = (0usize, 0usize);
            for (bi, j) in jobs.iter().enumerate() {
                host[a_glob as usize + bi * wgl..][..wgl].copy_from_slice(&j.glob);
                host[a_memp as usize + bi * mp..][..mp].copy_from_slice(&j.mem_player);
                host[a_memu as usize + u..][..j.mem_unit.len()].copy_from_slice(&j.mem_unit);
                u += j.mem_unit.len();
                host[a_memn as usize + n..][..j.mem_nation.len()].copy_from_slice(&j.mem_nation);
                n += j.mem_nation.len();
            }
        }
        let i_reg_start = put_ix(reg_start, &mut ix);
        let i_reg_tiles = put_ix(reg_tiles, &mut ix);
        let i_reg_rows = put_ix(reg_rows, &mut ix);
        let i_ent_rows = put_ix(ent_rows, &mut ix);
        let i_glob_rows = put_ix(glob_rows, &mut ix);
        let i_player_rows = put_ix(player_rows, &mut ix);
        let i_unit_rows = put_ix(unit_rows, &mut ix);
        let i_unit_ent = put_ix(unit_ent, &mut ix);
        let i_nation_rows = put_ix(nation_rows, &mut ix);
        let i_nation_ent = put_ix(nation_ent, &mut ix);
        let i_ent_tile = put_ix(ent_tile, &mut ix);
        let i_ev_reg = put_ix(ev_reg, &mut ix);
        let i_ev_act = put_ix(ev_act, &mut ix);
        let i_evq_rows = put_ix(evq_rows, &mut ix);
        let n_ev_blocks = (ev_range.len() / 4) as u32;
        let n_blocks = (tok_range.len() / 4) as u32;
        let i_ev_range = put_ix(ev_range, &mut ix);
        let i_tok_range = put_ix(tok_range, &mut ix);
        let i_table = put_ix(table, &mut ix);
        if ix.is_empty() {
            ix.push(0);
        }
        // The dispatches.
        let mut rec = Rec { calls: Vec::new(), params: Vec::new(), mma: self.mma, mma_attn: self.mma_attn };
        let d_ = d as usize;
        // Map.
        rec.gemm(tt, w.tile_embed, a_tin, wt as u32, a_h0, dt, NONE, NONE, NONE, 0, 0);
        // Map blocks: the convolution, then RMSNorm + pw1 + ReLU², pw2 added to the states.
        for h in &w.map {
            let idx = dil_ix.iter().find(|(k, _)| *k == h.dilation).expect("a dilation").1;
            rec.push(K::Hex, grid(((tt * dt as usize) as u32).div_ceil(64)), &[tt as u32, a_h0, a_h1, idx, h.dw, h.dw_bias, h.edge]);
            rec.gemm_rms(tt, h.pw1, a_h1, dt, a_mid, h.pw1.out, NONE, NONE, NONE, 2, 0, 1);
            rec.gemm(tt, h.pw2, a_mid, h.pw1.out, a_h0, dt, NONE, NONE, NONE, 0, 1);
        }
        rec.norm(tt, a_h0, dt, dt, 1, w.map_norm, a_tile_h, dt, 0, NONE);
        // Tokens: regions, entities, global, player, slots.
        if rt > 0 {
            rec.push(K::Pool, grid(rt as u32), &[rt as u32, a_tile_h, i_reg_start, i_reg_tiles, a_pool]);
        }
        rec.gemm(rt, w.region, a_pool, 2 * dt, a_x, d, NONE, i_reg_rows, NONE, 0, 0);
        let c0 = (ein - dt as usize - 1) as u32;
        rec.gather(et, a_tile_h, dt, dt, i_ent_tile, a_ent_in + c0, ein as u32, dt);
        rec.gemm(et, w.entity, a_ent_in, ein as u32, a_x, d, NONE, i_ent_rows, NONE, 0, 0);
        rec.gemm(b, w.glob, a_glob, wgl as u32, a_x, d, NONE, i_glob_rows, NONE, 0, 0);
        rec.gemm(b, w.slot_player, a_memp, mp as u32, a_x, d, NONE, i_player_rows, NONE, 0, 0);
        rec.gemm(ut, w.slot_unit, a_memu, mu as u32, a_x, d, NONE, i_unit_rows, i_unit_ent, 0, 0);
        rec.gemm(nat, w.slot_nation, a_memn, mn as u32, a_x, d, NONE, i_nation_rows, i_nation_ent, 0, 0);
        // Events.
        if cfg.events && vt > 0 && nev > 0 {
            let wv = net.norm_event.width() as u32;
            rec.gather(vt, a_x, d, d, i_ev_reg, a_ev_in + wv, vin as u32, d);
            rec.gather(vt, a_x, d, d, i_ev_act, a_ev_in + wv + d + 1, vin as u32, d);
            rec.gemm(vt, w.event, a_ev_in, vin as u32, a_ev, d, NONE, NONE, NONE, 0, 0);
            rec.norm(vt, a_ev, d, d, 1, NONE, a_evln, d, 1, NONE);
            rec.gemm(vt, w.xkv, a_evln, d, a_evkv, 2 * d, NONE, NONE, NONE, 0, 0);
            rec.norm(nev, a_x, d, d, 1, NONE, a_lx, d, 1, i_evq_rows);
            rec.gemm(nev, w.xq, a_lx, d, a_eq, d, NONE, NONE, NONE, 0, 0);
            let scale = 1.0 / (dh as f32).sqrt();
            rec.attn(n_ev_blocks, nh, &[n_ev_blocks, a_eq, d, a_evkv, a_evkv + d, 2 * d, a_eo, d, i_ev_range, 0, 0, 0, scale.to_bits(), nh, NONE, NONE]);
            rec.gemm(nev, w.xout, a_eo, d, a_x, d, NONE, i_evq_rows, NONE, 0, 1);
        }
        // The transformer.
        let scale = 1.0 / (dh as f32).sqrt();
        for blk in &w.blocks {
            rec.gemm_hn(nt, blk.qkv, a_x, d, a_qkv, 3 * d, NONE, NONE, NONE, 0, 0, 1, (blk.qn, blk.kn, d));
            rec.attn(n_blocks, nh, &[n_blocks, a_qkv, 3 * d, a_qkv + d, a_qkv + 2 * d, 3 * d, a_o, d, i_tok_range, 1, a_pos, blk.dist_bias, scale.to_bits(), nh, NONE, NONE]);
            rec.gemm(nt, blk.out, a_o, d, a_x, d, NONE, NONE, NONE, 0, 1);
            rec.gemm_rms(nt, blk.ff1, a_x, d, a_ff, cfg.d_ff as u32, NONE, NONE, NONE, 2, 0, 1);
            rec.gemm(nt, blk.ff2, a_ff, cfg.d_ff as u32, a_x, d, NONE, NONE, NONE, 0, 1);
        }
        rec.norm(nt, a_x, d, d, 1, w.final_norm, a_tok, d, 0, NONE);
        // Value, memory, the decisions' tables.
        rec.gemm(b, w.value_hidden, a_tok, d, a_vh, cfg.d_head as u32, i_glob_rows, NONE, NONE, 1, 0);
        rec.gemm(b, w.value_out, a_vh, cfg.d_head as u32, a_val, 1, NONE, NONE, NONE, 0, 0);
        let mut g_at = 0u32;
        for (k, (rows, width, h_off, out_off, map)) in [(b, mp, a_memp, a_newp, i_player_rows), (ut, mu, a_memu, a_newu, i_unit_rows), (nat, mn, a_memn, a_newn, i_nation_rows)].into_iter().enumerate() {
            if rows == 0 {
                continue;
            }
            let (ih, hh) = w.gru[k];
            let gi = a_gi + g_at;
            let gh = a_gh + g_at;
            rec.gemm(rows, ih, a_tok, d, gi, 3 * width as u32, map, NONE, NONE, 0, 0);
            rec.gemm(rows, hh, h_off, width as u32, gh, 3 * width as u32, NONE, NONE, NONE, 0, 0);
            rec.push(K::Gru, grid(((rows * width) as u32).div_ceil(64)), &[rows as u32, width as u32, gi, gh, h_off, out_off]);
            g_at += (rows * width * 3) as u32;
        }
        rec.gemm(et, w.ca_kv, a_tok, d, a_kv, 2 * d, i_ent_rows, NONE, NONE, 0, 0);
        rec.gemm(et, w.cand_ent, a_tok, d, a_pent, d, i_ent_rows, NONE, NONE, 0, 0);
        rec.gemm(et, w.att, a_side, nside as u32, a_att, ch, NONE, NONE, NONE, 0, 0);
        rec.gemm(et, w.dfd, a_side, nside as u32, a_dfd, ch, NONE, NONE, NONE, 0, 0);
        if et > 0 {
            rec.push(K::Combat, grid(et as u32), &[et as u32, a_att, a_dfd, i_table, w.l2.w, w.l2.b, w.head.w, w.head.b]);
        }
        // Upload, run, read back.
        let out_bytes = u64::from(out_end - in_end) * 4;
        self.ensure(arena_end * 4, (ix.len() * 4) as u64, (rec.params.len() * 4) as u64, out_bytes);
        {
            let arena = self.arena.as_ref().expect("ensured");
            assert!(arena.size() >= arena_end * 4, "v2 gpu: a batch of {} turns needs {} MB, the device allows {} MB", b, (arena_end * 4) >> 20, self.max_bytes >> 20);
            self.queue.write_buffer(arena, 0, bytes(&host));
            self.queue.write_buffer(self.iarena.as_ref().expect("ensured"), 0, bytes_u32(&ix));
            self.queue.write_buffer(self.params.as_ref().expect("ensured"), 0, bytes_u32(&rec.params));
        }
        // Profiling runs the dispatches itself (each in a timed pass).
        let profiled = self.device.features().contains(wgpu::Features::TIMESTAMP_QUERY);
        if profiled {
            self.profile(&rec);
        }
        let arena = self.arena.as_ref().expect("ensured");
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("v2 trunk") });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("v2 trunk"), timestamp_writes: None });
            let bind = self.bind.as_ref().expect("ensured");
            let mut last: Option<K> = None;
            for (i, (k, (gx, gy))) in rec.calls.iter().enumerate().filter(|_| !profiled) {
                if last != Some(*k) {
                    pass.set_pipeline(&self.pipes.iter().find(|(p, _)| p == k).expect("a kernel").1);
                    last = Some(*k);
                }
                pass.set_bind_group(0, bind, &[(i * PARAM_STRIDE * 4) as u32]);
                pass.dispatch_workgroups(*gx, *gy, 1);
            }
        }
        let readback = self.readback.as_ref().expect("ensured");
        enc.copy_buffer_to_buffer(arena, u64::from(in_end) * 4, readback, 0, out_bytes);
        self.queue.submit([enc.finish()]);
        let t1 = super::now_ns();
        let state: MapState = Arc::new(Mutex::new((None, None)));
        let s2 = state.clone();
        readback.map_async(wgpu::MapMode::Read, 0..out_bytes, move |r| {
            let mut g = s2.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            g.0 = Some(r);
            if let Some(w) = g.1.take() {
                w.wake();
            }
        });
        #[cfg(not(target_arch = "wasm32"))]
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        MapWait(state).await.expect("v2 gpu: the read-back mapping failed");
        let t2 = super::now_ns();
        let outv: Vec<f32> = {
            let view = readback.get_mapped_range(0..out_bytes).expect("mapped");
            view.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
        };
        readback.unmap();
        // Split per turn.
        let at = |x: u32| (x - in_end) as usize;
        let mut outs = Vec::with_capacity(b);
        let mut off = Span::default();
        for (bi, (j, s)) in jobs.iter().zip(&sp).enumerate() {
            let (tiles, toks, ents) = (s.tiles as usize, s.tokens as usize, s.ents as usize);
            let sl = |base: u32, from: usize, n: usize| outv[at(base) + from..at(base) + from + n].to_vec();
            let mut o = TrunkOut {
                tile_h: sl(a_tile_h, off.tiles as usize * dt as usize, tiles * dt as usize),
                tokens: sl(a_tok, off.tokens as usize * d_, toks * d_),
                value: outv[at(a_val) + bi],
                new_player: sl(a_newp, bi * mp, mp),
                new_unit: sl(a_newu, off.units as usize * mu, s.units as usize * mu),
                new_nation: sl(a_newn, off.nations as usize * mn, s.nations as usize * mn),
                ent_kv: sl(a_kv, off.ents as usize * 2 * d_, ents * 2 * d_),
                p_ent: sl(a_pent, off.ents as usize * d_, ents * d_),
                att_side: sl(a_att, off.ents as usize * ch as usize, ents * ch as usize),
                dfd_side: sl(a_dfd, off.ents as usize * ch as usize, ents * ch as usize),
                dealt: sl(a_dealt, off.dealt as usize, s.dealt as usize),
                focus: Vec::new(),
            };
            // Rows of entities neither ready nor hostile: the kernel leaves them (zero on the CPU).
            for i in 0..ents {
                if !(j.ready[i] || j.hostile[i]) {
                    o.dealt[i * ents..(i + 1) * ents].fill(0.0);
                }
            }
            trunk::focus(j, &mut o);
            outs.push(o);
            off.tiles += s.tiles;
            off.tokens += s.tokens;
            off.ents += s.ents;
            off.units += s.units;
            off.nations += s.nations;
            off.dealt += s.dealt;
        }
        let t3 = super::now_ns();
        let st = &mut self.stats;
        st.batches += 1;
        st.turns += b as u64;
        st.dispatches += rec.calls.len() as u64;
        st.largest_batch = st.largest_batch.max(b as u64);
        st.encode_ns += t1.saturating_sub(t0);
        st.wait_ns += t2.saturating_sub(t1);
        st.read_ns += t3.saturating_sub(t2);
        outs
    }
}


#[cfg(not(target_arch = "wasm32"))]
pub use native::{Batcher, Hold, Mode};

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::thread::ThreadId;
    use std::time::{Duration, Instant};

    use super::super::trunk::{Job, TrunkOut};
    use super::super::weights::Net;
    use super::{Gpu, Stats};

    /// The most turns a batch takes.
    const MAX_BATCH: usize = 64;

    struct Req {
        job: Arc<Job>,
        reply: Sender<TrunkOut>,
        sent: u64,
    }

    /// How the worker forms a batch (`AURELIA_V2_BATCH`, read once).
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum Mode {
        /// `free` (the default): the turns waiting when the worker is free.
        Free,
        /// `window:US`: after the first turn, up to US microseconds for more.
        Window(Duration),
        /// `lockstep[:MS]`: until every game thread that holds a scorer waits
        /// (vectorized environments: a batch is all the live games), at most
        /// MS milliseconds (default 50) after the first.
        Lockstep(Duration),
    }

    impl Mode {
        pub fn from_env() -> Mode {
            static MODE: OnceLock<Mode> = OnceLock::new();
            *MODE.get_or_init(|| {
                let v = std::env::var("AURELIA_V2_BATCH").unwrap_or_default();
                let (name, arg) = v.split_once(':').map_or((v.as_str(), None), |(a, b)| (a, b.parse::<u64>().ok()));
                match name {
                    "window" => Mode::Window(Duration::from_micros(arg.unwrap_or(300))),
                    "lockstep" => Mode::Lockstep(Duration::from_millis(arg.unwrap_or(50))),
                    _ => Mode::Free,
                }
            })
        }
    }

    /// The game threads that hold a scorer (lockstep waits for them all).
    #[derive(Default)]
    struct Live {
        threads: Mutex<HashMap<ThreadId, usize>>,
        count: AtomicUsize,
    }

    /// A game thread's hold on the batcher (made with its scorer, dropped with it).
    pub struct Hold {
        live: Arc<Live>,
        thread: ThreadId,
    }

    /// `AURELIA_V2_CPU_SLOTS=C` (0 or unset: off): at most C game threads
    /// that hold a scorer run on the CPU at a time; a thread gives its slot
    /// up while its turn waits for the GPU. So many games can be in flight
    /// (to fill the GPU's batches) without more runnable threads than cores.
    struct Slots {
        free: Mutex<usize>,
        cv: std::sync::Condvar,
    }

    fn slots() -> Option<&'static Slots> {
        static SLOTS: OnceLock<Option<Slots>> = OnceLock::new();
        SLOTS
            .get_or_init(|| {
                let n = std::env::var("AURELIA_V2_CPU_SLOTS").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
                (n > 0).then(|| Slots { free: Mutex::new(n), cv: std::sync::Condvar::new() })
            })
            .as_ref()
    }

    thread_local! {
        /// The scorers this thread holds (it holds a CPU slot while > 0).
        static HELD: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    fn take_slot(s: &Slots) {
        let mut f = s.free.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        while *f == 0 {
            f = s.cv.wait(f).unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        *f -= 1;
    }

    fn give_slot(s: &Slots) {
        *s.free.lock().unwrap_or_else(std::sync::PoisonError::into_inner) += 1;
        s.cv.notify_one();
    }

    impl std::fmt::Debug for Hold {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "Hold({:?})", self.thread)
        }
    }

    impl Drop for Hold {
        fn drop(&mut self) {
            if let Some(s) = slots() {
                if self.thread == std::thread::current().id() {
                    let n = HELD.with(|h| {
                        h.set(h.get().saturating_sub(1));
                        h.get()
                    });
                    if n == 0 {
                        give_slot(s);
                    }
                }
            }
            let mut t = self.live.threads.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(n) = t.get_mut(&self.thread) {
                *n -= 1;
                if *n == 0 {
                    t.remove(&self.thread);
                }
            }
            self.live.count.store(t.len(), Ordering::Release);
        }
    }

    /// The GPU on a worker thread: the trunks of the turns that wait at the
    /// same moment (from any thread) run as one batch.
    pub struct Batcher {
        tx: Mutex<Sender<Req>>,
        stats: Arc<Mutex<Stats>>,
        live: Arc<Live>,
        pub mode: Mode,
        pub adapter: String,
    }

    impl Batcher {
        pub fn start(net: Arc<Net>) -> Result<Self, String> {
            let gpu = pollster::block_on(Gpu::new(net))?;
            let adapter = gpu.adapter.clone();
            let (tx, rx) = channel::<Req>();
            let stats = Arc::new(Mutex::new(gpu.stats));
            let st = stats.clone();
            let live = Arc::new(Live::default());
            let mode = Mode::from_env();
            let lv = live.clone();
            std::thread::Builder::new().name("v2-gpu".into()).spawn(move || worker(gpu, &rx, &st, &lv, mode)).map_err(|e| format!("v2 gpu thread: {e}"))?;
            Ok(Self { tx: Mutex::new(tx), stats, live, mode, adapter })
        }

        /// The calling thread holds a scorer until the hold drops.
        pub fn hold(&self) -> Hold {
            let thread = std::thread::current().id();
            if let Some(s) = slots() {
                if HELD.with(|h| {
                    h.set(h.get() + 1);
                    h.get()
                }) == 1
                {
                    take_slot(s);
                }
            }
            let mut t = self.live.threads.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            *t.entry(thread).or_insert(0) += 1;
            self.live.count.store(t.len(), Ordering::Release);
            Hold { live: self.live.clone(), thread }
        }

        /// The trunk of one turn (blocks until its batch is done).
        pub fn run(&self, job: &Arc<Job>) -> TrunkOut {
            let (reply, back) = channel();
            let slot = slots().filter(|_| HELD.with(std::cell::Cell::get) > 0);
            if let Some(s) = slot {
                give_slot(s);
            }
            self.tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).send(Req { job: job.clone(), reply, sent: super::super::now_ns() }).expect("the v2 gpu thread");
            let out = back.recv().expect("the v2 gpu thread");
            if let Some(s) = slot {
                take_slot(s);
            }
            out
        }

        pub fn stats(&self) -> Stats {
            *self.stats.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
        }
    }

    impl std::fmt::Debug for Batcher {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "Batcher({})", self.adapter)
        }
    }

    /// Collects a batch as the mode says and runs it.
    fn worker(mut gpu: Gpu, q: &Receiver<Req>, stats: &Mutex<Stats>, live: &Live, mode: Mode) {
        loop {
            let Ok(first) = q.recv() else { return };
            let mut reqs = vec![first];
            while reqs.len() < MAX_BATCH {
                match q.try_recv() {
                    Ok(r) => reqs.push(r),
                    Err(_) => break,
                }
            }
            let wait = match mode {
                Mode::Free => None,
                Mode::Window(d) | Mode::Lockstep(d) => Some(Instant::now() + d),
            };
            if let Some(until) = wait {
                while reqs.len() < MAX_BATCH && !(matches!(mode, Mode::Lockstep(_)) && reqs.len() >= live.count.load(Ordering::Acquire)) {
                    let left = until.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        break;
                    }
                    match q.recv_timeout(left) {
                        Ok(r) => reqs.push(r),
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
            }
            let t0 = super::super::now_ns();
            let jobs: Vec<&Job> = reqs.iter().map(|r| &*r.job).collect();
            let outs = pollster::block_on(gpu.run(&jobs));
            let st = &mut gpu.stats;
            st.queue_ns += reqs.iter().map(|r| t0.saturating_sub(r.sent)).sum::<u64>();
            st.sizes[(usize::BITS - (reqs.len() - 1).leading_zeros()) as usize] += 1;
            for (r, o) in reqs.into_iter().zip(outs) {
                let _ = r.reply.send(o);
            }
            let t1 = super::super::now_ns();
            st.busy_ns += t1 - t0;
            if st.first_ns == 0 {
                st.first_ns = t0;
            }
            st.last_ns = t1;
            *stats.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = gpu.stats;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One kernel headless (the GEMM with a row map, bias, ReLU², accumulate)
    /// against the CPU; skipped where there is no adapter.
    #[test]
    fn gemm_kernel_matches_the_cpu() {
        let Some((device, queue, pipe, layout)) = pollster::block_on(async {
            let instance = wgpu::Instance::default();
            let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions::default()).await.ok()?;
            let (device, queue) = adapter.request_device(&wgpu::DeviceDescriptor::default()).await.ok()?;
            let mut net = Net::default();
            net.cfg.d_tile = 96;
            net.cfg.d_model = 256;
            net.cfg.heads = 4;
            net.combat.att.out = 128;
            net.combat.l2.out = 64;
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(Cow::Owned(shader(&net, false))) });
            let storage = |binding: u32, read_only: bool| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only }, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            };
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &[
                    storage(0, false),
                    storage(1, true),
                    storage(2, true),
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: true, min_binding_size: wgpu::BufferSize::new(128) },
                        count: None,
                    },
                ],
            });
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
            let pipe = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: None,
                layout: Some(&pl),
                module: &module,
                entry_point: Some("gemm"),
                compilation_options: compile_options(),
                cache: None,
            });
            Some((device, queue, pipe, layout))
        }) else {
            eprintln!("no wgpu adapter: skipped");
            return;
        };
        let (m, k, n) = (70usize, 37usize, 75usize);
        let x: Vec<f32> = (0..m * k).map(|i| ((i * 7 % 11) as f32 - 5.0) * 0.25).collect();
        let wt: Vec<f32> = (0..k * n).map(|i| ((i * 5 % 13) as f32 - 6.0) * 0.125).collect();
        let bias: Vec<f32> = (0..n).map(|i| i as f32 * 0.01 - 0.3).collect();
        let prior: Vec<f32> = (0..m * n).map(|i| (i % 3) as f32).collect();
        let rows: Vec<u32> = (0..m as u32).rev().collect();
        // Arena: x, then out (prior values; accumulate).
        let mut arena = x.clone();
        let out_off = arena.len() as u32;
        arena.extend_from_slice(&prior);
        let mut wv = wt.clone();
        let b_off = wv.len() as u32;
        wv.extend_from_slice(&bias);
        let mk = |data: &[u8], usage: wgpu::BufferUsages| {
            let b = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (data.len() as u64).next_multiple_of(256), usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            queue.write_buffer(&b, 0, data);
            b
        };
        let a_buf = mk(bytes(&arena), wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC);
        let w_buf = mk(bytes(&wv), wgpu::BufferUsages::STORAGE);
        let i_buf = mk(bytes_u32(&rows), wgpu::BufferUsages::STORAGE);
        let mut p = vec![0u32; PARAM_STRIDE];
        p[..14].copy_from_slice(&[m as u32, k as u32, n as u32, 0, k as u32, 0, b_off, out_off, n as u32, NONE, 0, NONE, 2, 1]);
        let p_buf = mk(bytes_u32(&p), wgpu::BufferUsages::UNIFORM);
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: a_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: w_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: i_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &p_buf, offset: 0, size: wgpu::BufferSize::new(128) }) },
            ],
        });
        let rb = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (m * n * 4) as u64, usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let mut enc = device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipe);
            pass.set_bind_group(0, &bind, &[0]);
            pass.dispatch_workgroups((n as u32).div_ceil(64), (m as u32).div_ceil(32), 1);
        }
        enc.copy_buffer_to_buffer(&a_buf, u64::from(out_off) * 4, &rb, 0, (m * n * 4) as u64);
        queue.submit([enc.finish()]);
        rb.map_async(wgpu::MapMode::Read, .., |r| r.expect("mapped"));
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        let got: Vec<f32> = rb.get_mapped_range(..).expect("mapped").chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        for r in 0..m {
            let o = rows[r] as usize;
            for c in 0..n {
                let s = bias[c] + (0..k).map(|i| x[r * k + i] * wt[i * n + c]).sum::<f32>();
                let want = prior[o * n + c] + s.max(0.0) * s.max(0.0);
                assert!((got[o * n + c] - want).abs() <= 1e-3 * want.abs().max(1.0), "{r} {c}: {} {want}", got[o * n + c]);
            }
        }
    }
}

#[cfg(test)]
mod place_tests {
    use super::*;

    struct H {
        device: wgpu::Device,
        queue: wgpu::Queue,
        layout: wgpu::BindGroupLayout,
        module: wgpu::ShaderModule,
        mma: bool,
    }

    fn harness() -> Option<H> {
        pollster::block_on(async {
            let (device, queue, mma) = test_device().await?;
            let mut net = Net::default();
            net.cfg.d_tile = 96;
            net.cfg.d_model = 256;
            net.cfg.heads = 4;
            net.combat.att.out = 128;
            net.combat.l2.out = 64;
            let module = make_module(&device, &net, mma);
            let storage = |binding: u32, read_only: bool| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only }, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            };
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &[
                    storage(0, false),
                    storage(1, true),
                    storage(2, true),
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: true, min_binding_size: wgpu::BufferSize::new(128) },
                        count: None,
                    },
                ],
            });
            Some(H { device, queue, layout, module, mma })
        })
    }

    fn run(h: &H, entry: &str, groups: (u32, u32), p: &[u32], arena: &[f32], weights: &[f32], index: &[u32]) -> Vec<f32> {
        let pl = h.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&h.layout)], immediate_size: 0 });
        let pipe = h.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: Some(&pl),
            module: &h.module,
            entry_point: Some(entry),
            compilation_options: compile_options(),
            cache: None,
        });
        let mk = |data: &[u8], usage: wgpu::BufferUsages| {
            let b = h.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (data.len().max(4) as u64).next_multiple_of(256), usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            h.queue.write_buffer(&b, 0, data);
            b
        };
        // Padded as `Gpu` pads them (`gemm_mmad` reads past a tile's last row and column).
        let mut ap = arena.to_vec();
        ap.extend(vec![0.0; PAD_A]);
        let mut wp = weights.to_vec();
        wp.extend([0.0; PAD_W]);
        let a = mk(bytes(&ap), wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC);
        let w = mk(bytes(&wp), wgpu::BufferUsages::STORAGE);
        let i = mk(bytes_u32(index), wgpu::BufferUsages::STORAGE);
        let mut pp = vec![0u32; PARAM_STRIDE];
        pp[..p.len()].copy_from_slice(p);
        let pb = mk(bytes_u32(&pp), wgpu::BufferUsages::UNIFORM);
        let bind = h.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &h.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: a.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: w.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: i.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &pb, offset: 0, size: wgpu::BufferSize::new(128) }) },
            ],
        });
        let size = (arena.len() * 4) as u64;
        let rb = h.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: size.next_multiple_of(256), usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let mut enc = h.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipe);
            pass.set_bind_group(0, &bind, &[0]);
            pass.dispatch_workgroups(groups.0, groups.1, 1);
        }
        enc.copy_buffer_to_buffer(&a, 0, &rb, 0, size);
        h.queue.submit([enc.finish()]);
        rb.map_async(wgpu::MapMode::Read, .., |r| r.expect("mapped"));
        let _ = h.device.poll(wgpu::PollType::wait_indefinitely());
        let v: Vec<f32> = rb.get_mapped_range(..).expect("mapped").chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        v[..arena.len()].to_vec()
    }

    fn noise(n: usize, seed: u32) -> Vec<f32> {
        let mut s = seed.wrapping_mul(2654435761).wrapping_add(12345);
        (0..n)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                (s % 2001) as f32 / 1000.0 - 1.0
            })
            .collect()
    }

    /// A row's result does not depend on its place in the batch: the same
    /// rows at row 0 and at row 37, for the GEMM with RMSNorm and QK-norm.
    #[test]
    fn gemm_rows_do_not_depend_on_their_place() {
        let Some(h) = harness() else { return };
        let (rows, pre, k, n) = (100usize, 37usize, 256usize, 768usize);
        let x = noise((rows + pre) * k, 1);
        let mut weights = noise(k * n, 2);
        let b_off = weights.len() as u32;
        weights.extend(noise(n, 3));
        let qn = weights.len() as u32;
        weights.extend(noise(64, 4));
        let kn = weights.len() as u32;
        weights.extend(noise(64, 5));
        let mut fails = 0;
        let entries: &[&str] = if h.mma { &["gemm", "gemm_mma", "gemm_mmad"] } else { &["gemm"] };
        for (entry, rms, hn) in entries.iter().flat_map(|e| [(*e, 0u32, false), (*e, 1, false), (*e, 0, true), (*e, 1, true)]) {
            let mut arena = x.clone();
            let out = arena.len() as u32;
            arena.extend(vec![0.0; (rows + pre) * n]);
            let (q, kk) = if hn { (qn, kn) } else { (NONE, NONE) };
            let p = |m: usize, x_off: usize| [m as u32, k as u32, n as u32, x_off as u32, k as u32, 0, b_off, out, n as u32, NONE, NONE, NONE, 0, 0, rms, q, kk, 256];
            let g = |m: usize| ((n as u32).div_ceil(64), (m as u32).div_ceil(32));
            let a = run(&h, entry, g(rows), &p(rows, pre * k), &arena, &weights, &[0]);
            let b = run(&h, entry, g(rows + pre), &p(rows + pre, 0), &arena, &weights, &[0]);
            let o = out as usize;
            let mut diff = 0;
            let mut cols = std::collections::BTreeSet::new();
            let mut maxd = 0.0f32;
            for r in 0..rows {
                if a[o + r * n..o + (r + 1) * n] != b[o + (r + pre) * n..o + (r + pre + 1) * n] {
                    diff += 1;
                    for c in 0..n {
                        let (u, v) = (a[o + r * n + c], b[o + (r + pre) * n + c]);
                        if u != v {
                            cols.insert(c / 64);
                            maxd = maxd.max((u - v).abs());
                        }
                    }
                }
            }
            eprintln!("{entry} rms {rms} head-norm {hn}: {diff} of {rows} rows differ by their place; column tiles {cols:?}; max {maxd:e}");
            fails += diff;
        }
        assert_eq!(fails, 0);
    }

    /// Both GEMM kernels against the CPU (a row map, bias, ReLU², accumulate; K not a multiple of 8).
    #[test]
    fn gemm_kernels_match_the_cpu() {
        let Some(h) = harness() else { return };
        let (m, k, n) = (70usize, 37usize, 75usize);
        let x = noise(m * k, 11);
        let wt = noise(k * n, 12);
        let bias = noise(n, 13);
        let prior = noise(m * n, 14);
        let rows: Vec<u32> = (0..m as u32).rev().collect();
        let mut arena = x.clone();
        let out = arena.len();
        arena.extend_from_slice(&prior);
        let mut w = wt.clone();
        let b_off = w.len() as u32;
        w.extend_from_slice(&bias);
        let entries: &[&str] = if h.mma { &["gemm", "gemm_mma"] } else { &["gemm"] };
        for entry in entries {
            let p = [m as u32, k as u32, n as u32, 0, k as u32, 0, b_off, out as u32, n as u32, NONE, 0, NONE, 2, 1];
            let got = run(&h, entry, ((n as u32).div_ceil(64), (m as u32).div_ceil(32)), &p, &arena, &w, &rows);
            for r in 0..m {
                let o = rows[r] as usize;
                for c in 0..n {
                    let s = bias[c] + (0..k).map(|i| x[r * k + i] * wt[i * n + c]).sum::<f32>();
                    let want = prior[o * n + c] + s.max(0.0) * s.max(0.0);
                    let g = got[out + o * n + c];
                    assert!((g - want).abs() <= 1e-4 * want.abs().max(1.0), "{entry} {r} {c}: {g} {want}");
                }
            }
        }
    }

    /// `gemm_mmad` (K a multiple of 8, no row map) against the CPU, with the RMSNorm and a head norm.
    #[test]
    fn gemm_direct_matches_the_cpu() {
        let Some(h) = harness() else { return };
        if !h.mma {
            return;
        }
        let (m, k, n) = (45usize, 40usize, 128usize);
        let x = noise(m * k, 21);
        let wt = noise(k * n, 22);
        let bias = noise(n, 23);
        let mut w = wt.clone();
        let b_off = w.len() as u32;
        w.extend_from_slice(&bias);
        let qn = w.len() as u32;
        w.extend(noise(64, 24));
        let mut arena = x.clone();
        let out = arena.len();
        arena.extend(vec![0.0; m * n]);
        let p = [m as u32, k as u32, n as u32, 0, k as u32, 0, b_off, out as u32, n as u32, NONE, NONE, NONE, 1, 0, 1, qn, NONE, 64];
        let g = ((n as u32).div_ceil(64), (m as u32).div_ceil(32));
        let a = run(&h, "gemm_mmad", g, &p, &arena, &w, &[0]);
        let b = run(&h, "gemm", g, &p, &arena, &w, &[0]);
        let md = a[out..].iter().zip(&b[out..]).map(|(u, v)| (u - v).abs()).fold(0.0f32, f32::max);
        eprintln!("gemm_mmad vs gemm: max diff {md:e}");
        assert!(md < 1e-4, "{md}");
        // And the plain product against the CPU.
        let p = [m as u32, k as u32, n as u32, 0, k as u32, 0, b_off, out as u32, n as u32, NONE, NONE, NONE, 0, 0, 0, NONE, NONE, 0];
        let a = run(&h, "gemm_mmad", g, &p, &arena, &w, &[0]);
        for r in 0..m {
            for c in 0..n {
                let want = bias[c] + (0..k).map(|i| x[r * k + i] * wt[i * n + c]).sum::<f32>();
                assert!((a[out + r * n + c] - want).abs() <= 1e-4 * want.abs().max(1.0), "{r} {c}");
            }
        }
    }

    /// The attention of a block does not depend on where its rows are, for
    /// both kernels, and `attn_mma` agrees with `attn` (300 tokens: two chunks of keys).
    #[test]
    fn attention_does_not_depend_on_its_place() {
        let Some(h) = harness() else { return };
        let mut outs = Vec::new();
        let entries: &[&str] = if h.mma { &["attn", "attn_mma"] } else { &["attn"] };
        for &(tok, pre) in &[(147usize, 41usize), (300, 13)] {
            let d = 256usize;
            let n = tok + pre;
            let qkv = noise(n * 3 * d, 7);
            let mut arena = qkv.clone();
            let o_off = arena.len();
            arena.extend(vec![0.0; n * d]);
            let pos_off = arena.len();
            arena.extend((0..n).flat_map(|i| [(i % 13) as f32, (i % 7) as f32 + 0.5 * (i % 2) as f32, 1.0, 0.0]));
            let weights = noise(4 * 14, 9);
            let blocks = |start: usize| -> Vec<u32> {
                let mut v = Vec::new();
                let mut q = 0;
                while q < tok {
                    let c = 16.min(tok - q);
                    v.extend([(start + q) as u32, c as u32, start as u32, tok as u32]);
                    q += c;
                }
                v
            };
            let nb = tok.div_ceil(16) as u32;
            let scale = (1.0f32 / 8.0).to_bits();
            let p = [nb, 0, 3 * d as u32, d as u32, 2 * d as u32, 3 * d as u32, o_off as u32, d as u32, 0, 1, pos_off as u32, 0, scale, 4, NONE, NONE];
            // The same token rows' content at both places.
            let mut arena2 = arena.clone();
            for r in 0..tok {
                arena2[(pre + r) * 3 * d..(pre + r + 1) * 3 * d].copy_from_slice(&qkv[r * 3 * d..(r + 1) * 3 * d]);
                for c in 0..4 {
                    arena2[pos_off + (pre + r) * 4 + c] = arena[pos_off + r * 4 + c];
                }
            }
            for entry in entries {
                let g = (nb * 4 * if *entry == "attn_mma" { 2 } else { 1 }, 1);
                let a = run(&h, entry, g, &p, &arena, &weights, &blocks(0));
                let b = run(&h, entry, g, &p, &arena2, &weights, &blocks(pre));
                let mut diff = 0;
                for r in 0..tok {
                    if a[o_off + r * d..o_off + (r + 1) * d] != b[o_off + (pre + r) * d..o_off + (pre + r + 1) * d] {
                        diff += 1;
                    }
                }
                assert_eq!(diff, 0, "{entry}: {diff} of {tok} rows differ by their place");
                outs.push((tok, entry, a[o_off..o_off + tok * d].to_vec()));
            }
        }
        for w in outs.windows(2) {
            if w[0].0 == w[1].0 {
                let md = w[0].2.iter().zip(&w[1].2).map(|(x, y)| (x - y).abs()).fold(0.0f32, f32::max);
                eprintln!("{} tokens: {} vs {}: max diff {md:e}", w[0].0, w[0].1, w[1].1);
                assert!(md < 1e-4, "{md}");
            }
        }
    }
}

#[cfg(test)]
mod speed_tests {
    use super::*;

    /// `cargo test --release --features gpu speed_ -- --ignored --nocapture` (`SPEED_ONLY=gemm` or `attn`):
    /// the GEMM and attention kernels at the trunk's shapes, 200 dispatches in one pass, microseconds each.
    #[test]
    #[ignore]
    fn speed_gemm() {
        let (device, queue, layout, module, mma) = pollster::block_on(async {
            let (device, queue, mma) = test_device().await.expect("a device");
            let mut net = Net::default();
            net.cfg.d_tile = 96;
            net.cfg.d_model = 256;
            net.cfg.heads = 4;
            net.combat.att.out = 128;
            net.combat.l2.out = 64;
            let module = make_module(&device, &net, mma);
            let storage = |binding: u32, read_only: bool| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only }, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            };
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &[storage(0, false), storage(1, true), storage(2, true), wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: true, min_binding_size: wgpu::BufferSize::new(128) },
                    count: None,
                }],
            });
            (device, queue, layout, module, mma)
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let entries: &[&str] = if mma { &["gemm", "gemm_mma", "gemm_mmad"] } else { &["gemm"] };
        let mk = |size: u64, usage: wgpu::BufferUsages| device.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let a = mk(64 << 20, wgpu::BufferUsages::STORAGE);
        let w = mk(32 << 20, wgpu::BufferUsages::STORAGE);
        let i = mk(1 << 16, wgpu::BufferUsages::STORAGE);
        let pb = mk(1 << 16, wgpu::BufferUsages::UNIFORM);
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: a.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: w.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: i.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &pb, offset: 0, size: wgpu::BufferSize::new(128) }) },
            ],
        });
        let time = |entry: &str, p: &[u32], groups: (u32, u32)| {
            let pipe = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: None, layout: Some(&pl), module: &module, entry_point: Some(entry), compilation_options: compile_options(), cache: None });
            let mut pp = vec![0u32; PARAM_STRIDE];
            pp[..p.len()].copy_from_slice(p);
            queue.write_buffer(&pb, 0, bytes_u32(&pp));
            let mut us = 0.0;
            for reps in [20u32, 200] {
                let t = std::time::Instant::now();
                let mut enc = device.create_command_encoder(&Default::default());
                {
                    let mut pass = enc.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&pipe);
                    for _ in 0..reps {
                        pass.set_bind_group(0, &bind, &[0]);
                        pass.dispatch_workgroups(groups.0, groups.1, 1);
                    }
                }
                queue.submit([enc.finish()]);
                let _ = device.poll(wgpu::PollType::wait_indefinitely());
                us = t.elapsed().as_secs_f64() * 1e6 / f64::from(reps);
            }
            us
        };
        let only = std::env::var("SPEED_ONLY").unwrap_or_default();
        if only.is_empty() || only == "gemm" {
            for &(m, k, n) in &[(193u32, 256u32, 768u32), (193, 256, 256), (193, 512, 256), (2016, 96, 192), (2016, 192, 96), (1, 256, 128), (14, 256, 512)] {
                let p = [m, k, n, 0, k, 0, NONE, 4 << 20, n, NONE, NONE, NONE, 0, 0, 0, NONE, NONE, 0];
                for entry in entries {
                    eprintln!("{entry} m{m} k{k} n{n}: {:.1} us", time(entry, &p, (n.div_ceil(64), m.div_ceil(32))));
                }
            }
        }
        if only.is_empty() || only == "attn" {
            // The self-attention of 196 tokens, 4 heads, the distance bias: blocks of 16 queries.
            let tok = 196u32;
            let mut blocks = Vec::new();
            let mut q = 0;
            while q < tok {
                let c = 16.min(tok - q);
                blocks.extend([q, c, 0, tok]);
                q += c;
            }
            queue.write_buffer(&i, 0, bytes_u32(&blocks));
            let pos: Vec<f32> = (0..tok).flat_map(|r| [(r % 13) as f32, (r % 7) as f32, 1.0, 0.0]).collect();
            let pos_off = 8u32 << 20;
            queue.write_buffer(&a, u64::from(pos_off) * 4, bytes(&pos));
            let nb = blocks.len() as u32 / 4;
            let p = [nb, 0, 768, 256, 512, 768, 4 << 20, 256, 0, 1, pos_off, 0, (0.125f32).to_bits(), 4, NONE, NONE];
            eprintln!("attn tokens {tok}: {:.1} us", time("attn", &p, (nb * 4, 1)));
            if mma {
                eprintln!("attn_mma tokens {tok}: {:.1} us", time("attn_mma", &p, (nb * 8, 1)));
            }
        }
    }
}

/// `cargo test --release --features gpu dump_msl -- --ignored --nocapture`: the
/// kernels (with the cooperative-matrix ones) validated by naga and written
/// out as Metal source, as wgpu would compile them.
#[cfg(test)]
mod msl_dump {
    use super::*;
    #[test]
    #[ignore]
    fn dump_msl() {
        use wgpu::naga;
        let mut net = Net::default();
        net.cfg.d_tile = 96;
        net.cfg.d_model = 256;
        net.cfg.heads = 4;
        net.combat.att.out = 128;
        net.combat.l2.out = 64;
        let module = naga::front::wgsl::parse_str(&shader(&net, true)).unwrap();
        let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&module).unwrap();
        let mut opts = naga::back::msl::Options::default();
        opts.lang_version = (3, 0);
        opts.bounds_check_policies = naga::proc::BoundsCheckPolicies::default();
        opts.force_loop_bounding = false;
        let (s, _) = naga::back::msl::write_string(&module, &info, &opts, &naga::back::msl::PipelineOptions::default()).unwrap();
        let path = std::env::temp_dir().join("v2-trunk.metal");
        std::fs::write(&path, s).unwrap();
        eprintln!("{}", path.display());
    }
}
