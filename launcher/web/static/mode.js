// Which mode the page runs in, decided before anything else loads (classic script, no wasm).
// Result: window.aureliaMode = { mode: "mobile" | "desktop", source: "query" | "stored" | "auto",
// why: [...signals], switchTo(mode), url(mode) } and <html data-mode="...">.
//
// Order: ?mode=mobile|desktop (this visit only) > the choice the player made with the switch
// link (localStorage "aurelia.mode") > detection. ?mode=auto forgets the stored choice.
// There is no redirect: the same page and the same files serve both modes, so there is
// no loop to fall into and no extra round trip; the URL only gets ?mode= when somebody
// chooses.
(() => {
  const nav = navigator, win = window;
  const KEY = "aurelia.mode";
  const read = () => { try { return win.localStorage.getItem(KEY); } catch (_) { return null; } };
  const write = (value) => {
    try { value ? win.localStorage.setItem(KEY, value) : win.localStorage.removeItem(KEY); } catch (_) { /* private mode, sandboxed frame */ }
  };
  const media = (query) => { try { return win.matchMedia(query).matches; } catch (_) { return false; } };
  const valid = (v) => (v === "mobile" || v === "desktop" ? v : null);

  const params = new URLSearchParams(win.location.search);
  const asked = params.get("mode");
  if (asked === "auto") write(null);

  // The signals. A phone or a tablet: the main pointer is a finger (coarse, no hover), or the
  // browser says "mobile"; a laptop with a touch screen has a mouse as its main pointer, so it is
  // only taken for a phone when its window is phone-sized too.
  const uaData = nav.userAgentData;
  const ua = nav.userAgent || "";
  const uaMobile = uaData && typeof uaData.mobile === "boolean"
    ? uaData.mobile
    : /Android|iPhone|iPod|Mobile|Silk/i.test(ua);
  const ipad = /iPad/.test(ua) || (/Macintosh/.test(ua) && nav.maxTouchPoints > 1);
  const touchMain = media("(pointer: coarse)") && media("(hover: none)");
  const anyTouch = nav.maxTouchPoints > 0 || media("(any-pointer: coarse)");
  const shortSide = Math.min(win.innerWidth, win.innerHeight);
  const longSide = Math.max(win.innerWidth, win.innerHeight);
  const small = shortSide < 500 || longSide < 900;
  const why = [];
  if (uaMobile) why.push("ua-mobile");
  if (ipad) why.push("ipad");
  if (touchMain) why.push("touch-main-pointer");
  if (anyTouch && small) why.push("touch+small-viewport");
  const detected = why.length > 0 ? "mobile" : "desktop";

  let mode = valid(asked), source = "query";
  if (!mode) { mode = valid(read()); source = "stored"; }
  if (!mode) { mode = detected; source = "auto"; }

  const url = (m) => {
    const next = new URL(win.location.href);
    next.searchParams.set("mode", m);
    return next.toString();
  };
  win.aureliaMode = {
    mode, source, why, detected,
    // The player's choice: remembered, and the page reloads in the other mode.
    switchTo(target) {
      if (!valid(target)) return;
      write(target);
      win.location.assign(url(target));
    },
    url,
  };
  const root = document.documentElement;
  root.dataset.mode = mode;
  root.dataset.modeSource = source;
  if (mode === "mobile") {
    const meta = document.getElementById("viewport");
    if (meta) {
      meta.setAttribute("content",
        "width=device-width, initial-scale=1, minimum-scale=1, maximum-scale=1, user-scalable=no, viewport-fit=cover, interactive-widget=resizes-content");
    }
  }
})();
