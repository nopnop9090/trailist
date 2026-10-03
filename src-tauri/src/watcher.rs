//! Watches the tray overflow flyout and swaps it for our own list.
//!
//! One thread owns UI Automation and every interaction with the shell, so the
//! webview thread only ever draws. The loop is deliberately plain: look for a
//! visible flyout, read it, put it away, show the panel, then watch for the
//! reasons the panel should go away again.

use std::sync::mpsc::Receiver;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};

use crate::overlay;
use crate::state::{AppState, Request, Shared};
use crate::types::{Fault, Prefs, SortMode, TrayItem, TrayList};
use crate::win::host::Session;
use crate::win::{capture, forward, host, island, registry, uia};

/// How often the flyout is looked for. Well below the point where a click feels
/// unanswered, and the check is a couple of `EnumWindows` calls.
const TICK: Duration = Duration::from_millis(15);

/// How long the flyout's contents get to finish arriving. Each icon is placed as
/// its turn comes during the opening animation, so this has to cover that; it is
/// the only part of the handover the user sees.
const READ_BUDGET: Duration = Duration::from_millis(500);

/// How long a flyout with nothing readable is tolerated before we give up and
/// leave the native one alone.
const READ_GIVE_UP: Duration = Duration::from_millis(900);

/// After the panel closes, a flyout appearing means "the user wants the normal
/// tray back" rather than "open the panel". Without this, the click that
/// dismisses would immediately be read as a request to open.
const COOLDOWN: Duration = Duration::from_millis(400);

/// How long the flyout is left alone after it has been parked, before the panel
/// replaces it. Nothing is on screen during this, so it only costs latency while
/// the list is being read.
const PANEL_GOES_AWAY: Duration = Duration::from_millis(80);

/// Long enough to cover a replayed click, during which the flyout is on screen on
/// purpose and must not be mistaken for anything.
const REPLAY_BLACKOUT: Duration = Duration::from_millis(700);

/// Settling time after our own show, so a stale flyout reading cannot close the
/// panel we just opened.
const OPEN_SETTLE: Duration = Duration::from_millis(200);

/// How long the flyout is given before it is rendered into a bitmap. The list read
/// waits for the layout to settle, so by the time this runs the window is drawn.
const CAPTURE_SETTLE: Duration = Duration::from_millis(30);

/// How long to wait before a second attempt, and how much ink a full flyout has.
/// A grid of icons covers most of the window, so a much emptier bitmap than this
/// was caught too early.
const CAPTURE_RETRY: Duration = Duration::from_millis(220);
const CAPTURE_MIN_INK: f32 = 0.65;

const EVENT_LIST: &str = "tl:list";
/// Carries the shell's colour scheme alongside the list, so the panel never
/// renders one opening behind a theme the user just changed.
const EVENT_THEME: &str = "tl:theme";

#[derive(Clone, Copy)]
enum Phase {
    Idle,
    /// The user just opened the flyout and we are reading it.
    Reading { island_window: isize, since: Instant },
    /// Our panel is up and the flyout is put away. The rectangle is where the
    /// shell had the flyout, which is where a replayed click has to land.
    Open {
        island_window: isize,
        rect: island::Rect,
    },
}

/// Starts the watcher. The receiver is how every other thread asks it to touch
/// the shell.
pub fn spawn(app: AppHandle, shared: Arc<Shared>, receiver: Receiver<Request>) {
    std::thread::Builder::new()
        .name("trailist-watcher".to_string())
        .spawn(move || run(app, shared, receiver))
        .expect("cannot start the tray watcher thread");
}

fn run(app: AppHandle, shared: Arc<Shared>, receiver: Receiver<Request>) {
    // A missing reader is not fatal: the tray menu still explains itself.
    let reader = match uia::Reader::new() {
        Ok(reader) => Some(reader),
        Err(error) => {
            eprintln!("TrayList: UI Automation unavailable: {error}");
            None
        }
    };

    let start = Instant::now();
    let mut phase = Phase::Idle;
    let mut blackout_until = start;
    let mut cooldown_until = start;
    let mut host = Session::attach();
    let mut host_retry = start;
    // A click holds the row rectangle for the app's own GetRect. Releasing it
    // the moment the panel hides would put the menu back on the stock icon.
    let mut gesture_until = start;
    if host.is_some() {
        eprintln!("TrayList: host attached");
    } else {
        eprintln!("TrayList: host not attached, the flyout path stays in use");
    }

    // Get the webview painted before the first real opening needs it.
    overlay::warm_up(&app);

    loop {
        while let Ok(request) = receiver.try_recv() {
            handle_request(
                &app,
                &shared,
                reader.as_ref(),
                request,
                &mut phase,
                &mut blackout_until,
                &mut cooldown_until,
                &mut host,
                &mut gesture_until,
            );
        }

        let now = Instant::now();
        if host.as_ref().is_some_and(|session| !session.same_explorer()) {
            host = None;
        }
        if host.is_none() && now.duration_since(host_retry) >= Duration::from_secs(2) {
            host = Session::attach();
            host_retry = now;
        }

        let mut host_owns_corner = false;
        let mut host_dropped = false;
        if let Some(session) = host.as_mut() {
            if let Some(pulse) = session.pulse() {
                if pulse.bound && !pulse.broken {
                    host_owns_corner = true;
                    if shared.visible() && pulse.revision != session.revision() {
                        if session.refresh().is_some() {
                            publish_host(&app, &shared, reader.as_ref(), session, false);
                        }
                    }
                    if pulse.open_requested {
                        if shared.visible() {
                            overlay::hide(&app);
                            shared.set_visible(false);
                            session.release();
                            phase = Phase::Idle;
                            cooldown_until = now + COOLDOWN;
                        } else if now >= cooldown_until {
                            let _ = session.refresh();
                            publish_host(&app, &shared, reader.as_ref(), session, true);
                            phase = Phase::Open {
                                island_window: 0,
                                rect: shared.flyout().unwrap_or(island::Rect {
                                    left: 0,
                                    top: 0,
                                    right: 1,
                                    bottom: 1,
                                }),
                            };
                        }
                    }
                }
            } else {
                host_dropped = true;
            }
        }
        if host_dropped {
            host = None;
            host_retry = now;
        }

        if !host_owns_corner && now >= blackout_until {
            let open_island = island::visible().into_iter().map(island::raw).next();
            phase = tick(
                &app,
                &shared,
                reader.as_ref(),
                phase,
                open_island,
                now,
                &mut blackout_until,
                &mut cooldown_until,
            );
        }

        if !shared.visible() && now >= gesture_until {
            if let Some(session) = host.as_mut() {
                if session.bound {
                    session.release();
                }
            }
            gesture_until = now + Duration::from_secs(60);
        }

        std::thread::sleep(TICK);
    }
}

/// Everything the rest of the app asks the watcher to do, because the watcher is
/// the thread that may touch the shell.
fn handle_request(
    app: &AppHandle,
    shared: &Arc<Shared>,
    reader: Option<&uia::Reader>,
    request: Request,
    phase: &mut Phase,
    blackout_until: &mut Instant,
    cooldown_until: &mut Instant,
    host: &mut Option<Session>,
    gesture_until: &mut Instant,
) {
    let now = Instant::now();
    match request {
        Request::Toggle => {
            if shared.visible() {
                overlay::hide(app);
                shared.set_visible(false);
                if let Some(session) = host.as_mut() {
                    session.release();
                }
                *phase = Phase::Idle;
                *cooldown_until = now + COOLDOWN;
                return;
            }

            if let Some(session) = host.as_mut() {
                let _ = session.refresh();
                if session.bound {
                    publish_host(app, shared, reader, session, true);
                    *phase = Phase::Open {
                        island_window: 0,
                        rect: shared.flyout().unwrap_or(island::Rect {
                            left: 0,
                            top: 0,
                            right: 1,
                            bottom: 1,
                        }),
                    };
                    return;
                }
            }

            // Already open: let the normal detection path handle it.
            if !island::visible().is_empty() {
                return;
            }

            let Some(reader) = reader else {
                publish_error(app, Fault::new("uia_missing"));
                return;
            };
            let Some(taskbar) = island::find_by_class(island::TASKBAR_CLASS) else {
                publish_error(app, Fault::new("no_taskbar"));
                return;
            };

            *blackout_until = now + Duration::from_millis(150);
            match reader.chevron(island::raw(taskbar)) {
                Ok(Some(chevron)) => {
                    let _ = reader.invoke(&chevron);
                }
                Ok(None) => publish_error(app, Fault::new("no_chevron")),
                Err(error) => publish_error(app, Fault::with("chevron_unreadable", error)),
            }
        }

        Request::Hover {
            index,
            enter,
            anchor,
        } => {
            let Some(session) = host.as_mut() else {
                return;
            };
            if !session.bound {
                return;
            }
            let Some(key) = shared.item_at(index).and_then(|item| item.host_key) else {
                return;
            };
            let legacy = (!session.unknown_is_classic && enter).then(|| {
                session.icons.iter().find(|icon| icon.key == key && !icon.version_known).cloned()
            });
            if session.hover(&key, enter, anchor) && enter {
                *gesture_until = Instant::now() + Duration::from_secs(3);
            }
            if let Some(Some(icon)) = legacy {
                icon.post_legacy_move();
            }
        }

        Request::Click {
            index,
            right,
            anchor,
        } => {
            if let Some(session) = host.as_mut() {
                if session.bound {
                    if let Some(key) = shared.item_at(index).and_then(|item| item.host_key) {
                        let icon = session.icons.iter().find(|icon| icon.key == key).cloned();
                        // Grant while this process is still foreground. Hiding the
                        // panel gives the foreground away, and the menu needs the
                        // grant to already be in place.
                        if let Some(icon) = icon.as_ref() {
                            host::grant_foreground(icon.hwnd);
                        }
                        overlay::hide(app);
                        shared.set_visible(false);
                        std::thread::sleep(PANEL_GOES_AWAY);
                        let supplement = icon.as_ref().is_some_and(|icon| {
                            !session.unknown_is_classic && !icon.version_known
                        });
                        if session.activate(&key, right, anchor) {
                            *gesture_until = Instant::now() + Duration::from_secs(3);
                        }
                        // After the gesture is held, so GetRect during the
                        // handler names this row. The host's own message is the
                        // version-4 one these windows ignore.
                        if supplement {
                            if let Some(icon) = icon.as_ref() {
                                icon.post_legacy(right);
                            }
                        }
                        *phase = Phase::Idle;
                        *cooldown_until = Instant::now() + COOLDOWN;
                        return;
                    }
                }
            }

            let Some(item) = shared.item_at(index) else {
                return;
            };
            let (x, y) = (item.x, item.y);
            if let Phase::Open {
                island_window,
                rect,
            } = *phase
            {
                // Our own window has to be out of the way first: it is
                // always-on-top, so a click at the icon's coordinates would land
                // on our row instead. The hide is dispatched to the main thread,
                // hence the pause before clicking.
                overlay::hide(app);
                shared.set_visible(false);
                std::thread::sleep(PANEL_GOES_AWAY);

                // The flyout is about to be on screen deliberately, so detection
                // has to sit this one out.
                *blackout_until = Instant::now() + REPLAY_BLACKOUT;
                let button = if right {
                    forward::Button::Right
                } else {
                    forward::Button::Left
                };
                match forward::replay(island::hwnd(island_window), rect, x, y, button) {
                    Ok(true) => crate::trace!("watcher: replayed a click at {x},{y}"),
                    Ok(false) => {
                        crate::trace!("watcher: the flyout would not come back for a click")
                    }
                    Err(error) => crate::trace!("watcher: replaying a click failed: {error}"),
                }
            } else {
                crate::trace!("watcher: a click arrived while no panel was open");
            }
            *phase = Phase::Idle;
            *cooldown_until = Instant::now() + COOLDOWN;
        }
    }
}

/// One pass of the state machine. Returns the phase to continue in.
fn tick(
    app: &AppHandle,
    shared: &Arc<Shared>,
    reader: Option<&uia::Reader>,
    phase: Phase,
    open_island: Option<isize>,
    now: Instant,
    blackout_until: &mut Instant,
    cooldown_until: &mut Instant,
) -> Phase {
    match phase {
        Phase::Idle => match open_island {
            None => Phase::Idle,
            Some(island_window) => {
                if now < *cooldown_until {
                    // The chevron click that just closed our panel. Swallow it
                    // so the two do not fight over the same gesture.
                    crate::trace!("watcher: flyout appeared during cooldown, hiding it");
                    island::hide(island::hwnd(island_window));
                    Phase::Idle
                } else {
                    crate::trace!("watcher: flyout opened at 0x{island_window:X}, reading");
                    Phase::Reading {
                        island_window,
                        since: now,
                    }
                }
            }
        },

        Phase::Reading {
            island_window,
            since,
        } => {
            let window = island::hwnd(island_window);
            if !island::is_visible(window) {
                *cooldown_until = now + COOLDOWN;
                return Phase::Idle;
            }

            let Some(reader) = reader else {
                *cooldown_until = now + COOLDOWN;
                return Phase::Idle;
            };

            // Out of sight immediately, then read and render it there. Parking off
            // screen rather than hiding matters: a hidden window answers nothing
            // and renders blank, a parked one keeps both. So the native grid is
            // visible for only as long as the poll interval, and everything the
            // handover needs happens while the user sees nothing.
            //
            // The position is taken from the last time the flyout was really on
            // screen, because a flyout that was parked and then shown again by
            // explorer is still where we left it.
            let found = island::rect(window).filter(|rect| island::is_on_screen(*rect));
            let Some(onscreen) = found.or_else(island::remembered_rect) else {
                island::hide(window);
                *cooldown_until = now + COOLDOWN;
                return Phase::Idle;
            };
            island::remember_rect(onscreen);
            island::park(window, onscreen);

            let reading_since = Instant::now();

            let raw = reader
                .items_settled(island_window, READ_BUDGET)
                .unwrap_or_default();
            crate::trace!(
                "watcher: read {} names in {} ms",
                raw.len(),
                reading_since.elapsed().as_millis()
            );

            if raw.is_empty() {
                if now.duration_since(since) >= READ_GIVE_UP {
                    // Nothing to show, so leave the native flyout alone rather
                    // than replacing it with an empty panel.
                    island::hide(window);
                    *cooldown_until = now + COOLDOWN;
                    return Phase::Idle;
                }
                return phase;
            }

            let grab = |wait: Duration| {
                std::thread::sleep(wait);
                capture::print_window(window, onscreen.width(), onscreen.height())
                    .map_err(|error| eprintln!("TrayList: rendering the flyout failed: {error}"))
                    .ok()
            };

            let mut bitmap = grab(CAPTURE_SETTLE);
            if bitmap
                .as_ref()
                .map(|bitmap| capture::ink_ratio(bitmap) < CAPTURE_MIN_INK)
                .unwrap_or(false)
            {
                crate::trace!("watcher: capture looked half drawn, retrying");
                if let Some(better) = grab(CAPTURE_RETRY) {
                    let improved = bitmap
                        .as_ref()
                        .map(|current| capture::ink_ratio(&better) > capture::ink_ratio(current))
                        .unwrap_or(true);
                    if improved {
                        bitmap = Some(better);
                    }
                }
            }

            // The icons are in hand, so the flyout can go. Back to its own place
            // first, so a hidden flyout is right where the shell left it.
            island::move_to(window, onscreen);
            island::hide(window);
            let collected_at = reading_since.elapsed().as_millis();
            crate::trace!("watcher: everything ready in {collected_at} ms");
            crate::trace!(
                "watcher: capture {}",
                bitmap
                    .as_ref()
                    .map(|bitmap| format!("{}x{}", bitmap.width, bitmap.height))
                    .unwrap_or_else(|| "missing".to_string())
            );

            let prefs = app.state::<AppState>().prefs();
            let items = collect(onscreen, &raw, bitmap, &prefs);
            let count = items.len();

            shared.set_items(items.clone(), island_window, onscreen);
            publish(app, items, None, false, true);
            overlay::show(app, onscreen, count, &prefs);
            shared.set_visible(true);
            crate::trace!(
                "watcher: showing {count} icons, flyout {}x{} at {},{}",
                onscreen.width(),
                onscreen.height(),
                onscreen.left,
                onscreen.top
            );
            dump(&shared.items());

            // Our own show may race the next visibility read.
            *blackout_until = Instant::now() + OPEN_SETTLE;

            Phase::Open {
                island_window,
                rect: onscreen,
            }
        }

        Phase::Open { island_window, rect } => {
            if !shared.visible() {
                // Dismissed: Esc, a row click, a click away, or the tray menu.
                crate::trace!("watcher: panel no longer wanted, closing");
                let window = island::hwnd(island_window);
                if island::is_visible(window) {
                    island::hide(window);
                }
                *cooldown_until = now + COOLDOWN;
                return Phase::Idle;
            }

            if let Some(other) = open_island {
                // A flyout is on screen while our panel owns that corner, so the
                // user clicked the chevron again, which on this shell means
                // "close".
                crate::trace!("watcher: chevron toggled the flyout again, closing the panel");
                island::hide(island::hwnd(other));
                overlay::hide(app);
                shared.set_visible(false);
                *cooldown_until = now + COOLDOWN;
                return Phase::Idle;
            }

            Phase::Open { island_window, rect }
        }
    }
}

/// Writes the last read out to `logs/last-read.txt`, which is the only practical
/// way to see what the shell actually reported on a machine that is not the one
/// the code is being written on. Debug only.
fn dump(items: &[TrayItem]) {
    if !crate::log::is_enabled() {
        return;
    }
    let mut text = String::new();
    for item in items {
        text.push_str(&format!(
            "{:>2}  {:>5},{:<5} icon={:<6} {}\n",
            item.index,
            item.x,
            item.y,
            item.icon.len(),
            // The tooltip escaped rather than as it reads: when a row shows the
            // wrong thing, the separator inside the tooltip is the first thing
            // worth seeing, and an invisible one is exactly what cannot be seen.
            item.tooltip.escape_debug()
        ));
    }
    if let Some(dir) = crate::log::directory() {
        let _ = std::fs::write(dir.join("last-read.txt"), text);
    }
}

/// Turns the raw read into rows.
///
/// Icon positions are taken from the drawn grid rather than from UI Automation:
/// its rectangles can still be moving while the flyout animates, and one that is
/// half a cell off crops the wrong square. The drawn grid is in flyout-local
/// pixels, so a cell centre is both the middle of the square to crop and — once
/// the flyout's own origin is added — the point to click.
fn collect(
    rect: island::Rect,
    raw: &[uia::RawItem],
    bitmap: Option<capture::ScreenBitmap>,
    prefs: &Prefs,
) -> Vec<TrayItem> {
    let entries = registry::entries();
    let icon_size = prefs.icon_size.max(8);
    let cells = bitmap
        .as_ref()
        .map(capture::grid_centres)
        .unwrap_or_default();
    // Only trust the grid when it has exactly one cell per icon; otherwise fall
    // back to what the shell reported, however wobbly that is.
    let aligned = cells.len() == raw.len();
    crate::trace!(
        "watcher: grid has {} cells for {} icons{}",
        cells.len(),
        raw.len(),
        if aligned { "" } else { ", falling back to reported rectangles" }
    );

    let mut items: Vec<TrayItem> = raw
        .iter()
        .enumerate()
        .map(|(position, raw_item)| {
            let (title, detail) = TrayItem::compose(&raw_item.name);
            let (reported_x, reported_y) = raw_item.center();
            let (center_x, center_y) = if aligned {
                cells[position]
            } else {
                (reported_x - rect.left, reported_y - rect.top)
            };

            let matched = registry::best_match(&entries, &raw_item.name);

            // The shell keeps a PNG snapshot of every icon it has seen, with the
            // icon's real alpha. That beats cropping a cell out of a rendering of
            // the flyout, so it is used whenever there is one; the crop covers the
            // rest.
            let (icon, icon_background) = match matched
                .as_ref()
                .and_then(|entry| entry.snapshot.as_deref())
                .filter(|png| !png.is_empty())
            {
                Some(png) => (capture::png_bytes_data_url(png), "transparent".to_string()),
                None => match &bitmap {
                    Some(bitmap) => {
                        let left = center_x - icon_size / 2;
                        let top = center_y - icon_size / 2;
                        let (rgba, background) = capture::icon_rgba(bitmap, left, top, icon_size);
                        let png = capture::png_data_url(icon_size as u32, icon_size as u32, &rgba)
                            .unwrap_or_default();
                        (
                            png,
                            format!("rgb({}, {}, {})", background[0], background[1], background[2]),
                        )
                    }
                    None => (String::new(), "transparent".to_string()),
                },
            };

            TrayItem {
                index: 0,
                tooltip: raw_item.name.clone(),
                title,
                detail,
                icon,
                icon_background,
                x: rect.left + center_x,
                y: rect.top + center_y,
                registry_key: matched.as_ref().map(|entry| entry.key.clone()),
                promoted: matched
                    .as_ref()
                    .map(|entry| entry.promoted)
                    .unwrap_or(false),
                host_key: None,
            }
        })
        .collect();

    apply_order(&mut items, prefs);

    // The click replay works from absolute coordinates, so renumbering here only
    // affects what the UI shows.
    for (index, item) in items.iter_mut().enumerate() {
        item.index = index;
    }
    items
}

/// Orders the rows the way the preferences ask for. Split out because changing
/// the sort order must not require a new flyout read.
pub fn apply_order(items: &mut [TrayItem], prefs: &Prefs) {
    match prefs.sort {
        SortMode::Name => items.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase())),
        SortMode::Tray => {
            if prefs.pinned_first {
                items.sort_by_key(|item| !item.promoted);
            }
        }
    }

    for (index, item) in items.iter_mut().enumerate() {
        item.index = index;
    }
}

fn publish_host(
    app: &AppHandle,
    shared: &Arc<Shared>,
    reader: Option<&uia::Reader>,
    session: &Session,
    opening: bool,
) {
    let prefs = app.state::<AppState>().prefs();
    let items = items_from_host(&session.icons, &prefs);
    let anchor = if opening {
        panel_anchor(reader)
    } else {
        shared.flyout().unwrap_or_else(|| panel_anchor(reader))
    };
    let previous = shared.items().len();
    let count = items.len();
    shared.set_items(items.clone(), 0, anchor);
    publish(app, items, None, true, opening);
    if opening {
        overlay::show(app, anchor, count, &prefs);
        shared.set_visible(true);
    } else if count != previous {
        overlay::relayout(app, anchor, count, &prefs);
    }
}

/// Overflow icons from the host mirror. Promoted ones stay on the taskbar.
fn items_from_host(icons: &[host::HostIcon], prefs: &Prefs) -> Vec<TrayItem> {
    let entries = registry::entries();
    let mut exe_of: std::collections::HashMap<isize, String> = std::collections::HashMap::new();
    let icons = listed_icons(icons, &mut exe_of);
    let mut items: Vec<TrayItem> = icons
        .into_iter()
        .filter_map(|icon| {
            let mut matched = registry::match_icon(&entries, &icon.key, &icon.tip);
            if matched.is_none() && icon.tip.trim().is_empty() {
                if let Some(exe) = exe_of.get(&icon.hwnd) {
                    matched = registry::match_executable(&entries, exe);
                }
            }
            let promoted = matched.as_ref().map(|entry| entry.promoted).unwrap_or(false);
            if promoted {
                return None;
            }
            // An empty szTip is not "no name". The shell keeps the last tooltip
            // and otherwise shows InitialTooltip from NotifyIconSettings.
            let tip = if icon.tip.trim().is_empty() {
                matched
                    .as_ref()
                    .and_then(|entry| entry.tooltip.clone())
                    .filter(|stored| !stored.trim().is_empty())
                    .unwrap_or_default()
            } else {
                icon.tip.clone()
            };
            let (title, detail) = TrayItem::compose(&tip);
            let icon_url = if !icon.icon.is_empty() {
                icon.icon.clone()
            } else {
                matched
                    .as_ref()
                    .and_then(|entry| entry.snapshot.as_deref())
                    .filter(|png| !png.is_empty())
                    .map(capture::png_bytes_data_url)
                    .unwrap_or_default()
            };
            Some(TrayItem {
                index: 0,
                tooltip: tip,
                title,
                detail,
                icon: icon_url,
                icon_background: "transparent".to_string(),
                x: 0,
                y: 0,
                registry_key: matched.as_ref().map(|entry| entry.key.clone()),
                promoted: false,
                host_key: Some(icon.key.clone()),
            })
        })
        .collect();
    apply_order(&mut items, prefs);
    items
}

/// Live overflow rows. A killed process often never sends `NIM_DELETE`, and a
/// modify without a GUID can sit beside the GUID row for the same icon.
fn listed_icons<'a>(
    icons: &'a [host::HostIcon],
    exe_of: &mut std::collections::HashMap<isize, String>,
) -> Vec<&'a host::HostIcon> {
    let mut ranked: Vec<&host::HostIcon> = Vec::new();
    let mut slot_of: std::collections::HashMap<(isize, u32), usize> = std::collections::HashMap::new();
    for icon in icons {
        if icon.hidden || !icon.window_alive() {
            continue;
        }
        let slot = (icon.hwnd, icon.id);
        if let Some(&index) = slot_of.get(&slot) {
            if richer(icon, ranked[index]) {
                ranked[index] = icon;
            }
        } else {
            slot_of.insert(slot, ranked.len());
            ranked.push(icon);
        }
    }

    let mut newest: std::collections::HashMap<(String, String), isize> = std::collections::HashMap::new();
    for icon in &ranked {
        let tip = icon.tip.trim();
        if tip.is_empty() {
            continue;
        }
        let exe = exe_of
            .entry(icon.hwnd)
            .or_insert_with(|| host::owner_exe(icon.hwnd).unwrap_or_default())
            .to_ascii_lowercase();
        if exe.is_empty() {
            continue;
        }
        newest
            .entry((exe, tip.to_ascii_lowercase()))
            .and_modify(|hwnd| {
                if icon.hwnd > *hwnd {
                    *hwnd = icon.hwnd;
                }
            })
            .or_insert(icon.hwnd);
    }

    ranked
        .into_iter()
        .filter(|icon| {
            let tip = icon.tip.trim();
            if tip.is_empty() {
                return true;
            }
            let Some(exe) = exe_of.get(&icon.hwnd) else {
                return true;
            };
            if exe.is_empty() {
                return true;
            }
            newest.get(&(exe.to_ascii_lowercase(), tip.to_ascii_lowercase())) == Some(&icon.hwnd)
        })
        .collect()
}

fn richer(candidate: &host::HostIcon, current: &host::HostIcon) -> bool {
    let score = |icon: &host::HostIcon| {
        (
            !icon.tip.trim().is_empty() as u8,
            icon.key.starts_with("guid:") as u8,
            (icon.callback != 0) as u8,
        )
    };
    score(candidate) > score(current)
}

/// A one-pixel-tall anchor centred on the chevron, sitting on the taskbar top.
/// The panel is placed from that the same way it used to be placed from the flyout.
fn panel_anchor(reader: Option<&uia::Reader>) -> island::Rect {
    let Some(taskbar) = island::find_by_class(island::TASKBAR_CLASS) else {
        return island::Rect {
            left: 0,
            top: 0,
            right: 1,
            bottom: 1,
        };
    };
    let taskbar_rect = island::rect(taskbar).unwrap_or(island::Rect {
        left: 0,
        top: 0,
        right: 1,
        bottom: 1,
    });
    let centre = reader
        .and_then(|reader| reader.chevron(island::raw(taskbar)).ok().flatten())
        .and_then(|chevron| chevron.get_bounding_rectangle().ok())
        .map(|bounds| (bounds.get_left() + bounds.get_right()) / 2)
        .unwrap_or(taskbar_rect.right - 48);
    island::Rect {
        left: centre,
        right: centre + 1,
        top: taskbar_rect.top - 1,
        bottom: taskbar_rect.top,
    }
}

fn publish(app: &AppHandle, items: Vec<TrayItem>, error: Option<Fault>, direct: bool, opening: bool) {
    if let Some(state) = app.try_state::<AppState>() {
        state.shared.set_direct(direct);
    }
    let payload = TrayList {
        items,
        error,
        source: shell_build(),
        // A published list from a read is an opening. A host update of an
        // already open panel is not, so the filter the user is typing stays.
        opening,
        direct,
    };
    let _ = app.emit(EVENT_LIST, payload);
    // The panel is about to open, which is the only moment its colour scheme
    // matters, so the shell's answer travels with the list instead of needing a
    // watcher of its own.
    let _ = app.emit(EVENT_THEME, crate::win::theme::is_dark());
}

fn publish_error(app: &AppHandle, fault: Fault) {
    eprintln!("TrayList: {fault}");
    publish(app, Vec::new(), Some(fault), false, true);
}

/// The shell build the list was read from. Cheap to cache, and genuinely useful
/// in a bug report about a flyout that changed shape.
///
/// Only the number, without any wording: the panel is the one that knows which
/// language to put around it, and an empty string means the shell would not say.
pub fn shell_build() -> String {
    static BUILD: OnceLock<String> = OnceLock::new();
    BUILD
        .get_or_init(|| {
            use winreg::enums::HKEY_LOCAL_MACHINE;
            winreg::RegKey::predef(HKEY_LOCAL_MACHINE)
                .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
                .and_then(|key| key.get_value::<String, _>("CurrentBuildNumber"))
                .unwrap_or_default()
        })
        .clone()
}