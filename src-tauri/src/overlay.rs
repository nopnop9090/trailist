//! Sizing and placing the panel.
//!
//! The window is deliberately larger than the visible panel so the drop shadow
//! has somewhere to live; the rounded card is drawn by the stylesheet inside
//! that margin. Sizes below are the stylesheet's logical pixels, converted to
//! physical ones with the flyout monitor's DPI so both sides agree.

use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

use crate::types::Prefs;
use crate::win::island;

pub const WINDOW: &str = "overlay";
pub const TITLE: &str = "TrayList";

/// Row height, fixed chrome around the list and the shadow margin. These must
/// match the stylesheet, which is why the frontend keeps the same numbers.
pub const ROW_HEIGHT: i32 = 38;
pub const CHROME: i32 = 104;
pub const SHADOW: i32 = 22;
pub const MIN_HEIGHT: i32 = 180;

pub fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(WINDOW)
}

/// Anchors the panel to the tray corner.
///
/// The anchor is the flyout when that window is actually beside a taskbar, and
/// the chevron otherwise. An anchor at the origin — a window that has not been
/// placed yet — is refused, because that is what put the list in the top-left
/// corner of the screen.
///
/// Horizontally the panel is centred on that anchor, which is the chevron.
/// Vertically it rests against the same edge of the work area the taskbar
/// occupies. `prefs.edge_gap`, zero by default, is air between the panel and
/// the shell.
pub fn geometry(
    flyout: island::Rect,
    count: usize,
    prefs: &Prefs,
) -> (PhysicalPosition<i32>, PhysicalSize<i32>) {
    let flyout = if island::beside_taskbar(flyout) {
        flyout
    } else {
        island::tray_anchor(None)
    };
    let scale = island::dpi_scale(island::hwnd(0)).max(1.0);
    let px = |logical: i32| ((logical as f64) * scale).round() as i32;

    let gap = px(prefs.edge_gap.clamp(0, 120));
    let shadow = px(SHADOW);

    let work = island::work_area_for_point(
        flyout.left + flyout.width() / 2,
        flyout.top + flyout.height() / 2,
    )
    .unwrap_or(island::Rect {
        left: 0,
        top: 0,
        right: flyout.right.max(1),
        bottom: flyout.bottom.max(1),
    });

    let panel_width = px(prefs.panel_width).min((work.width() - 2 * gap).max(px(240)));
    let content = px(CHROME + (count as i32) * ROW_HEIGHT);
    let panel_height = content
        .min(px(prefs.panel_max_height))
        .min((work.height() - 2 * gap).max(px(MIN_HEIGHT)))
        .max(px(MIN_HEIGHT));

    let width = panel_width + 2 * shadow;
    let height = panel_height + 2 * shadow;
    let (x, y) = hang(flyout, work, width, height, gap);

    (PhysicalPosition::new(x, y), PhysicalSize::new(width, height))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Edge {
    Left,
    Top,
    Right,
    Bottom,
}

/// Which side of the work area the anchor is leaning on.
fn nearest_edge(anchor: island::Rect, work: island::Rect) -> Edge {
    let choices = [
        (Edge::Bottom, (anchor.bottom - work.bottom).abs()),
        (Edge::Top, (anchor.top - work.top).abs()),
        (Edge::Left, (anchor.left - work.left).abs()),
        (Edge::Right, (anchor.right - work.right).abs()),
    ];
    choices
        .into_iter()
        .min_by_key(|(_, distance)| *distance)
        .map(|(edge, _)| edge)
        .unwrap_or(Edge::Bottom)
}

/// The window's outer position, before it is clamped into the work area.
///
/// The bottom edge is the usual one: the anchor's bottom is the top of the
/// taskbar, and the transparent margin under the card has to end there. An
/// overlap would swallow clicks meant for the taskbar, because this window is
/// always-on-top and not click-through.
fn hang(anchor: island::Rect, work: island::Rect, width: i32, height: i32, gap: i32) -> (i32, i32) {
    let centre_x = anchor.left + anchor.width() / 2;
    let centre_y = anchor.top + anchor.height() / 2;
    let (mut x, mut y) = match nearest_edge(anchor, work) {
        Edge::Bottom => (centre_x - width / 2, anchor.bottom - gap - height),
        Edge::Top => (centre_x - width / 2, anchor.top + gap),
        Edge::Left => (anchor.right + gap, centre_y - height / 2),
        Edge::Right => (anchor.left - gap - width, centre_y - height / 2),
    };
    x = x.clamp(
        work.left + gap,
        (work.right - width - gap).max(work.left + gap),
    );
    y = y.clamp(
        work.top + gap,
        (work.bottom - height - gap).max(work.top + gap),
    );
    (x, y)
}

/// Sizes and places the panel without showing it, and hands back what it used.
///
/// Split out from `show` so that a settings change can be applied to a panel that
/// is already on screen. The height limit and the gap to the taskbar then behave
/// like what they are — live controls — instead of only taking effect on the next
/// opening.
pub fn place(
    window: &WebviewWindow,
    flyout: island::Rect,
    count: usize,
    prefs: &Prefs,
) -> (PhysicalPosition<i32>, PhysicalSize<i32>) {
    let (position, size) = geometry(flyout, count, prefs);
    let _ = window.set_size(size);
    let _ = window.set_position(position);
    // The webview's move is posted to its own thread and can be dropped, which
    // leaves the window where it was created: the top-left of the screen.
    if let Ok(handle) = window.hwnd() {
        island::pin(
            island::hwnd(handle.0 as isize),
            position.x,
            position.y,
            size.width,
            size.height,
        );
    }
    (position, size)
}

/// Re-places the panel after a settings change, if it is on screen.
///
/// Quiet when there is no window: the same call happens whether or not the panel
/// is open, and that is not worth an error.
pub fn relayout(app: &AppHandle, flyout: island::Rect, count: usize, prefs: &Prefs) {
    let Some(window) = window(app) else {
        return;
    };
    let (position, size) = place(&window, flyout, count, prefs);
    crate::trace!(
        "overlay: re-placed to {}x{} at {},{} (gap {})",
        size.width,
        size.height,
        position.x,
        position.y,
        prefs.edge_gap
    );
}

/// Places, sizes and reveals the panel, then does its best to give it the
/// keyboard. Failing to take the foreground is survivable: the list still works
/// with the mouse and the chevron still closes it.
pub fn show(app: &AppHandle, flyout: island::Rect, count: usize, prefs: &Prefs) {
    let Some(window) = window(app) else {
        return;
    };
    let (position, size) = place(&window, flyout, count, prefs);
    crate::trace!(
        "overlay: flyout {}x{} at {},{}, panel {}x{} at {},{}",
        flyout.width(),
        flyout.height(),
        flyout.left,
        flyout.top,
        size.width,
        size.height,
        position.x,
        position.y
    );

    let _ = window.show();
    let _ = window.set_focus();
    note_shown();
    // Timestamped because the wait before this point is the one the user feels.
    crate::trace!("overlay: panel is on screen");

    // Tauri's own focus call does not survive Windows' foreground rules when the
    // click that opened us went to the taskbar, so insist. The second attempt
    // covers the auto-hide taskbar sliding away again, which otherwise hands the
    // foreground back to whatever had it before us.
    for attempt in 0..2 {
        if attempt > 0 {
            std::thread::sleep(FOCUS_RETRY);
        }
        match island::find_by_title(TITLE) {
            Some(handle) => {
                let held = crate::win::focus::is_foreground(handle);
                crate::win::focus::raise(handle);
                crate::trace!("overlay: focus attempt {attempt}, already held: {held}");
                if held {
                    break;
                }
            }
            None => crate::trace!("overlay: own window not found by title yet"),
        }
    }
    // Posted after `show`, so it runs once the window is visible. A show that
    // restored the creation position is corrected on the thread that owns it.
    let raw = window.hwnd().ok().map(|handle| handle.0 as isize);
    let x = position.x;
    let y = position.y;
    let width = size.width;
    let height = size.height;
    let _ = window.run_on_main_thread(move || {
        if let Some(raw) = raw {
            island::pin(island::hwnd(raw), x, y, width, height);
        }
    });
}

pub fn hide(app: &AppHandle) {
    if let Some(window) = window(app) {
        let _ = window.hide();
    }
}

/// Paints the panel once, off screen, so the first real opening is instant.
///
/// WebView2 only finishes its first paint after its window has been shown. Doing
/// that at minus thirty-two thousand pixels means nobody sees it, and it costs
/// one round of startup instead of a visible blank panel on the first opening.
pub fn warm_up(app: &AppHandle) {
    let Some(window) = window(app) else {
        return;
    };
    let _ = window.set_position(PhysicalPosition::new(-32_000, -32_000));
    if window.show().is_ok() {
        std::thread::sleep(std::time::Duration::from_millis(250));
        let _ = window.hide();
    }
    crate::trace!("overlay: warmed up");
}

/// When the panel went up, and whether it ever actually held the keyboard.
///
/// Both are needed because Windows sends a blur as soon as a window is revealed
/// but not yet activated. Without the grace period, showing the panel would
/// dismiss it again within the same frame.
static SHOWN_AT: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
static HAD_FOCUS: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

const SETTLE: std::time::Duration = std::time::Duration::from_millis(300);

/// How long to wait before insisting on the foreground a second time. Long enough
/// for an auto-hide taskbar to finish sliding away.
const FOCUS_RETRY: std::time::Duration = std::time::Duration::from_millis(180);

fn note_shown() {
    *SHOWN_AT.lock().unwrap() = Some(std::time::Instant::now());
    HAD_FOCUS.store(false, std::sync::atomic::Ordering::SeqCst);
}

/// Called when the panel actually receives the keyboard.
pub fn note_focus_gained() {
    HAD_FOCUS.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Whether a focus loss means "the user clicked somewhere else" rather than the
/// panel still settling into place.
pub fn is_dismissable() -> bool {
    let settled = SHOWN_AT
        .lock()
        .ok()
        .and_then(|shown| *shown)
        .map(|shown| shown.elapsed() > SETTLE)
        .unwrap_or(true);
    settled && HAD_FOCUS.load(std::sync::atomic::Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::{hang, Edge, nearest_edge};
    use crate::win::island::Rect;

    fn work() -> Rect {
        Rect {
            left: 0,
            top: 0,
            right: 5120,
            bottom: 1392,
        }
    }

    #[test]
    fn a_chevron_anchor_hangs_off_the_bottom_right() {
        let anchor = Rect {
            left: 4867,
            top: 1391,
            right: 4868,
            bottom: 1392,
        };
        assert_eq!(nearest_edge(anchor, work()), Edge::Bottom);
        let (x, y) = hang(anchor, work(), 400, 500, 0);
        assert!(x > 4000, "x={x}");
        assert_eq!(y, 1392 - 500);
    }

    #[test]
    fn the_origin_would_hang_off_the_top_if_it_were_accepted() {
        let origin = Rect {
            left: 0,
            top: 0,
            right: 1,
            bottom: 1,
        };
        assert_eq!(nearest_edge(origin, work()), Edge::Top);
        let (x, y) = hang(origin, work(), 400, 500, 0);
        assert_eq!((x, y), (0, 0));
    }
}