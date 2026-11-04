// "Report a bug" in the page (docs/bug-reports.md). The game calls two things here:
//   aureliaReport.download(name, bytes)  the report file as a download (a Blob and an anchor)
//   aureliaReport.crash(info)            a panic: the program cannot run again, so the page shows
//                                        the dialog itself, with the same two buttons
// Every word comes from the game (its language setting); nothing here is translated.
(function () {
  "use strict";

  function download(name, bytes) {
    const type = /\.zip$/i.test(name) ? "application/zip" : "text/plain;charset=utf-8";
    const url = URL.createObjectURL(new Blob([bytes], { type }));
    const link = document.createElement("a");
    link.href = url;
    link.download = name;
    link.rel = "noopener";
    link.style.display = "none";
    document.body.appendChild(link);
    link.click();
    link.remove();
    setTimeout(() => URL.revokeObjectURL(url), 30000);
  }

  const css = `
    #aurelia-crash { position: fixed; inset: 0; z-index: 2147483000; display: flex; align-items: center; justify-content: center;
      background: rgba(5, 12, 15, .88); font-family: system-ui, sans-serif; color: #cfe0dc; }
    #aurelia-crash .box { box-sizing: border-box; width: min(760px, 100vw - 24px); max-height: calc(100dvh - 24px); overflow: auto;
      padding: 18px; background: #11242b; border: 1px solid #3f7d7c; border-radius: 8px; display: flex; flex-direction: column; gap: 10px; }
    #aurelia-crash h2 { margin: 0; font-size: 22px; font-weight: 600; }
    #aurelia-crash p { margin: 0; font-size: 14px; line-height: 1.4; color: #92a8aa; }
    #aurelia-crash label { font-size: 12px; color: #c1a263; }
    #aurelia-crash textarea { box-sizing: border-box; width: 100%; min-height: 160px; flex: 1; resize: vertical; padding: 8px;
      background: #192f34; color: #cfe0dc; border: 1px solid #3f7d7c; border-radius: 4px; font: 12px/1.35 ui-monospace, Menlo, Consolas, monospace; }
    #aurelia-crash .row { display: flex; flex-wrap: wrap; gap: 8px; }
    #aurelia-crash .row > * { flex: 1 1 180px; min-height: 44px; display: inline-flex; align-items: center; justify-content: center;
      box-sizing: border-box; padding: 0 14px; font: inherit; font-size: 15px; color: #cfe0dc; background: #192f34;
      border: 1px solid #3f7d7c; border-radius: 6px; text-decoration: none; cursor: pointer; }
    #aurelia-crash .row > .primary { background: #3f7d7c; border-color: #6fb3a5; }
    #aurelia-crash .row > *:hover { border-color: #6fb3a5; }
    #aurelia-crash .note { min-height: 18px; font-size: 13px; color: #6fb3a5; }
  `;

  function crash(info) {
    if (document.getElementById("aurelia-crash")) return;
    const words = info.words || {};
    const make = (tag, text, className) => {
      const element = document.createElement(tag);
      if (text !== undefined) element.textContent = text;
      if (className) element.className = className;
      return element;
    };
    const style = make("style", css);
    const root = make("div");
    root.id = "aurelia-crash";
    root.setAttribute("role", "alertdialog");
    root.setAttribute("aria-modal", "true");
    const box = make("div", undefined, "box");
    const title = make("h2", words.title || info.title);
    const intro = make("p", words.intro || "");
    const label = make("label", words.preview || "");
    const body = make("textarea");
    body.readOnly = true;
    body.value = info.body;
    body.setAttribute("aria-label", words.preview || "");
    const note = make("div", "", "note");
    const row = make("div", undefined, "row");
    // A plain link: it opens GitHub's "new issue" page with the text above filled in.
    const open = make("a", words.openIssue || "GitHub", "primary");
    open.href = info.url;
    open.target = "_blank";
    open.rel = "noopener noreferrer";
    const save = make("button", words.saveFile || "");
    save.type = "button";
    save.addEventListener("click", () => {
      download(info.fileName, new TextEncoder().encode(info.fileText));
      note.textContent = words.saved || info.fileName;
    });
    const reload = make("button", words.reload || "");
    reload.type = "button";
    reload.addEventListener("click", () => location.reload());
    row.append(open, save, reload);
    box.append(title, intro, label, body, note, row);
    root.append(box);
    document.head.append(style);
    document.body.append(root);
    open.focus();
  }

  window.aureliaReport = { download, crash };
})();
