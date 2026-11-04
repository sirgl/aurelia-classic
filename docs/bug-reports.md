# Bug reports

**The "Report a bug" button opens a prefilled GitHub issue and saves a report file; the game sends nothing.** The player reads the text, presses Submit on GitHub, and drags the file into the issue.

## What the button does

"Report a bug" is on the main menu and on the in-game menu (Esc); it is the same on desktop, in the browser and on Android. It opens a dialog with:

1. **The text of the issue**, read-only, exactly what the page will be filled with: the sections of the template (`.github/ISSUE_TEMPLATE/bug_report.md`) and the facts below.
2. **Open the issue page** — opens `https://github.com/<repo>/issues/new?template=bug_report.md&title=…&body=…` in the system browser (a new tab on the web). Nothing leaves the game before the player presses Submit there.
3. **Save report file** — makes `aurelia-report-YYYYMMDD-HHMMSS.zip` (under 5 MB) and says where it went. On desktop it is written to `reports/` in the saves folder (`$AURELIA_SAVE_DIR`, default `saves/`), the dialog shows the full path and a "Copy path" button. In the browser it is a download (a Blob and an anchor, `static/report.js`). GitHub accepts `.zip` as an attachment.

The report file holds `report.txt` (the same facts and the log tail), `game.aurelia` (the match as it is now, a normal tagged save) and, once an autosave exists, `autosave.aurelia` with `autosave.aurlog` (the commands since that autosave). To replay: unzip, then `aurelia-classic --replay autosave.aurlog` (see `../../platform/docs/saves.md`, section 3). A part that would take the file past 5 MB is left out and named in `report.txt`. In the browser the autosave and its journal come from the browser's storage (`platform/docs/saves.md`, section 4), so its file has them too once an autosave exists.

## What the text says

Game version (`CARGO_PKG_VERSION` of the launcher; the commit too when the build is made with `AURELIA_GIT_HASH` set), operating system and architecture, graphics backend and adapter (on the web also `navigator.userAgent`), the language, the window size, whether a match runs with its seed (unknown for a match loaded from a save, which does not keep it), the round and the number of the last command played in this session, the last panic (message and location) if there was one, and the last 20 lines of the game's log (warnings and errors).

## Privacy

- Nothing is sent by the game: no server, no background request.
- Not included: the player's name, the environment variables, the IP address, the settings file, other saves.
- The home directory is written as `~` everywhere in the text (`$HOME`/`%USERPROFILE%`, and any `/Users/<name>/`, `/home/<name>/`, `C:\Users\<name>\` prefix).
- The player reads the whole text in the dialog before the page opens; the free-text sections are theirs to write on GitHub.
- The issue text is cut to 6000 characters before it is encoded, and the whole link is kept under 7500 bytes (GitHub refuses about 8 KB): the oldest log lines go first, the facts never.

## A crash in the browser

A panic in the web build cannot be handled by the game (the wasm instance is dead after it), so `static/report.js` shows its own dialog over the frozen canvas: the message, the same issue text, "Open the issue page", "Save report file" (a `.txt` with the facts and the log, no save) and "Reload the page". The words come from the game's language files and were resolved before the crash (`client::shell::report::remember`).

The trace: the full stack the page's panic hook keeps in `window.__aureliaLastPanic` ({message, location, stack}; a stack made in the crash hook if the global is missing) goes into the report file, the first 40 lines into the issue before the log, and `window.__aureliaBuild` ({wasm, art}) is written as "Web build" so the matching symbols file can be picked (`web-traces.md`, `platform/tools/launcher/wasm-symbolize.py`). On desktop the Rust backtrace of the panic is captured in the panic hook the same way. Home directories are written as `~` in all of it.

## Change the repository

The repository is one constant: `ISSUE_REPO` in `platform/crates/client/src/shell/report/url.rs` (`"sirgl/aurelia-classic"` today, while the repository is private). Change that line when the repository is renamed or made public somewhere else; the template name is `ISSUE_TEMPLATE` next to it. The template files are `.github/ISSUE_TEMPLATE/bug_report.md` and `config.yml` (`blank_issues_enabled: true`).

## Code map

| Part | Where |
|---|---|
| link, encoding, cap, home scrubbing | `platform/crates/client/src/shell/report/url.rs` |
| facts, title and body | `…/report/info.rs` |
| zip, size cap | `…/report/bundle.rs` |
| last panic, log tail, crash snapshot | `…/report/crash.rs` |
| dialog | `…/report/dialog.rs` |
| gathering and saving (the app) | `platform/crates/client/src/shell/app/report_host.rs` |
| web download and crash hook | `platform/crates/launcher-core/src/web/report.rs`, `launcher/web/static/report.js` |
| build registration | `launcher/*/src/main.rs`, `lib.rs` (`report::set_build`) |
| the seed of a match | `ModuleUi::world_seed`, `classic-ui/src/module.rs` |
