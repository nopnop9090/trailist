/**
 * The panel's two languages.
 *
 * German is what the app was written in and English is the fallback, so a machine
 * set to anything else reads English rather than a half-German mix. Nothing here is
 * generated: these tables are the whole translation, and a key missing from one of
 * them falls back to English and, failing that, to the key itself — both are
 * visible at a glance, which a blank would not be.
 */

export type Lang = "de" | "en";

/** What the setting may hold: one of the two, or `system` for the shell's own. */
export type LangPref = Lang | "system";

/** The choices the settings dialog offers. */
export const LANG_CHOICES: { value: LangPref; label: string }[] = [
  { value: "system", label: "System" },
  // Both languages are named in themselves: someone looking for English should not
  // have to read German to find it.
  { value: "de", label: "Deutsch" },
  { value: "en", label: "English" },
];

function isLang(value: string): value is Lang {
  return value === "de" || value === "en";
}

/** Which language to use: the setting if it names one, the system's otherwise. */
export function resolveLang(setting: LangPref | undefined, system: string): Lang {
  if (setting === "de" || setting === "en") {
    return setting;
  }
  return isLang(system) ? system : "en";
}

/** The values that go into `{placeholders}`. */
type Params = Record<string, string | number>;

export type Translate = (key: string, params?: Params) => string;

const DE: Record<string, string> = {
  "search.placeholder": "Suchen…",
  "search.clear": "Filter löschen",
  "sort.toName": "Alphabetisch sortiert – klicken für Tray-Reihenfolge",
  "sort.toTray": "In Tray-Reihenfolge – klicken für alphabetisch",
  "toolbar.settings": "Einstellungen",
  "list.count": "{shown} von {total}",
  "list.none": "Keine Symbole gelesen.",
  "list.noneHint":
    "Öffne den Tray über den Pfeil – TrayList liest die Symbole mit, sobald sie sichtbar sind.",
  "list.fallback":
    "Direkte Weiterleitung ist nicht verbunden. Ein Klick hier ist nur ein Versuch und kann das falsche Symbol treffen.",
  "list.noMatch": "Nichts gefunden.",
  "list.noMatchHint": "„{query}“ passt zu keinem der {total} Symbole.",
  "keys.move": "Mit den Pfeiltasten bewegen",
  "keys.open": "Das gewählte Symbol öffnen",
  "keys.close": "Die Liste schließen",
  "row.unnamed": "(ohne Namen)",
  "row.promoted": "im Tray sichtbar",
  "row.pin": "Immer im Tray anzeigen",
  "row.unpin": "Nicht mehr immer anzeigen",
  "settings.title": "Einstellungen",
  "settings.close": "Schließen",
  "settings.appearance": "Darstellung",
  "settings.height": "Höhe der Liste",
  "settings.heightHint": "bis zu {rows} Zeilen",
  "settings.gap": "Abstand zur Taskleiste",
  "settings.flush": "bündig",
  "settings.behaviour": "Verhalten",
  "settings.autostart": "Mit Windows starten",
  "settings.pinnedFirst": "Immer sichtbare zuerst",
  "settings.showSearch": "Suchfeld anzeigen",
  "settings.sortByName": "Alphabetisch sortieren",
  "settings.language": "Sprache",
  "settings.about": "Über",
  "settings.version": "Version",
  "settings.shell": "Shell",
  "settings.shellBuild": "Windows build {build}",
  "settings.shellUnknown": "unbekannt",
  "settings.openConfig": "config-Ordner öffnen",
  "settings.done": "Fertig",
  "settings.unavailable": "Die Einstellungen ließen sich nicht laden.",
  "error.item_gone": "Dieser Eintrag existiert nicht mehr.",
  "error.worker_down": "Der Hintergrunddienst läuft nicht.",
  "error.no_registry_entry":
    "Für dieses Symbol wurde kein Eintrag in der Registry gefunden.",
  "error.registry_write": "Die Tray-Einstellung ließ sich nicht schreiben.",
  "error.settings_write": "Die Einstellungen ließen sich nicht speichern.",
  "error.autostart_write": "Der Autostart-Eintrag ließ sich nicht ändern.",
  "error.open_config": "Der config-Ordner ließ sich nicht öffnen.",
  "error.open_url": "Der Link ließ sich nicht öffnen.",
  "error.uia_missing": "UI Automation ist auf diesem System nicht verfügbar.",
  "error.no_taskbar": "Die Taskleiste wurde nicht gefunden.",
  "error.no_chevron": "Der Pfeil für ausgeblendete Symbole wurde nicht gefunden.",
  "error.chevron_unreadable": "Der Pfeil ließ sich nicht lesen.",
  "error.unknown": "Unbekannter Fehler: {code}",
};

const EN: Record<string, string> = {
  "search.placeholder": "Search…",
  "search.clear": "Clear the filter",
  "sort.toName": "Sorted by name – click for tray order",
  "sort.toTray": "In tray order – click to sort by name",
  "toolbar.settings": "Settings",
  "list.count": "{shown} of {total}",
  "list.none": "No icons were read.",
  "list.noneHint":
    "Open the tray with the chevron – TrayList reads the icons along as soon as they are visible.",
  "list.fallback":
    "Direct forwarding is not connected. A click here is only a best effort and can hit the wrong icon.",
  "list.noMatch": "Nothing found.",
  "list.noMatchHint": "“{query}” matches none of the {total} icons.",
  "keys.move": "Move with the arrow keys",
  "keys.open": "Open the selected icon",
  "keys.close": "Close the list",
  "row.unnamed": "(unnamed)",
  "row.promoted": "shown in the tray",
  "row.pin": "Always show in the tray",
  "row.unpin": "Stop always showing it",
  "settings.title": "Settings",
  "settings.close": "Close",
  "settings.appearance": "Appearance",
  "settings.height": "List height",
  "settings.heightHint": "up to {rows} rows",
  "settings.gap": "Gap to the taskbar",
  "settings.flush": "flush",
  "settings.behaviour": "Behaviour",
  "settings.autostart": "Start with Windows",
  "settings.pinnedFirst": "Always-visible first",
  "settings.showSearch": "Show the search field",
  "settings.sortByName": "Sort by name",
  "settings.language": "Language",
  "settings.about": "About",
  "settings.version": "Version",
  "settings.shell": "Shell",
  "settings.shellBuild": "Windows build {build}",
  "settings.shellUnknown": "unknown",
  "settings.openConfig": "Open the config folder",
  "settings.done": "Done",
  "settings.unavailable": "The settings could not be loaded.",
  "error.item_gone": "That entry is gone.",
  "error.worker_down": "The background worker is not running.",
  "error.no_registry_entry": "No registry entry was found for this icon.",
  "error.registry_write": "The tray setting could not be written.",
  "error.settings_write": "The settings could not be saved.",
  "error.autostart_write": "The autostart entry could not be changed.",
  "error.open_config": "The config folder could not be opened.",
  "error.open_url": "The link could not be opened.",
  "error.uia_missing": "UI Automation is not available on this system.",
  "error.no_taskbar": "The taskbar was not found.",
  "error.no_chevron": "The chevron for hidden icons was not found.",
  "error.chevron_unreadable": "The chevron could not be read.",
  "error.unknown": "Unknown failure: {code}",
};

const TABLES: Record<Lang, Record<string, string>> = { de: DE, en: EN };

/** The translator for one language, with English as the fallback. */
export function translator(lang: Lang): Translate {
  const table = TABLES[lang];
  return (key, params) => {
    const template = table[key] ?? EN[key] ?? key;
    if (!params) {
      return template;
    }
    return template.replace(/\{(\w+)\}/g, (placeholder, name: string) =>
      name in params ? String(params[name]) : placeholder,
    );
  };
}

/** What Windows or the shell said, as one line the panel can show. */
export function faultText(fault: { code: string; detail?: string }, t: Translate): string {
  const sentence = t(`error.${fault.code}`);
  // A code with no translation comes back as its own key, which says less than
  // admitting that the code is the unfamiliar part.
  const head = sentence.startsWith("error.")
    ? t("error.unknown", { code: fault.code })
    : sentence;
  // The detail is passed through as it came: it is Windows' own wording, in the
  // language Windows is installed in, and paraphrasing it would lose what it names.
  return fault.detail ? `${head} (${fault.detail})` : head;
}
