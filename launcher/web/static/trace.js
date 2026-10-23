// Timing trace of the picture pipeline (docs/web.md, "Picture loading"). Off unless the page
// URL has `?trace=1`; then `window.__trace` holds [ms on performance.now(), kind, name, detail]
// rows (the page's own marks and the game's `take_trace` events) and `window.__long` the
// main-thread long tasks [start, duration] (> 50 ms) seen since the page began.
const on = new URLSearchParams(location.search).has("trace");
export const tracing = on;
export const rows = (window.__trace = []);
export const long = (window.__long = []);
if (on) {
  performance.setResourceTimingBufferSize(5000);
  try {
    new PerformanceObserver((list) => {
      for (const e of list.getEntries()) long.push([e.startTime, e.duration]);
    }).observe({ type: "longtask", buffered: true });
  } catch (_) { /* no longtask API: the table says so */ }
}
export const mark = (kind, name, detail = "") => { if (on) rows.push([performance.now(), kind, name, detail]); };
// Moves the game's events into `rows`; called by a timer.
export const drain = (take_trace) => {
  if (on) for (const row of take_trace()) rows.push(Array.from(row));
};
