//! Reading the overflow flyout's icons through UI Automation.
//!
//! The shell exposes each notification icon as a button whose `AutomationId` is
//! `NotifyItemIcon`, and — this is the useful part — its `Name` is the icon's
//! full tooltip. That is where the human-readable label comes from.
//!
//! UI Automation only sees the icons while the flyout is on screen, so every
//! read has to happen in the moment between "the user clicked the chevron" and
//! "we hide the window".

use std::time::{Duration, Instant};

use uiautomation::core::UICondition;
use uiautomation::patterns::UIInvokePattern;
use uiautomation::types::{Handle, TreeScope, UIProperty};
use uiautomation::variants::Variant;
use uiautomation::{UIAutomation, UIElement};

/// AutomationId shared by every notification icon button in the flyout.
pub const NOTIFY_ICON_ID: &str = "NotifyItemIcon";
/// AutomationId of the chevron, which toggles the flyout.
pub const CHEVRON_ID: &str = "SystemTrayIcon";
/// Only the chevron carries this class alongside the chevron AutomationId; the
/// clock, volume, network and battery buttons reuse the id with other classes.
pub const TRAY_BUTTON_CLASS: &str = "SystemTray.NormalButton";

#[derive(Debug, Clone)]
pub struct RawItem {
    pub name: String,
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// Whether two reads list the same icons in the same order.
///
/// Only the names and the order are compared. The rectangles are deliberately not
/// part of this: they keep moving while the flyout animates, and nothing depends
/// on them any more — every icon's position is measured from the drawn grid. So
/// waiting for stable rectangles would only add delay.
fn same_icons(before: &[RawItem], after: &[RawItem]) -> bool {
    before.len() == after.len()
        && before
            .iter()
            .zip(after)
            .all(|(one, other)| one.name == other.name)
}

impl RawItem {
    pub fn center(&self) -> (i32, i32) {
        (
            self.left + (self.right - self.left) / 2,
            self.top + (self.bottom - self.top) / 2,
        )
    }
}

pub struct Reader {
    automation: UIAutomation,
}

impl Reader {
    /// `UIAutomation::new` also initialises COM for this thread as
    /// multi-threaded, which is why the reader is created inside the watcher
    /// thread rather than shared.
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            automation: UIAutomation::new()?,
        })
    }

    fn element_for(&self, hwnd_raw: isize) -> anyhow::Result<UIElement> {
        Ok(self.automation.element_from_handle(Handle::from(hwnd_raw))?)
    }

    fn condition(&self, property: UIProperty, value: &str) -> anyhow::Result<UICondition> {
        Ok(self
            .automation
            .create_property_condition(property, Variant::from(value), None)?)
    }

    /// Every notification icon button inside the flyout, in the order the shell
    /// lays them out.
    pub fn items(&self, island_hwnd: isize) -> anyhow::Result<Vec<RawItem>> {
        Ok(self.items_with_pending(island_hwnd)?.0)
    }

    /// The icons that are laid out, plus how many were not.
    ///
    /// While the flyout animates open the shell already exposes every icon, but
    /// the ones still waiting their turn report an empty rectangle. Those have to
    /// be counted rather than trusted: an empty rectangle sits at the origin, so
    /// using one would sort it to the front of the list and crop whatever happens
    /// to be in the flyout's top-left corner.
    fn items_with_pending(&self, island_hwnd: isize) -> anyhow::Result<(Vec<RawItem>, usize)> {
        let root = self.element_for(island_hwnd)?;
        let condition = self.condition(UIProperty::AutomationId, NOTIFY_ICON_ID)?;
        let elements = root.find_all(TreeScope::Descendants, &condition)?;

        let mut out = Vec::with_capacity(elements.len());
        let mut pending = 0usize;
        for element in elements {
            let Ok(rect) = element.get_bounding_rectangle() else {
                pending += 1;
                continue;
            };
            if rect.get_right() <= rect.get_left() || rect.get_bottom() <= rect.get_top() {
                pending += 1;
                continue;
            }
            out.push(RawItem {
                name: element.get_name().unwrap_or_default(),
                // The class name is deliberately not read: every icon in the
                // flyout is a `SystemTray.NormalButton`, and asking for it costs
                // one more cross-process call per icon on the path that decides
                // how long the panel takes to appear.
                left: rect.get_left(),
                top: rect.get_top(),
                right: rect.get_right(),
                bottom: rect.get_bottom(),
            });
        }

        // The shell lists the icons in the order it draws them, which is the
        // order they need to be shown in. Their rectangles are deliberately not
        // used for ordering: they can still be moving while the flyout animates.
        Ok((out, pending))
    }

    /// Waits until the icon list is complete.
    ///
    /// The flyout animates in, and the icons appear one after another, so a read
    /// taken the moment the window appears can still be missing some. Two reads
    /// in a row that both list every icon and agree on the order are good enough:
    /// where the icons are is measured from the drawing, not from these
    /// rectangles, so there is nothing to wait for beyond the list itself.
    pub fn items_settled(
        &self,
        island_hwnd: isize,
        budget: Duration,
    ) -> anyhow::Result<Vec<RawItem>> {
        let deadline = Instant::now() + budget;
        let mut best: Vec<RawItem> = Vec::new();
        let mut best_pending = usize::MAX;
        let mut previous: Option<Vec<RawItem>> = None;

        loop {
            let (items, pending) = self.items_with_pending(island_hwnd).unwrap_or_default();
            let laid_out = items.len();

            // Prefer a read with nothing left unplaced, and the fullest one after
            // that, so a budget that runs out still yields the best attempt.
            if pending < best_pending || (pending == best_pending && laid_out > best.len()) {
                best = items.clone();
                best_pending = pending;
            }

            let settled = pending == 0
                && previous
                    .as_ref()
                    .map(|before| same_icons(before, &items))
                    .unwrap_or(false);
            if settled {
                break;
            }
            previous = Some(items);

            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        Ok(best)
    }

    /// The chevron that opens the flyout, found by requiring both the tray
    /// button class and the chevron's AutomationId so no other tray button can
    /// ever be invoked by mistake.
    pub fn chevron(&self, taskbar_hwnd: isize) -> anyhow::Result<Option<UIElement>> {
        let root = self.element_for(taskbar_hwnd)?;
        let condition = self.condition(UIProperty::AutomationId, CHEVRON_ID)?;
        let candidates = root.find_all(TreeScope::Descendants, &condition)?;
        for element in candidates {
            if element.get_classname().unwrap_or_default() == TRAY_BUTTON_CLASS {
                return Ok(Some(element));
            }
        }
        Ok(None)
    }

    /// Toggles the flyout by invoking the chevron.
    pub fn invoke(&self, element: &UIElement) -> anyhow::Result<()> {
        let pattern: UIInvokePattern = element.get_pattern()?;
        pattern.invoke()?;
        Ok(())
    }
}