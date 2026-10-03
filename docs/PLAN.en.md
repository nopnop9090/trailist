# TrayList — plan, what, how, when, why

The German original of this document is [PLAN.md](PLAN.md); both say the same
thing. The user-facing documentation is [README.md](../README.md) and
[README.de.md](../README.de.md).

In short: TrayList replaces the Windows 11 tray's grid flyout with a **vertical,
scrollable list with names**. Click the chevron as always — the window Windows just
opened is read, then hidden, and the list is shown in its place.

---

## Why

**Problem:** Windows 11 hides new tray icons behind the chevron and shows them as a
grid of unlabelled 16×16 glyphs. With several dozen icons that means hovering,
waiting for a tooltip, guessing.

**Goal:** A panel listing every icon **underneath each other with names**, with a
filter, keyboard control and scrolling — and one that feels like the native flyout,
only readable.

**Design principles:**
- No injection, no shell hooks, no administrator rights
- The click on the chevron stays the way in; nothing has to be relearned
- Explorer is never left in a broken state (hide rather than "close")
- One binary, portable, its configuration beside it

---

## What (scope)

| Feature | Description |
|--------|----------------|
| List | Icon + name (the tooltip) + current state per row, arrow keys/Enter/Esc |
| Filter | Search box, focused automatically, filters as you type |
| Sorting | Tray order or alphabetical |
| Click forwarding | Left opens the app, right opens the icon's real context menu |
| Pin | "Always show in the tray" writes `IsPromoted` in `NotifyIconSettings` |
| Hotkey | `Alt+Shift+T` opens the list without the chevron |
| Tray menu | Show the list, open the config folder, quit |
| Geometry | Panel anchored to the flyout, inside the work area of the right monitor, DPI-correct |
| Language | German and English, following the shell's own language by default |

**Out of scope:** changing the order of icons in the real tray, replacing icon
designs, the Windows 10 tray, remote control.

---

## How (architecture and technique)

```
watcher thread (owns UI Automation and every shell access)
  |
  |- island.rs    find the flyout       TopLevelWindowForOverflowXamlIsland
  |- uia.rs       read the icons        buttons with AutomationId=NotifyItemIcon,
  |                                     whose Name is the complete tooltip
  |- capture.rs   fetch the icons       the shell's PNG snapshot first
  |                                    (NotifyIconSettings\IconSnapshot), otherwise
  |                                    PrintWindow(PW_RENDERFULLCONTENT) + chroma key
  |- suppress     ShowWindow(island, SW_HIDE)
  |- overlay      our own panel in the geometry of the flyout
  |- forward.rs   replay a click        SW_SHOWNOACTIVATE, SendInput, SW_HIDE
```

### The findings this is built on

Everything checked empirically on **Windows 11 25H2, build 26200**:

1. **The classic route is dead.** There is no `ToolbarWindow32` in the tray any
   more; `TrayNotifyWnd` is empty. `TB_BUTTONCOUNT`/`TB_GETBUTTON`/`TRAYDATA` no
   longer work. (Windows 11 22H2 introduced that.)
2. **The flyout is an addressable window:**
   `TopLevelWindowForOverflowXamlIsland`, visible while the flyout is open and
   hidden otherwise.
3. **UI Automation supplies the names** as long as the flyout is open:
   `ClassName=SystemTray.NormalButton`, `AutomationId=NotifyItemIcon`, `Name` = the
   complete tooltip — line breaks included.
4. **`ShowWindow(SW_HIDE)` hides the grid cleanly**, and the next click on the
   chevron opens it normally again: Explorer's state stays intact.
5. **Clicks can be replayed**: show the flyout briefly without activating it, click
   through `SendInput`, hide it again → the target app's real menu appears.
6. **`HKCU\Control Panel\NotifyIconSettings`** holds `ExecutablePath`,
   `InitialTooltip`, `IconSnapshot` (PNG) and `IsPromoted` per icon.

### Important technical decisions

1. **One thread, one truth.** UI Automation and every Win32 interaction run on the
   watcher thread. The UI only calls commands; every shell request goes there over a
   channel. That avoids COM apartment trouble on Tauri's main thread (WebView2 needs
   STA there) and keeps 300 ms click sequences off the UI.
2. **A state machine instead of a mess of events.** `Idle -> Reading -> Open ->
   Idle`, with `blackout` (ignore our own actions) and `cooldown` (the click that
   closes must not be read as "open").
3. **`SetCursorPos` does not exist in the windows-rs bindings.** Hence absolute
   `SendInput` movement in virtual-desktop coordinates, which covers a second
   monitor as well.
4. **Force the foreground.** The click that opened us went to the taskbar, so we are
   not the foreground process. `AttachThreadInput` on the foreground and the target
   thread is the documented way there, and it is tried **twice**: the first attempt
   often fails, and an auto-hide taskbar hands the foreground back when it folds
   away.
5. **Read only once the layout is settled.** The flyout animates open and places each
   icon in turn. A read at the moment it appears therefore returns icons at positions
   they are just leaving, and some without a rectangle at all. An element without a
   rectangle reports the origin — it would sort to the front of the list and cut the
   wrong corner out of the bitmap. Reading starts once a read reports nothing
   unplaced any more and the count repeats. Those are the ~150 ms Windows' grid stays
   visible.
6. **Icons come from the shell, not from the screen.** `HKCU\Control Panel\`
   `NotifyIconSettings` stores a PNG snapshot plus `InitialTooltip` for every icon;
   rows are matched by tooltip and so get the real bitmap with real transparency.
   Only icons without a snapshot are cut out of a
   `PrintWindow(PW_RENDERFULLCONTENT)` rendering of the flyout and keyed against its
   background.

7. **Positions come from what was drawn.** UIA rectangles can still be moving
   during the opening animation; a rectangle half a cell off cuts out background and
   pushes the icon down. So the cell grid is measured from the rendering (projecting
   "not the background" onto both axes and cutting the result into evenly spaced
   bands) — and the order comes from the shell's own listing rather than from those
   wobbling rectangles.
8. **Explorer is left alone.** Hiding instead of closing; no hooks, no restart, no
   registry manipulation beyond the explicit pin.
9. **The panel follows the shell's appearance.** Windows keeps two schemes side by
   side: `AppsUseLightTheme` (app windows — and that is what `prefers-color-scheme`
   reports in a webview) and `SystemUsesLightTheme` (taskbar, tray, flyout). The
   panel replaces the flyout, so it follows the shell scheme: read on every opening
   on the watcher thread and sent along with the list as `tl:theme`. `index.html`
   therefore no longer carries a theme class — the `:root` tokens are the light
   ones, so a panel without an answer is correct rather than black.
10. **The panel sits at the chevron and on the taskbar.** Two bugs in the old
    geometry: the panel was aligned to `flyout.right` and *additionally* inset by the
    shadow margin, which left the visible card 44 px away from the flyout edge (2 ×
    `SHADOW`), and the width pushed it to the edge of the screen through the clamp
    instead of under the chevron. Now it is centred horizontally *on the flyout's
    midpoint* — the shell centres the flyout on the chevron, so the panel stands
    under the button the user clicked — and the window's bottom edge (shadow margin
    included) rests on `flyout.bottom`, i.e. on the taskbar. The shadow margin must
    *not* overlap the taskbar: the window is always-on-top and does not pass clicks
    through. `edgeGap` moves the panel up if wanted.
11. **Settings in a modal, autostart as a real Run entry.** The gear opens a dialog
    inside the panel (not a second window: the panel is exactly as big as it needs to
    be, and a window of its own would have to be placed and closed separately). Height
    and gap act immediately: `set_prefs` re-places the open panel, and `Shared`
    remembers the flyout position needed for that. "Start with Windows" writes
    `HKCU\...\CurrentVersion\Run` instead of using a plugin — that is where Windows
    looks and where Task Manager can switch it off.
12. **Version and build stamp.** A panel that looks like the system has to be able to
    say which build it is. The stamp is made in `build.rs` from `GetLocalTime` (no
    date crate; the compiler host is always Windows) and injected as `TRAILIST_BUILT`.
    Important: `cargo:rerun-if-changed=src`, because Cargo caches build script output
    — without it a new build carries an old date.
13. **The name never appears twice in a row.** This machine's tooltips come in three
    shapes: two lines (`CapsLock` / `shift` as two lines), one line with a colon
    (`CapsLock: shift`), and one line with the name repeated
    (`TrayMaster TrayMaster - 3/3 running`). The third shape is why rows used to show
    the name twice. `TrayItem::compose` takes off the longest leading word repetition
    and trims the separator behind it; the test in `types.rs` runs against this
    machine's real tooltips.
14. **Two languages, and errors that travel as codes.** The panel, the tray menu and
    the error messages follow a `lang` setting whose default is `system` — Windows'
    own user-interface language, with English as the fallback for everything that is
    not German. An error therefore no longer travels as a finished German sentence:
    it travels as a code plus, where Windows had something to say, its raw text, and
    the sentence is written where the language is known. `commands::system_lang`
    answers once at startup, and `retitle_tray` hands the shell a new menu when the
    setting moves.
15. **Paths do not leak the build machine.** rustc records the source file of every
    panic site, and for dependencies that is an absolute path under the builder's
    user profile. `scripts/build.ps1` remaps those prefixes through
    `--remap-path-prefix`, with the prefixes taken from the environment so that no
    machine-specific string sits in the script. Same binary, one less thing in it.

### Repository structure

| Path | Role |
|------|--------|
| `src/` | React panel (the list) |
| `src/lib/i18n.ts` | The panel's German and English strings |
| `src-tauri/src/watcher.rs` | State machine |
| `src-tauri/src/overlay.rs` | Geometry and placement |
| `src-tauri/src/i18n.rs` | The language setting and the tray menu's strings |
| `src-tauri/src/win/` | The points where the shell is touched |
| `src-tauri/src/win/theme.rs` | Light or dark shell scheme |
| `src-tauri/src/win/autostart.rs` | The Run entry for "start with Windows" |
| `src-tauri/src/types.rs` | What crosses the bridge, and how a tooltip becomes a row |
| `src-tauri/build.rs` | Bakes in the build stamp |
| `public/badges/` | The not-by-humans badge, one version per panel theme |
| `src-tauri/src/bin/probe.rs` | Console front end for testing without the UI |
| `scripts/build.ps1` | Release build |
| `config/settings.json` | Settings (portable) |

---

## When (flow and use)

### Day to day
1. Start `TrayList.exe` (or let autostart do it) — it runs quietly in the tray.
2. Click the tray chevron as always.
3. Windows' grid appears briefly, disappears, and the list stands in the same place.
4. Typing filters, arrow keys move, `Enter` opens, right click shows the menu.
5. `Esc`, a click beside it, or the chevron again closes it.

### Development
1. Node 20+, Rust (MSVC), `npm install`, `npm rebuild esbuild`.
2. `npm start` → Tauri dev.
3. Check the shell behaviour without the UI: `cargo run --bin trailist-probe --
   list|watch|hide|theme`.
4. Release: `.\scripts\build.ps1 -Portable`.

### What to check first after a Windows update
`trailist-probe list` — if the number of icons is right and the names look
plausible, the window class and the automation ids are unchanged. If not, `watch`
shows fastest what the new build is reporting.

---

## Plan (status)

### Done (2026-10)
- [x] Feasibility proven empirically (detection, UIA reading, hide, click forwarding)
- [x] Tauri v2 skeleton: Rust core + React panel, portable binary, no console window
- [x] Watcher state machine, overlay geometry including DPI and work area
- [x] Icons from `IconSnapshot`, cell grid measured from the rendered flyout
- [x] Filter, sorting, keyboard, left and right click forwarding
- [x] Tray icon with a menu, global hotkey, `config/settings.json`
- [x] Pin through `IsPromoted`
- [x] `trailist-probe` as a test front end
- [x] Handover brought from 2.2 s down to ~0.5 s (registry check once instead of per
      icon, positions from the grid instead of UIA rectangles)
- [x] Panel flush on the taskbar and centred under the clicked chevron
- [x] Settings dialog inside the panel: height, gap to the taskbar, sorting, search
      field, "always-visible first"
- [x] "Start with Windows" through the per-user Run entry
- [x] Version + build stamp in the footer and the dialog (`build.rs`, `TRAILIST_BUILT`)
- [x] Panel follows the shell colour scheme (`SystemUsesLightTheme`)
- [x] not-by-humans badge in the dialog, light and dark, with a link back
- [x] Tooltip repetition removed from names, with a test against real tooltips
- [x] German and English in the panel, the tray menu and the error messages
- [x] README with screenshots, version, credit — as a pair, one per language
- [x] Repo published: `nopnop9090/trailist` with a `v0.3.0` release

### Open / optional
- [ ] **Speed up the reaction / avoid the grid entirely.** Two routes, both with a
      price:
      1. *UIA events instead of polling.* Rather than asking `EnumWindows` every
         40 ms, listen for `AutomationFocusChanged`/`WindowOpened` (or
         `SetWinEventHook(EVENT_OBJECT_SHOW)` on the island class). Saves the ~60 ms
         in which the grid is visible, but changes nothing about the ~200 ms the
         shell needs to lay out.
      2. *A Windhawk mod in explorer.exe* (`@include explorer.exe`) that does not
         draw the grid in the first place and opens our list directly instead. That
         is the only way to get rid of the flash completely — at the price of foreign
         code running inside Explorer, and of the `SystemTray` icons moving on new
         builds (see windhawk-mods #4071). Models: `tray-hover-expand`,
         `tray-utility-customizer`.
- [ ] **Clicks without replaying them** — that would also remove the flyout's brief
      reappearance on a click.
- [ ] Click interception through `WH_MOUSE_LL`: global mouse hooks deliver
      coordinates *and* stop the click from being passed on (`return 1`; that is the
      only way the click stays with our list). The flyout would no longer have to be
      shown — the click would come to us and we would send it to the icon. Catches:
      it runs in the app's UI thread, needs a message loop, costs a hook round trip
      per click, and some games with raw input bypass the hook anyway. A
      `RegisterHotKey` hotkey would even become globally consumable that way — an
      approach for "intercept the chevron click".
- [ ] Fetch an icon from the application itself when there is no snapshot:
      `ExecutablePath` is in `NotifyIconSettings` and `ExtractIconEx` yields the
      executable's real icon — sharp at any size, but possibly not the live state
      (which the snapshot does not guarantee either). UI Automation itself carries no
      image data; the snapshot and the executable are the only real sources.
- [ ] **Trigger the list with `Ctrl` + the right Windows key** (a wish). The Windows
      key is awkward as a hotkey: Windows intercepts it itself, and `RegisterHotKey`
      cannot express it as a modifier. Realistic is a low-level keyboard hook
      (`WH_KEYBOARD_LL`) like the click point above, which recognises the combination
      and swallows it — or a free combination without the Windows key.
- [ ] Replay hover tooltips (the target app would have to show its tooltip)
- [ ] Avoid the flicker on click forwarding entirely: move the flyout off screen and
      capture it with `PrintWindow(PW_RENDERFULLCONTENT)` instead of showing it
- [ ] Group/reorder icons, profiles

### Risks / notes
- **Build changes.** The chain hangs on one window class and three automation ids.
  `trailist-probe` turns diagnosis into a minute's work.
- **Auto-hide taskbar.** The flyout only opens while the taskbar is visible; the
  geometry comes from the flyout itself, so it is right anyway. The hotkey makes the
  list independent of that.
- **Full-screen apps.** TrayList does not hide there at the moment; a guard like the
  one in `tray-hover-expand` would make sense.
- **Tooltips are not a contract.** Apps decide what goes in them.

---

## Quick reference

```powershell
# Dev
npm start

# Test the shell behaviour without the UI
cargo run --manifest-path .\src-tauri\Cargo.toml --bin trailist-probe -- list
cargo run --manifest-path .\src-tauri\Cargo.toml --bin trailist-probe -- watch

# Release
.\scripts\build.ps1 -Portable

# Settings
notepad .\config\settings.json
```

See also: [README.md](../README.md) · [README.de.md](../README.de.md) ·
[PLAN.md](PLAN.md) (German).
