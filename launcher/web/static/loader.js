// The page's loader (docs/web.md, "Loading"). sizes.json (written by web-build.sh, never cached) names the
// content-hashed files and their sizes: the wasm, its JS glue, the baked 3D models (and the fog engravings, which
// are not part of the start: `fog()` fetches them in the background once the menu is up).
// Every download is a job that counts its bytes into one progress number; a job that failed runs again on the
// next call (the page's retry button), a job that finished is not repeated. The wasm streams through a counter
// into WebAssembly.instantiateStreaming, so it compiles while it downloads.

export class LoadError extends Error {}

export function createLoader(onProgress) {
  const jobs = new Map(); // name -> { loaded, failed, promise }
  let total = 1;
  const report = () => {
    let loaded = 0;
    for (const j of jobs.values()) loaded += j.loaded;
    onProgress(Math.min(1, loaded / total), loaded, total);
  };
  // Runs `work(job)` once; a failure is remembered as a LoadError and the job starts over when asked again.
  const job = (name, work) => {
    const old = jobs.get(name);
    if (old && !old.failed) return old.promise;
    const j = { loaded: 0, failed: false };
    jobs.set(name, j);
    j.promise = (async () => work(j))().catch((error) => {
      j.failed = true; j.loaded = 0; report();
      throw error instanceof LoadError ? error : new LoadError(`${name}: ${error && error.message ? error.message : error}`);
    });
    return j.promise;
  };
  const open = async (url, options) => {
    let response;
    try { response = await fetch(url, options); } catch (error) { throw new LoadError(`${url}: ${error.message || "network error"}`); }
    if (!response.ok) throw new LoadError(`${url}: HTTP ${response.status}`);
    return response;
  };
  // The body of a response with every chunk counted into the job.
  const counted = (response, j) => response.body.pipeThrough(new TransformStream({
    transform(chunk, controller) { j.loaded += chunk.length; report(); controller.enqueue(chunk); },
  }));
  // `gzip`: the file is a .gz the page inflates itself (a plain file, no Content-Encoding); the bar counts the bytes that travel.
  const bytes = (name, url, size, gzip = false) => job(name, async (j) => {
    const response = await open(url);
    const out = new Uint8Array(size || Number(response.headers.get("Content-Length")) || 0);
    let used = 0, grown = out;
    const reader = counted(response, j).getReader();
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      if (used + value.length > grown.length) { const next = new Uint8Array(Math.max(grown.length * 2, used + value.length)); next.set(grown.subarray(0, used)); grown = next; }
      grown.set(value, used); used += value.length;
    }
    j.loaded = size || used; report();
    const packed = used === grown.length ? grown : grown.slice(0, used);
    // The gzip magic bytes decide: a server that already decoded the file on the way leaves plain bytes.
    if (!gzip || packed[0] !== 0x1f || packed[1] !== 0x8b) return packed;
    if (typeof DecompressionStream === "undefined") throw new LoadError(`${url}: this browser cannot inflate gzip (DecompressionStream)`);
    return new Uint8Array(await new Response(new Blob([packed]).stream().pipeThrough(new DecompressionStream("gzip"))).arrayBuffer());
  });

  // The bytes handed over to the wasm are not kept in the page.
  const release = (...names) => { for (const n of names) { const j = jobs.get(n); if (j) j.promise = Promise.resolve(null); } };
  const state = { sizes: null, applied: { models: false } };
  return {
    bytes,
    // sizes.json: the list of files. `extra` is what the page fetches itself in the same bar (bytes).
    async sizes(extra = 0) {
      if (!state.sizes) {
        const response = await open("sizes.json", { cache: "no-cache" });
        state.sizes = await response.json();
      }
      const s = state.sizes;
      total = s.wasm.bytes + s.glue.bytes + s.models.units.bytes + s.models.world.bytes + (s.boot ? s.boot.bytes : 0) + extra;
      return s;
    },
    // The engine: glue, wasm (compiled while it downloads) and models handed over. Returns the glue's API.
    async engine(sizes) {
      const glue = await job("glue", async (j) => { const m = await import(`./${sizes.glue.file}`); j.loaded = sizes.glue.bytes; report(); return m; });
      const [instance, units, world] = await Promise.all([
        job("wasm", async (j) => {
          const response = await open(sizes.wasm.file);
          const stream = new Response(counted(response, j), { headers: { "Content-Type": "application/wasm" } });
          const instance = await glue.default({ module_or_path: stream });
          j.loaded = sizes.wasm.bytes; report();
          return instance;
        }),
        bytes("units", sizes.models.units.file, sizes.models.units.bytes, sizes.models.units.encoding === "gzip"),
        bytes("world", sizes.models.world.file, sizes.models.world.bytes, sizes.models.world.encoding === "gzip"),
      ]);
      // Each is called exactly once, however many times the page retries.
      if (!state.applied.models) {
        const words = (b) => new Uint32Array(b.buffer, b.byteOffset, b.byteLength >> 2);
        glue.set_models(words(units), words(world));
        state.applied.models = true; release("units", "world");
      }
      return { api: glue, wasm: instance };
    },
    // The fog engravings (6.7 MB), in the background after the menu is up: not in the bar, not required. The game
    // draws the plain fog until they arrive (client::shell::hud::fog_ornaments::provide). A failed download is
    // tried again a few times, then given up with a console warning (the game is fine without them).
    async fog(api, sizes) {
      for (let attempt = 0; attempt < 4; attempt++) {
        try {
          const files = await Promise.all(["serpent", "kraken", "compass"].map(async (name) => {
            const response = await open(sizes.fog[name].file);
            return new Uint8Array(await response.arrayBuffer());
          }));
          api.set_fog_ornaments(...files);
          return true;
        } catch (error) {
          console.warn("fog engravings:", error);
          await new Promise((resolve) => setTimeout(resolve, 2000 * (attempt + 1)));
        }
      }
      return false;
    },
  };
}
