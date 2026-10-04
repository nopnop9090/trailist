//! The two languages the app speaks.
//!
//! German is what it was written in and English is the fallback, so a machine set
//! to anything else gets English rather than a half-German mix. Which one is in use
//! is a setting whose default is `system`, and the system's answer comes from
//! Windows' own user-interface language rather than from the culture of the current
//! thread: the question that matters is what the shell around the panel is
//! written in.

use windows::Win32::Globalization::GetUserDefaultUILanguage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    De,
    En,
}

impl Lang {
    /// The tag that travels over IPC and into `settings.json`.
    pub fn tag(self) -> &'static str {
        match self {
            Self::De => "de",
            Self::En => "en",
        }
    }

    /// The language a preference asks for: `de` or `en` when it names one, and the
    /// system's own answer for `system` and for anything unrecognised.
    pub fn resolve(preference: &str) -> Self {
        match preference {
            "de" => Self::De,
            "en" => Self::En,
            _ => Self::system(),
        }
    }

    /// The language Windows' own interface is in.
    ///
    /// `GetUserDefaultUILanguage` honours the user's language list, which is what
    /// the taskbar and the tray flyout are drawn in. Only the primary language
    /// counts: `de-DE`, `de-AT` and `de-CH` differ in the low bits of the id and
    /// are all German.
    fn system() -> Self {
        // SAFETY: the call takes no arguments, writes nothing and cannot fail, so
        // there is no invariant for the caller to hold up.
        let id = unsafe { GetUserDefaultUILanguage() };
        if id & 0x3ff == 0x07 {
            Self::De
        } else {
            Self::En
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Lang;

    /// A preference that names a language wins; `system` and nonsense both fall
    /// through to the system's answer, which is one of the two either way.
    #[test]
    fn resolves_the_setting_before_the_system() {
        assert_eq!(Lang::resolve("de"), Lang::De);
        assert_eq!(Lang::resolve("en"), Lang::En);
        assert!(matches!(Lang::resolve("system"), Lang::De | Lang::En));
        assert!(matches!(Lang::resolve("klingon"), Lang::De | Lang::En));
        assert!(matches!(Lang::resolve(""), Lang::De | Lang::En));
    }
}
