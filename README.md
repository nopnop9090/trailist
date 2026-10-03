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
  ▸ a right-click leaves the list up until you click away
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
| **When** | Version `0.5.1`. The build stamp is baked in at compile time and shown in the panel's footer, in the settings dialog, and to `trailist-probe theme`. |
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
- Right-click a row for the icon's own context menu. The list stays open until
  you click away, so a second right-click does not mean opening the panel again
- A double-click is forwarded as a double-click. The single click waits out the
  system double-click time, so an icon that opens on double-click and one that
  shows a menu on a single click behave as they do in the tray
- Starting TrayList again finds the copy that is already running
- The first start asks whether to check GitHub for a newer release. With that on,
  the check runs once per start and, if the app stays open, once a day — whichever
  comes first. A newer release asks before the release page is opened. Nothing is
  downloaded. The same check is a switch and a button in the settings
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
- Pin button per row, including icons that are already in the visible strip, so
  the pin that put them there can be turned off again. It writes the shell's own
  `IsPromoted` flag
- Icons that are currently visible in the tray are marked as such
- An empty tooltip uses the shell's `InitialTooltip`, and when that is empty too
  the executable's own description, so a row is not left nameless
- Flat shell glyphs such as Safely Remove Hardware are drawn in the flyout's text
  colour: black on a light tray, white on a dark one. A colourful icon is left as
  it was registered
- Icons that redraw themselves constantly, such as Process Lasso, stay visible.
  The host copies the glyph only after Explorer has taken its own copy

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
also as `TrayList.exe` in the repo root next to `config/`. `trailist_host.dll`
is copied beside each of those. Without that DLL the panel falls back to UI
Automation.

## Installing

The NSIS installer is per-user and needs no administrator. It recognises a copy
that is already installed:

- **0.3 and older** have no host DLL. The running app is closed and the files are
  replaced. Explorer stays up.
- **0.4 and newer** load `trailist_host.dll` into Explorer. The taskbar restarts
  once so that file can be replaced; Windows brings the shell back by itself.
  Uninstalling one of those versions does the same, otherwise the DLL stays
  mapped and the file cannot be deleted.

A portable copy is `TrayList.exe` with `trailist_host.dll` in the same folder.

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
`InitialTooltip` from `HKCU\Control Panel\NotifyIconSettings`, and when that is
empty too, the executable's file description. A registration whose window has
already gone is dropped. Explorer does the same when a process is killed
without `NIM_DELETE`.

The glyph is read only after Explorer has stored its own copy. Reading the
caller's bitmap first is what used to blank icons that replace themselves on
every update. A 32-bit icon that leaves the alpha byte at zero takes its shape
from the mask. A glyph that is only one flat colour is then tinted to the shell
text colour, which is why Safely Remove Hardware matches the flyout instead of
staying the white form the taskbar registered.

The previous path is still in the tree. UI Automation reads
`TopLevelWindowForOverflowXamlIsland` and `forward.rs` replays a click with
`SendInput`. The panel labels that list as a best effort. The Windows 10
`ToolbarWindow32` route is not implemented.

## Known limits

- **The chevron opens this list.** Hover and clicks go to the registration
  (`GUID`, or window and id), not to a pixel in the stock grid. The grid itself
  stays hidden while the host is attached.
- **The UI Automation path is only the fallback**, used when the DLL could not
  attach. The panel labels that list. It is the old behaviour: the grid can
  flash, and hover is not delivered.
- **A row whose live glyph has not arrived yet** still uses the PNG snapshot under
  `HKCU\Control Panel\NotifyIconSettings`.
- **The tooltip is the name.** Applications decide what goes in it, so a few rows
  read as a status rather than a name. An empty tooltip falls back to
  `InitialTooltip`, then to the executable's description. Where an application
  writes its own name twice — `TrayMaster TrayMaster - 3/3 running` — the repeat
  is removed, because only the repeat is wrong there.
- **Upgrading 0.4 or newer restarts Explorer once.** The host DLL is mapped in
  that process, and the file cannot be replaced until the process is gone. The
  taskbar blinks and comes back. An upgrade from 0.3 does not need that.
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
  win/glyph.rs           tints a flat shell glyph to the flyout's text colour
  win/theme.rs           which colour scheme the shell is drawing in
  win/autostart.rs       the per-user Run entry
  win/launch.rs          opening a link in the browser
  win/focus.rs           taking the foreground
  bin/probe.rs           console front end for testing
src-tauri/host/          the DLL Explorer loads; it mirrors Shell_NotifyIcon
src-tauri/windows/       NSIS hooks: replace 0.3 in place, restart Explorer from 0.4 on
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

The app crate tests the piece with real guesswork in it: turning the tooltips
this machine's tray actually produces into a name and a state. Every case in it
came off a real tray, including the awkward ones — the name repeated on one
line, the name repeated across two, a name that merely shares a word, and a
tooltip that is nothing but the name twice. Further tests cover the language
setting (the setting wins over the system's language, and every language has a
complete tray menu) and that a flat white glyph is recoloured for a light shell.
The frontend's own strings are checked by the compiler rather than by a test —
the tables are one object each, so `tsc` is what catches a key that no longer
exists.

The host crate checks the 32-bit `NOTIFYICONDATA` layout this build actually
sends, including a `Shell_NotifyIconGetRect` request, and that an icon with no
alpha takes its shape from the mask. Run it with
`cargo test --manifest-path .\src-tauri\host\Cargo.toml`.

## Credit

The badge is [not by humans](https://notbyhumans.fyi) — *developed by AI, not by
humans*. Ink on a dark panel, paper on a light one; the copies in `public/badges`
are the files the site serves, and clicking the badge goes back to the site that
explains it.

<a href="https://notbyhumans.fyi"><img src="https://notbyhumans.fyi/badges/developed-paper.svg" width="165" height="54" alt="Developed by AI, not by humans"></a>
