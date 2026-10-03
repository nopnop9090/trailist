# TrayList

Shows the hidden Windows tray icons as a **scrollable list with names** instead
of an icon grid you have to recognise by sight.

Click the tray chevron as you always have. TrayList already knows the icons
from their registrations, keeps the Windows grid from opening, and shows its
own panel in that corner: one row per icon, icon on the left, name and current
state on the right, and a filter box for when there are forty of them.

*English · [Deutsch](README.de.md)*

```
  ▸ you click ^ in the tray
  ▸ the Windows grid stays hidden
  ▸ the list opens from the registrations Explorer already holds
  ▸ click or hover a row -> that app gets the same message the shell would send
  ▸ Esc / click away / the chevron again -> the list goes away
```

Built as a [Tauri v2](https://tauri.app) app: a Rust core that talks to the shell
and a React panel for the list. A small DLL inside Explorer forwards hover and
clicks to the icon that registered them. No elevation.

![The list: one row per hidden icon, with names, states and a filter box](docs/screenshot-list-en.png)

## What, who, when, how

| | |
|---|---|
| **What** | The Windows 11 tray overflow, as a named and filterable list instead of a grid of 16x16 glyphs. |
| **Who** | Written for one very full tray, with an AI assistant doing the typing — which is what the badge at the bottom is about. |
| **When** | Version `0.4.0`. The build stamp is baked in at compile time and shown in the panel's footer, in the settings dialog, and to `trailist-probe theme`. |
| **How** | A small DLL inside Explorer records each `Shell_NotifyIcon` registration and forwards hover and clicks to that window. A React panel draws the list. If the DLL cannot attach, the previous UI Automation path remains and the panel says so. Settings live in `config/settings.json` next to the exe. |

The settings dialog, including where the badge and the version live:

![The settings dialog: height limit, gap to the taskbar, language, and the about block](docs/screenshot-settings-en.png)

## Why this exists

Windows 11 hides new tray icons behind a chevron and shows them in a fixed grid
of unlabelled 16x16 glyphs. With a few dozen icons that means hovering, waiting
for tooltips, and guessing. The icon *names* exist — they are the tooltips — they
just are not shown anywhere at once. That is the whole problem this solves.

## Features

**The list**
- Icon, name and current state per row — the tooltip is the name, and it is
  often the most useful text on the machine (`GPU: Eco | 250 W | 51 C`,
  `AdGuard VPN — Türkei (Istanbul): Getrennt`, `TrayMaster - 3/3 running`)
- Filter box, auto-focused: type a few letters, the list narrows as you type
- Sorting: the shell's own order, or alphabetical
- Keyboard: `↑`/`↓` to move, `Enter` to open, `Esc` to close
- Right-click a row for the icon's own context menu
- Panel sized to the content, centred on the chevron that was clicked and resting
  on the taskbar, always inside the work area of the right monitor
- Follows the shell's colour scheme: the palette is chosen from
  `SystemUsesLightTheme`, the same setting the taskbar and the flyout follow, so
  the panel matches the tray it replaces even when Windows is set to a different
  scheme for app windows

**Settings** (the gear in the panel, or `config/settings.json`)
- How tall the list may get, up to 20-odd rows without scrolling
- How much air to leave between the panel and the taskbar, from flush to 40 px
- Start with Windows, via the per-user autostart entry Windows itself reads
- Filter box on or off, always-visible icons first, alphabetical or tray order
- Language: `system`, `de` or `en`. The panel, the tray menu and the error messages
  all follow it; anything that is not German gets English rather than a mixture

**Tray attributes**
- Optional pin button per row: turns an icon on or off in the *visible* part of
  the tray by writing the shell's own `IsPromoted` flag
- Icons that are currently visible in the tray are marked as such

**Getting to it**
- The normal chevron click, or
- `Alt+Shift+T` from anywhere, or
- the tray icon's menu

## Requirements

- Windows 11 (build 22000+). It is built against Windows 11's XAML tray; on
  Windows 10 the flyout has a different shape and the list will not find it.
- WebView2, which ships with Windows 11.

## Getting started

```powershell
npm install
npm rebuild esbuild   # npm does not run esbuild's postinstall by default
npm start             # tauri dev
```

Release build (exe plus NSIS installer):

```powershell
.\scripts\build.ps1 -Portable
```

The portable exe lands in `dist-app/portable/` and, when the script has run,
also as `TrayList.exe` in the repo root next to `config/`.

## Testing without the UI

`trailist-probe` drives the same machinery from a console, which is the fastest
way to check the shell interaction after a Windows update:

```powershell
cargo run --manifest-path .\src-tauri\Cargo.toml --bin trailist-probe -- list
cargo run --manifest-path .\src-tauri\Cargo.toml --bin trailist-probe -- watch
cargo run --manifest-path .\src-tauri\Cargo.toml --bin trailist-probe -- hide
cargo run --manifest-path .\src-tauri\Cargo.toml --bin trailist-probe -- theme
```

`list` prints what the panel would show (and opens the flyout if it is closed),
`watch` reports every opening and how many icons were readable, `hide` puts a
stray flyout away, and `theme` prints the colour scheme the panel would use —
that last one straight from the registry, so a panel that looks wrong can be told
apart from a panel that read the wrong value.

With `TRAILIST_DEBUG=1` set, the app narrates the handover in
`logs/trailist.log` — including how long the read took and how many cells the
drawn grid was found to have — and writes every read to `logs/last-read.txt`
(index, click coordinates, icon size and title per row). Those two files are the
quickest way to see what the shell actually reported.

## Settings

`config/settings.json` next to the exe (or `%APPDATA%\TrayList` when that folder
is read-only). The gear in the panel edits the same file; the tray menu opens the
folder.

| Key | Default | Meaning |
|---|---|---|
| `panelWidth` | 344 | Panel width in logical pixels. The native flyout is only ~234, so there is room for the names. |
| `panelMaxHeight` | 900 | Upper bound; the list scrolls past it. Generous on purpose: on a large screen there is no reason to scroll a list that would have fitted. |
| `edgeGap` | 0 | Breathing room between the panel and the taskbar in logical pixels. `0` rests the panel on the taskbar the way the native flyout does. |
| `iconSize` | 16 | Icon size in the rows. |
| `sort` | `tray` | `tray` or `name`. |
| `showSearch` | true | Show the filter box. |
| `hotkey` | `Alt+Shift+T` | Global hotkey. Set `null` to disable. |
| `pinnedFirst` | false | Sort icons that are visible in the tray first. |
| `lang` | `system` | `de`, `en`, or `system` for the language Windows' own interface is in. Anything else counts as `system`. |

Starting with Windows is not in this file: it is a real
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run` entry, which is where
Windows looks and where Task Manager's startup tab can switch it off again.

## How it works

```
watcher thread
  |
  |- host.rs     load trailist_host.dll into explorer and talk over a pipe
  |              the DLL subclasses Shell_TrayWnd, records NIM_ADD / MODIFY /
  |              DELETE / SETVERSION, and copies the live icon
  |- swallow     while the pipe is connected, the overflow flyout is not shown
  |- overlay     the panel, anchored on the chevron
  |- click       post the callback that icon registered, version 0 or version 4
  |
  '- fallback, only when the DLL did not attach
       uia.rs + forward.rs    read the flyout, hide it, replay the click
```

Apps keep registering with Explorer. The DLL watches that path and forwards
pointer events back to the same window, identified by GUID or by window and id.
It does not replace the taskbar, the clock, or the icons that are pinned in the
visible strip.

On this machine the registration arrives as a 32-bit `NOTIFYICONDATA` inside
`WM_COPYDATA` (`dwData == 1`, signature `0x34753423`). An icon that never called
`NIM_SETVERSION` is version 0: `wParam` is the icon id and `lParam` is the mouse
message, so a right-click is `WM_RBUTTONUP`. Version 4, only after
`NIM_SETVERSION`, packs the point into `wParam` and the event into the low half
of `lParam`. Sending the version-4 form to a version-0 window does nothing.

The name is the live tooltip. When that string is empty, the row uses
`InitialTooltip` from `HKCU\Control Panel\NotifyIconSettings`. A registration
whose window has already gone is dropped; Explorer does the same when a process
is killed without `NIM_DELETE`, and the mirror used to keep those rows.

The previous path is still in the tree. UI Automation reads
`TopLevelWindowForOverflowXamlIsland` and `forward.rs` replays a click with
`SendInput`. The panel labels that list as a best effort. The Windows 10
`ToolbarWindow32` route is not implemented.

## Known trade-offs

- **With the host attached, the stock overflow does not stay on screen.** The
  chevron opens this list. Hover on a row is posted to that app, and a click names
  the registration (`GUID` or window and id) instead of a pixel in the flyout.
- **If the host cannot attach, the old flyout path is the fallback.** That list is
  labelled as a best effort: the grid can flash, a click briefly shows the flyout,
  and hover is not delivered. A click there is allowed to miss.
- **Icons come from the live registration when the host has copied one.** A row
  whose icon has not arrived yet still uses the PNG snapshot under
  `HKCU\Control Panel\NotifyIconSettings`.
- **The tooltip is the name.** Applications decide what goes in it, so a few rows
  read as a status rather than a name. An empty tooltip falls back to the shell's
  `InitialTooltip`. Where an application writes its own name twice —
  `TrayMaster TrayMaster - 3/3 running` — the repeat is removed, because only the
  repeat is wrong there.
- **No foreground, no keyboard.** The panel appears because you clicked the
  *taskbar*, so Windows may refuse to hand us the foreground. TrayList insists
  twice; if that ever fails, the list still works with the mouse.
- **A hidden taskbar hides the chevron.** With auto-hide on, the chevron is gone
  from the tree until the taskbar is up, so `Alt+Shift+T` has nothing to invoke
  and reports that it could not find it. Clicking the chevron is unaffected,
  because that is what reveals the taskbar in the first place.
- **Error messages are bilingual in one line each.** The sentence itself is written
  by the panel, so it is in the language that is set; what Windows said about the
  failure is appended unchanged, because it is Windows' own wording in Windows' own
  language. Only the two languages exist, and everything that is not German gets
  English.

## Layout

```
src/                     React panel (the list and the settings dialog)
src/lib/i18n.ts          the panel's German and English strings
src-tauri/src/
  watcher.rs             the state machine: host list, or the flyout fallback
  overlay.rs             panel geometry and placement
  commands.rs            IPC surface
  settings.rs            config/settings.json
  types.rs               what crosses the bridge, and how a tooltip becomes a row
  i18n.rs                the language setting, and the tray menu's own strings
  version.rs             the version label the panel shows
  win/host.rs            loads trailist_host.dll and talks to it
  win/forward.rs         replaying clicks, only when the host is not attached
  win/uia.rs             reading the flyout on that fallback
  win/island.rs          the overflow flyout window
  win/capture.rs         icon bitmaps
  win/registry.rs        NotifyIconSettings, IsPromoted
  win/theme.rs           which colour scheme the shell is drawing in
  win/autostart.rs       the per-user Run entry
  win/launch.rs          opening a link in the browser
  win/focus.rs           taking the foreground
  bin/probe.rs           console front end for testing
src-tauri/host/          the DLL Explorer loads; it mirrors Shell_NotifyIcon
src-tauri/build.rs       stamps the build with the compiler host's local time
public/badges/           the not-by-humans badge, one per panel theme
scripts/build.ps1        release build
scripts/make-icons.ps1   generates the icon set
docs/PLAN.md             the plan and the decisions, in German
docs/PLAN.en.md          the same plan and decisions, in English
docs/screenshot-*.png    the pictures above, one pair per language
```

## Tests

```powershell
cargo test --manifest-path .\src-tauri\Cargo.toml
```

Three tests in the app crate. The first is on the piece with real guesswork in
it: turning the tooltips this machine's tray actually produces into a name and a
state. Every case in it came off a real tray, including the awkward ones — the
name repeated on one line, the name repeated across two, a name that merely
shares a word, and a tooltip that is nothing but the name twice.

Two more cover the language setting: that the setting wins over the system's
language, and that every language has a complete tray menu. The frontend's own
strings are checked by the compiler rather than by a test — the tables are one
object each, so `tsc` is what catches a key that no longer exists.

The host crate checks the 32-bit `NOTIFYICONDATA` layout this build actually
sends, including a `Shell_NotifyIconGetRect` request. Run it with
`cargo test --manifest-path .\src-tauri\host\Cargo.toml`.

## Credit

The badge is [not by humans](https://notbyhumans.fyi) — *developed by AI, not by
humans*. Ink on a dark panel, paper on a light one; the copies in `public/badges`
are the files the site serves, and clicking the badge goes back to the site that
explains it.

<a href="https://notbyhumans.fyi"><img src="https://notbyhumans.fyi/badges/developed-paper.svg" width="165" height="54" alt="Developed by AI, not by humans"></a>
