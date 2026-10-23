// Mobile mode of the page (loaded only when window.aureliaMode.mode === "mobile").
// Everything a phone's browser needs that the game's own code cannot do from inside a canvas:
// no browser gestures on the canvas, the visible area (address bar, on-screen keyboard), safe-area
// insets, the on-screen keyboard for the game's text fields, fullscreen and a rotate hint.
// The touch gestures themselves (tap, long press, drag, pinch) are the shell's (`client::shell::touch`).

const doc = document;
const root = doc.documentElement;

// Keep the page from scrolling, zooming, pulling to refresh or opening menus under the finger.
function lockGestures(canvas) {
  const stop = (event) => event.preventDefault();
  for (const name of ["gesturestart", "gesturechange", "gestureend"]) {
    doc.addEventListener(name, stop, { passive: false }); // iOS Safari's own pinch, not covered by touch-action
  }
  // iOS ignores user-scalable=no and overscrolls a fixed page: touches on the canvas never scroll.
  doc.addEventListener("touchmove", (event) => {
    if (event.target === canvas || event.touches.length > 1) event.preventDefault();
  }, { passive: false });
  doc.addEventListener("contextmenu", stop);
  doc.addEventListener("dblclick", stop);
  doc.addEventListener("selectstart", stop);
}

// The area the page can use: the visual viewport (it shrinks for the on-screen keyboard on iOS,
// and for the address bar) below the optional hint bar.
function followViewport(hint) {
  const apply = () => {
    const viewport = window.visualViewport;
    const height = viewport ? viewport.height : window.innerHeight;
    const top = viewport ? viewport.offsetTop : 0;
    const bar = hint.hidden ? 0 : hint.offsetHeight;
    root.style.setProperty("--top", `${top + bar}px`);
    root.style.setProperty("--vh", `${Math.max(100, height - bar)}px`);
  };
  apply();
  const viewport = window.visualViewport;
  if (viewport) {
    viewport.addEventListener("resize", apply);
    viewport.addEventListener("scroll", apply);
  }
  window.addEventListener("resize", apply);
  window.addEventListener("orientationchange", apply);
  return apply;
}

// env(safe-area-inset-*) in CSS pixels, read through a probe element; sent to the shell.
function followSafeArea(api) {
  const probe = doc.createElement("div");
  probe.style.cssText = "position:fixed;left:0;top:0;width:0;height:0;visibility:hidden;pointer-events:none;"
    + "padding:env(safe-area-inset-top,0px) env(safe-area-inset-right,0px) env(safe-area-inset-bottom,0px) env(safe-area-inset-left,0px)";
  doc.body.append(probe);
  const send = () => {
    const style = getComputedStyle(probe);
    const px = (name) => parseFloat(style[name]) || 0;
    api.set_safe_area(px("paddingLeft"), px("paddingTop"), px("paddingRight"), px("paddingBottom"));
  };
  send();
  window.addEventListener("resize", send);
  window.addEventListener("orientationchange", send);
  screen.orientation?.addEventListener?.("change", send);
  return send;
}

// The on-screen keyboard. A canvas cannot take text, so a 1 px invisible <input> takes the focus
// while one of the game's text fields has it; its text and keys go to the shell. The element is
// focused inside the finger's lift (a browser shows the keyboard only for a user gesture) with
// inputmode "none" and switched to "text" once the shell says a field wants the keyboard.
function keyboard(api, canvas) {
  const field = doc.createElement("input");
  field.type = "text";
  field.setAttribute("inputmode", "none");
  field.setAttribute("enterkeyhint", "done");
  field.setAttribute("autocomplete", "off");
  field.setAttribute("autocorrect", "off");
  field.setAttribute("autocapitalize", "off");
  field.setAttribute("spellcheck", "false");
  field.setAttribute("aria-hidden", "true");
  field.tabIndex = -1;
  // 16 px: iOS zooms the page into a focused field with smaller text.
  field.style.cssText = "position:fixed;left:0;top:0;width:1px;height:1px;padding:0;border:0;opacity:0;font-size:16px;pointer-events:none;caret-color:transparent";
  doc.body.append(field);

  let wanted = false;
  let lift = 0;
  const sync = () => {
    const want = api.wants_keyboard();
    if (want !== wanted) {
      wanted = want;
      field.setAttribute("inputmode", want ? "text" : "none");
      if (!want) { field.blur(); canvas.focus({ preventScroll: true }); }
    }
    if (want && doc.activeElement !== field && performance.now() - lift < 1500) {
      field.focus({ preventScroll: true });
    }
    requestAnimationFrame(sync);
  };
  requestAnimationFrame(sync);

  // A finger lifted from the canvas: if a field wants the keyboard, (re)focus the input so the
  // keyboard comes back even when the player dismissed it with the system's Back.
  canvas.addEventListener("pointerup", (event) => {
    if (event.pointerType === "mouse") return;
    lift = performance.now();
    if (api.wants_keyboard()) { field.blur(); field.focus({ preventScroll: true }); }
  });

  const named = new Set(["Backspace", "Delete", "Enter", "Tab", "Escape", "ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End"]);
  field.addEventListener("keydown", (event) => {
    if (event.isComposing || !named.has(event.key)) return; // soft keyboards often say "Unidentified": beforeinput has it
    api.push_key(event.key);
    event.preventDefault();
  });
  field.addEventListener("beforeinput", (event) => {
    if (event.isComposing) return; // the composition events below carry it
    switch (event.inputType) {
      case "insertText":
      case "insertReplacementText":
        if (event.data) api.push_text(event.data);
        break;
      case "insertFromPaste":
        api.push_text(event.data ?? event.dataTransfer?.getData("text") ?? "");
        break;
      case "deleteContentBackward": api.push_key("Backspace"); break;
      case "deleteContentForward": api.push_key("Delete"); break;
      case "insertLineBreak":
      case "insertParagraph": api.push_key("Enter"); break;
      default: return;
    }
    event.preventDefault();
  });
  field.addEventListener("compositionupdate", (event) => api.push_preedit(event.data ?? ""));
  field.addEventListener("compositionend", (event) => {
    api.push_commit(event.data ?? "");
    field.value = "";
  });
  return field;
}

// Portrait is playable, not comfortable: a bar above the game says so, and can be dismissed.
function rotateHint(hint, text, apply) {
  let dismissed = false;
  try { dismissed = sessionStorage.getItem("aurelia.hint") === "off"; } catch (_) { /* no storage */ }
  const label = hint.querySelector("span");
  label.textContent = text.rotate;
  const portrait = matchMedia("(orientation: portrait)");
  const update = () => { hint.hidden = dismissed || !portrait.matches; apply(); };
  hint.querySelector("button").addEventListener("click", () => {
    dismissed = true;
    try { sessionStorage.setItem("aurelia.hint", "off"); } catch (_) { /* no storage */ }
    update();
  });
  portrait.addEventListener("change", update);
  update();
}

// Fullscreen, landscape and a screen that stays on, from the one tap the browser asks for.
export function canEnterFullscreen() {
  const standalone = matchMedia("(display-mode: fullscreen), (display-mode: standalone)").matches || navigator.standalone === true;
  const framed = window.top !== window.self;
  return !standalone && !framed && !!(doc.fullscreenEnabled && root.requestFullscreen);
}

export async function enterFullscreen() {
  try { await root.requestFullscreen({ navigationUI: "hide" }); } catch (_) { return false; }
  try { await screen.orientation?.lock?.("landscape"); } catch (_) { /* iOS, desktop: not allowed */ }
  return true;
}

export async function keepAwake() {
  try {
    let lock = await navigator.wakeLock?.request?.("screen");
    doc.addEventListener("visibilitychange", async () => {
      if (doc.visibilityState === "visible" && (!lock || lock.released)) {
        try { lock = await navigator.wakeLock.request("screen"); } catch (_) { /* denied */ }
      }
    });
  } catch (_) { /* not supported or not allowed */ }
}

// Called once, before the game starts. `api` is the wasm module's exports.
export function prepare(api, { canvas, hint, text }) {
  api.set_mobile(true);
  lockGestures(canvas);
  const apply = followViewport(hint);
  followSafeArea(api);
  keyboard(api, canvas);
  rotateHint(hint, text, apply);
}
