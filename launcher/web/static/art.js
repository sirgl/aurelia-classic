// The page's art service (docs/web.md, "Picture loading"). The game lists the pictures its
// screen shows, most urgent first (`take_picture_requests`); this module fetches the files with a
// bounded number of requests, decodes them with the browser's own decoder (`createImageBitmap`, off
// the main thread), cuts the atlas region and scales to the wanted size on an OffscreenCanvas, and
// hands RGBA pixels back (`register_picture`). Requests that leave the list before they start are
// dropped, and a fetch nobody wants any more is aborted.
import { mark } from "./trace.js";

const params = new URLSearchParams(location.search);
// Files fetched at once. HTTP/1.1 allows 6 per origin; more would only queue in the browser.
const FETCH_MAX = Number(params.get("fetch")) || 6;
// Decoded sheets kept for the regions still to cut.
const BITMAPS_KEPT = 10;
// Main-thread time one slice of rendering may take before it yields to the frame.
const SLICE_MS = 6;

export function createArt(base, suffix = "") {
  const files = new Map();   // path -> { promise, controller, done, wanted }
  const bitmaps = new Map(); // path -> ImageBitmap | null, in use order
  let queue = [];            // requests not started, most urgent first
  let running = 0;
  const started = new Set(); // keys started and not yet delivered
  const ready = [];          // [request, bitmap] waiting for the render slice
  let sliceScheduled = false;
  let api = null;
  const inFlightPaths = new Map(); // key -> path of the requests started

  const fetchBitmap = (path) => {
    let entry = files.get(path);
    if (entry) return entry;
    entry = { controller: new AbortController(), done: false, aborted: false };
    entry.promise = (async () => {
      mark("fetch", path);
      const response = await fetch(`${base}${path}${suffix}`, { signal: entry.controller.signal });
      if (!response.ok) throw new Error(`${path}: HTTP ${response.status}`);
      const blob = await response.blob();
      mark("fetched", path, String(blob.size));
      entry.done = true;
      const t = performance.now();
      const bitmap = await createImageBitmap(blob, { premultiplyAlpha: "none", colorSpaceConversion: "none" });
      mark("decoded", path, `${(performance.now() - t).toFixed(1)} ms ${bitmap.width}x${bitmap.height}`);
      return bitmap;
    })();
    files.set(path, entry);
    entry.promise.catch(() => { if (files.get(path) === entry) files.delete(path); });
    return entry;
  };

  const bitmapOf = async (path) => {
    if (bitmaps.has(path)) {
      const b = bitmaps.get(path);
      bitmaps.delete(path); bitmaps.set(path, b);
      return b;
    }
    const bitmap = await fetchBitmap(path).promise;
    bitmaps.set(path, bitmap);
    while (bitmaps.size > BITMAPS_KEPT) bitmaps.delete(bitmaps.keys().next().value);
    // The file entry is not needed once its bitmap is kept (or evicted: fetched again if asked).
    files.delete(path);
    return bitmap;
  };

  // [key, path, x, y, w, h, maxSide, priority] -> request
  const asRequest = (row) => ({ key: row[0], path: row[1], rect: row[2] < 0 ? null : row.slice(2, 6), maxSide: row[6], priority: row[7] });

  const render = (request, bitmap) => {
    const [sx, sy, sw, sh] = request.rect || [0, 0, bitmap.width, bitmap.height];
    if (sx + sw > bitmap.width || sy + sh > bitmap.height || sw <= 0 || sh <= 0) return null;
    let w = sw, h = sh;
    if (request.maxSide > 0) {
      // As `image::DynamicImage::thumbnail`: fit the longest side to the bound, up or down.
      const scale = request.maxSide / Math.max(sw, sh);
      w = Math.max(1, Math.round(sw * scale));
      h = Math.max(1, Math.round(sh * scale));
    }
    const canvas = new OffscreenCanvas(w, h);
    const ctx = canvas.getContext("2d", { willReadFrequently: true });
    ctx.imageSmoothingEnabled = true;
    ctx.imageSmoothingQuality = "high";
    ctx.drawImage(bitmap, sx, sy, sw, sh, 0, 0, w, h);
    return { w, h, data: ctx.getImageData(0, 0, w, h).data };
  };

  const slice = () => {
    sliceScheduled = false;
    const t0 = performance.now();
    while (ready.length && performance.now() - t0 < SLICE_MS) {
      const [request, bitmap] = ready.shift();
      const t = performance.now();
      const out = render(request, bitmap);
      started.delete(request.key);
      inFlightPaths.delete(request.key);
      if (out) {
        mark("rendered", request.key, `${(performance.now() - t).toFixed(1)} ms ${out.w}x${out.h}`);
        api.register(request.key, out.w, out.h, new Uint8Array(out.data.buffer, out.data.byteOffset, out.data.byteLength));
      } else {
        api.fail(request.key);
      }
    }
    if (ready.length) schedule();
  };
  const schedule = () => {
    if (!sliceScheduled) { sliceScheduled = true; setTimeout(slice, 0); }
  };

  const start = (request) => {
    running += 1;
    started.add(request.key);
    inFlightPaths.set(request.key, request.path);
    bitmapOf(request.path).then(
      (bitmap) => { running -= 1; ready.push([request, bitmap]); schedule(); pump(); },
      (error) => {
        running -= 1;
        started.delete(request.key);
        inFlightPaths.delete(request.key);
        if (error && error.name === "AbortError") { pump(); return; }
        console.warn(error);
        if (api) api.fail(request.key);
        pump();
      },
    );
  };

  const pump = () => {
    // Slots are files in flight: requests for a file already being fetched or decoded cost nothing.
    while (queue.length && running < FETCH_MAX * 2) {
      const request = queue.shift();
      if (started.has(request.key)) continue;
      start(request);
    }
  };

  return {
    // Start fetching a file before the game asks for it (the menu cover, while the wasm loads).
    prefetch(path) {
      const entry = fetchBitmap(path);
      entry.pinned = true;
      bitmapOf(path).catch(() => {});
    },
    bind(functions) { api = functions; },
    // The game's current list: replaces the queue, aborts fetches nobody wants.
    update(rows) {
      queue = rows.map(asRequest).filter((r) => !started.has(r.key));
      const wanted = new Set(queue.map((r) => r.path));
      for (const path of inFlightPaths.values()) wanted.add(path);
      for (const [path, entry] of files) {
        if (!entry.done && !entry.pinned && !wanted.has(path)) {
          entry.controller.abort();
          files.delete(path);
          mark("abort", path);
        }
      }
      pump();
    },
    stats: () => ({ queued: queue.length, running, ready: ready.length, files: files.size, bitmaps: bitmaps.size }),
  };
}
