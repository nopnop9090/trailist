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
/// Two things decide where it lands. Horizontally it is *centred on the flyout*,
/// and the shell centres the flyout on the chevron — so the panel sits centred
/// under the button the user actually clicked, rather than being pushed to the
/// right edge of the screen by its own width. Vertically the window's bottom edge
/// lands on the flyout's bottom, which is the top of the taskbar, so the panel
/// rests against the taskbar the way the native flyout does.
///
/// The one remaining offset is `prefs.edge_gap`, zero by default: anything larger
/// is the user asking for air between the panel and the shell.
pub fn geometry(
    flyout: island::Rect,
    count: usize,
    prefs: &Prefs,
) -> (PhysicalPosition<i32>, PhysicalSize<i32>) {
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
        right: flyout.right,
        bottom: flyout.bottom,
    });

    let panel_width = px(prefs.panel_width).min((work.width() - 2 * gap).max(px(240)));
    let content = px(CHROME + (count as i32) * ROW_HEIGHT);
    let panel_height = content
        .min(px(prefs.panel_max_height))
        .min((work.height() - 2 * gap).max(px(MIN_HEIGHT)))
        .max(px(MIN_HEIGHT));

    let width = panel_width + 2 * shadow;
    let height = panel_height + 2 * shadow;

    let mut x = flyout.left + flyout.width() / 2 - width / 2;
    // The transparent margin under the card has to end at the taskbar, not on top
    // of it: this window is always-on-top and not click-through, so an overlap
    // there would swallow clicks meant for the taskbar.
    let mut y = flyout.bottom - gap - height;

    x = x.clamp(
        work.left + gap,
        (work.right - width - gap).max(work.left + gap),
    );
    y = y.clamp(
        work.top + gap,
        (work.bottom - height - gap).max(work.top + gap),
    );

    (PhysicalPosition::new(x, y), PhysicalSize::new(width, height))
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