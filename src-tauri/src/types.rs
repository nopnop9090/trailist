//! What crosses the bridge: the list the overlay renders and the preferences
//! that shape it.

use serde::{Deserialize, Serialize};

/// One icon in the flyout, resolved far enough to render a row.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayItem {
    /// Position in the flyout's own order, which is also what a click replays.
    pub index: usize,
    /// The complete tooltip, which is the closest thing to a name the shell has.
    pub tooltip: String,
    /// First line of the tooltip: usually the application name.
    pub title: String,
    /// Everything after the first line: usually the current state.
    pub detail: String,
    /// PNG data URL of the icon as it looked in the flyout.
    pub icon: String,
    /// The flyout colour the icon was keyed against, painted behind it so pale
    /// icons and anti-aliased edges blend instead of showing a halo.
    pub icon_background: String,
    /// Where the icon sat on screen, so a click can be aimed at it again.
    pub x: i32,
    pub y: i32,
    /// The icon's registry key, when it could be matched, which is what makes
    /// "always show in the tray" possible for it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry_key: Option<String>,
    /// Whether the icon is currently in the visible part of the tray.
    pub promoted: bool,
    /// The host's registration key, when the list came from the in-explorer mirror.
    /// A click names this instead of a pixel in the stock flyout.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_key: Option<String>,
}

impl TrayItem {
    /// Splits a tooltip into its name and its state.
    ///
    /// Three shapes turn up in practice, and all three are handled here so the
    /// frontend never has to guess:
    ///
    /// - two lines, the second often repeating the name: `Cline` / `Cline - 1 session`
    /// - one line with a colon: `CapsLock: shift`
    /// - one line with the name repeated instead: `Cline Cline - 1 session`
    pub fn compose(tooltip: &str) -> (String, String) {
        let cleaned = tooltip.trim();
        let mut lines = cleaned
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty());
        let first = lines.next().unwrap_or_default().to_string();
        let rest: Vec<&str> = lines.collect();

        if !rest.is_empty() {
            return (first.clone(), without_repeat(&first, &rest.join(" ")));
        }

        // The colon is unambiguous, so it wins: a state spelled out after one is
        // not a repeat of the name.
        match first.split_once(AML_COLON) {
            Some((head, tail)) if head.chars().count() >= 4 => {
                (head.to_string(), tail.to_string())
            }
            _ => match repeated_prefix(&first) {
                Some((name, state)) => (name, state),
                None => (first, String::new()),
            },
        }
    }
}

/// Splits `Name Name state` into `Name` and `state`.
///
/// Some applications put the name into their own tooltip and the state after it,
/// which leaves the row reading `TrayMaster TrayMaster - 3/3 running`. Only the
/// repetition is wrong there, so only the repetition is removed.
///
/// The longest repeated run of leading words wins, so that
/// `Visual Syslog Server Visual Syslog Server 1.6.4` loses the whole name rather
/// than the first word of it.
fn repeated_prefix(text: &str) -> Option<(String, String)> {
    let words = words_with_offsets(text);

    for length in (1..=words.len() / 2).rev() {
        let first = &words[..length];
        let again = &words[length..length * 2];
        let same = first
            .iter()
            .zip(again)
            .all(|((_, one), (_, other))| one.eq_ignore_ascii_case(other));
        if !same {
            continue;
        }

        let name = first
            .iter()
            .map(|(_, word)| *word)
            .collect::<Vec<_>>()
            .join(" ");
        // From the start of the repeated run onwards is the state, taken from the
        // original text so the spacing stays as the application wrote it. The
        // repeat can also be the whole tooltip, in which case nothing is left.
        let state = match words.get(length * 2) {
            Some((offset, _)) => text[*offset..].trim_start(),
            None => "",
        };
        let state = state.trim_start_matches(SEPARATORS).trim_start();
        return Some((name, state.to_string()));
    }

    None
}

/// Every word with the byte offset it starts at.
///
/// Offsets rather than slices, because the state has to be taken out of the
/// original string: rebuilding it from words would quietly normalise the spacing
/// an application chose.
fn words_with_offsets(text: &str) -> Vec<(usize, &str)> {
    let mut words = Vec::new();
    let mut start: Option<usize> = None;

    for (index, character) in text.char_indices() {
        match (character.is_whitespace(), start) {
            (false, None) => start = Some(index),
            (true, Some(from)) => {
                words.push((from, &text[from..index]));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        words.push((from, &text[from..]));
    }

    words
}

/// Removes a leading repeat of the name from the state text.
///
/// Plenty of applications format their tooltip as the name on one line and
/// "name - state" on the next, so the row would otherwise say the same word
/// twice: `TrayMaster` over `TrayMaster - 3/3 running`. The separator left behind
/// by the removed name is not information either, so it goes too.
fn without_repeat(name: &str, detail: &str) -> String {
    if name.is_empty() {
        return detail.to_string();
    }

    // `get` rather than slicing, because a byte boundary can land inside a
    // character the name does not have.
    let Some(head) = detail.get(..name.len()) else {
        return detail.to_string();
    };
    if !head.eq_ignore_ascii_case(name) {
        return detail.to_string();
    }

    detail[head.len()..]
        .trim_start()
        .trim_start_matches(SEPARATORS)
        .trim_start()
        .to_string()
}

/// The separator a tooltip uses between a name and its state. Written as an
/// escape because a literal colon in the source is easy to misread.
const AML_COLON: &str = ":\u{20}";

/// What a name leaves behind when it is taken off the front of a state: the
/// separators applications use between the two, in the order they turn up.
const SEPARATORS: [char; 6] = ['-', '\u{2013}', '\u{2014}', ':', '|', '\u{00b7}'];

/// Something that went wrong, in a shape the panel can put into words.
///
/// The sentence is written in the panel, so a code travels instead of a finished
/// German one: an error in German inside an English panel would give the lie to
/// the language setting. `detail` is whatever Windows or the registry said, which
/// is in the language Windows itself is installed in, and is passed through as it
/// came.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fault {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl Fault {
    /// A failure that needs nothing beyond its own code.
    pub fn new(code: &str) -> Self {
        Self {
            code: code.to_string(),
            detail: None,
        }
    }

    /// A failure with what the system underneath said about it.
    pub fn with(code: &str, detail: impl std::fmt::Display) -> Self {
        Self {
            code: code.to_string(),
            detail: Some(detail.to_string()),
        }
    }
}

/// How a fault reads in the log, where there is no language to choose from.
impl std::fmt::Display for Fault {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.detail {
            Some(detail) => write!(formatter, "{}: {detail}", self.code),
            None => write!(formatter, "{}", self.code),
        }
    }
}

impl std::error::Error for Fault {}

/// The snapshot the overlay draws.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayList {
    pub items: Vec<TrayItem>,
    /// Set when the read failed, so the UI can say so instead of looking empty.
    pub error: Option<Fault>,
    /// Which shell build the shape of the flyout was read from, useful in bug
    /// reports.
    pub source: String,
    /// Whether this is a fresh read of the flyout, i.e. the panel just opened,
    /// rather than the same list being re-sent after a settings change.
    ///
    /// The panel resets its filter and reaches for the keyboard only on a fresh
    /// one: doing that after a sort-order change would wipe a filter the user is
    /// still typing and pull the focus out of the settings dialog.
    pub opening: bool,
    /// Whether the list came from the in-explorer host. When this is false the
    /// panel is the UI Automation fallback, and a click is only a best effort.
    pub direct: bool,
}

/// User-tunable knobs, stored next to the executable so the whole thing stays
/// portable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Prefs {
    /// Panel width in logical pixels; the native flyout is only about 234 wide.
    pub panel_width: i32,
    /// Upper bound for the panel height. The list scrolls past it.
    ///
    /// The default is deliberately generous: on a large screen there is no reason
    /// to scroll through a list that would have fitted.
    pub panel_max_height: i32,
    /// Breathing room between the panel and the taskbar, in logical pixels. Zero
    /// puts the panel flush against the shell, which is what the native flyout
    /// does; anything larger pulls it up and away.
    pub edge_gap: i32,
    /// Size of the icon drawn in each row.
    pub icon_size: i32,
    /// How the rows are ordered.
    pub sort: SortMode,
    /// Show the filter box at the top of the panel.
    pub show_search: bool,
    /// Ask for the whole list on a global hotkey, without the chevron.
    pub hotkey: Option<String>,
    /// Order pinned icons first.
    pub pinned_first: bool,
    /// Which language the panel speaks: `de`, `en`, or `system` for whatever the
    /// shell is set to. Anything unrecognised counts as `system`.
    pub lang: String,
}

impl Prefs {
    /// The hotkey shipped as the default: awkward enough that it is unlikely to
    /// already belong to something else, short enough to actually use.
    pub const DEFAULT_HOTKEY: &'static str = "Alt+Shift+T";
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            panel_width: 344,
            panel_max_height: 900,
            edge_gap: 0,
            icon_size: 16,
            sort: SortMode::Tray,
            show_search: true,
            hotkey: Some(Self::DEFAULT_HOTKEY.to_string()),
            pinned_first: false,
            lang: "system".to_string(),
        }
    }
}

/// How the rows are ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SortMode {
    /// The order the shell lays the flyout out in, which is the layout the user
    /// already has in their head.
    Tray,
    /// Alphabetical by name, for when hunting for one app.
    Name,
}

/// What the panel says about itself.
///
/// The build stamp cannot be worked out at runtime — the running exe does not
/// know when it was compiled — so it is baked in by `build.rs` and read back from
/// the environment here.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct About {
    /// Crate version, e.g. `0.2.0`.
    pub version: String,
    /// When this build was made, as `YYYY-MM-DD HH:MM` local time.
    pub built: String,
    /// The shell build the list is read from, which is what a bug report needs.
    pub shell: String,
}

#[cfg(test)]
mod tests {
    use super::TrayItem;

    /// Every tooltip here is one this machine's tray actually produced, which is
    /// where the three shapes come from.
    #[test]
    fn splits_tooltips_the_way_the_tray_writes_them() {
        let cases = [
            // The name repeated on one line, which several applications do.
            (
                "Cline Cline \u{2014} 1 session running",
                "Cline",
                "1 session running",
            ),
            (
                "TrayMaster TrayMaster - 3/3 running",
                "TrayMaster",
                "3/3 running",
            ),
            // The longest repeated run wins, so the whole name comes off.
            (
                "Visual Syslog Server Visual Syslog Server 1.6.4",
                "Visual Syslog Server",
                "1.6.4",
            ),
            // A colon already separates the two, so nothing is removed from it.
            ("CapsLock: shift", "CapsLock", "shift"),
            // Two lines, the second repeating the name.
            ("Cline\nCline - 1 session", "Cline", "1 session"),
            // Nothing to split: whatever is left is the name.
            ("WhatsApp", "WhatsApp", ""),
            (
                "Plop! 1.1.260825.b4 \u{2013} Fenster-Konfetti",
                "Plop! 1.1.260825.b4 \u{2013} Fenster-Konfetti",
                "",
            ),
            // A name that merely shares a word is left alone: only the same run of
            // words twice is a repeat.
            ("G HUB Logitech G HUB", "G HUB Logitech G HUB", ""),
            // A repeated name with no state loses the repeat and nothing else.
            ("PowerToys Awake PowerToys Awake", "PowerToys Awake", ""),
        ];

        for (tooltip, title, detail) in cases {
            let (got_title, got_detail) = TrayItem::compose(tooltip);
            assert_eq!(got_title, title, "title of {tooltip:?}");
            assert_eq!(got_detail, detail, "detail of {tooltip:?}");
        }
    }
}
