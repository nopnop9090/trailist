import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { LangPref } from "./i18n";

/**
 * Typed mirror of the Rust IPC surface.
 *
 * Field names are camelCase because Rust serialises them that way; the
 * preferences keep the same shape on disk so `config/settings.json` stays
 * hand-editable.
 */

export interface TrayItem {
  /** Position in the list, which is what `activate` takes. */
  index: number;
  /** The icon's complete tooltip, exactly as the shell reported it. */
  tooltip: string;
  /** First line of the tooltip: usually the application name. */
  title: string;
  /** Everything after that: usually the current state. */
  detail: string;
  /** PNG data URL, empty when the icon could not be grabbed. */
  icon: string;
  /** Flyout colour painted behind the icon so pale pixels blend in. */
  iconBackground: string;
  /** Where the icon sat on screen, used to replay a click. */
  x: number;
  y: number;
  /** The shell's registry key for this icon, when it could be matched. */
  registryKey?: string;
  /** Whether the icon currently lives in the visible part of the tray. */
  promoted: boolean;
}

/**
 * Something that went wrong, as a code the panel puts into words.
 *
 * The sentence is written in the panel, so the code is what travels: an error in
 * German inside an English panel would give the lie to the language setting.
 * `detail` is what Windows said, unchanged and in Windows' own language.
 */
export interface Fault {
  code: string;
  detail?: string;
}

export interface TrayList {
  items: TrayItem[];
  /** Set when the read failed, so the panel can say so instead of looking empty. */
  error: Fault | null;
  source: string;
  /**
   * Whether this is a fresh read, i.e. the panel just opened, rather than the same
   * list being re-sent after a settings change. Only a fresh one resets the filter
   * and takes the keyboard.
   */
  opening: boolean;
  /**
   * The list came from the in-explorer host. When this is false, a click is only
   * a best effort through the stock flyout.
   */
  direct: boolean;
}

/** A row in CSS pixels, with the webview's device-pixel ratio. */
export interface RowBox {
  left: number;
  top: number;
  right: number;
  bottom: number;
  dpr: number;
}

/** What the panel says about itself: version, build stamp, shell build. */
export interface About {
  version: string;
  /** Build stamp as `YYYY-MM-DD HH:MM`, baked in at compile time. */
  built: string;
  shell: string;
}

export type SortMode = "tray" | "name";

export interface Prefs {
  panelWidth: number;
  panelMaxHeight: number;
  /** Breathing room to the taskbar in logical pixels; 0 sits flush. */
  edgeGap: number;
  iconSize: number;
  sort: SortMode;
  showSearch: boolean;
  hotkey: string | null;
  pinnedFirst: boolean;
  /** `de`, `en`, or `system` for whatever the shell is set to. */
  lang: LangPref;
}

export type MouseButton = "left" | "right";

export const api = {
  /** Activates an icon. Left opens it, right opens its menu. */
  activate: (index: number, button: MouseButton, row: RowBox) =>
    invoke<void>("activate", { index, button, row }),
  /** Pointer entered or left a row. The panel stays open. */
  hover: (index: number, enter: boolean, row: RowBox) =>
    invoke<void>("hover", { index, enter, row }),
  /** How long the shell waits before a hover counts, in milliseconds. */
  hoverTime: () => invoke<number>("hover_time"),
  dismiss: () => invoke<void>("dismiss"),
  /** The language Windows' own interface is in, as `de` or `en`. */
  systemLang: () => invoke<string>("system_lang"),
  current: () => invoke<TrayList>("current"),
  /** Whether the shell is dark, so the panel can follow it rather than guess. */
  darkTheme: () => invoke<boolean>("dark_theme"),
  getPrefs: () => invoke<Prefs>("get_prefs"),
  setPrefs: (prefs: Prefs) => invoke<Prefs>("set_prefs", { prefs }),
  /** Turns an icon on or off in the visible part of the tray. */
  setPinned: (index: number, pinned: boolean) =>
    invoke<void>("set_pinned", { index, pinned }),
  openConfig: () => invoke<void>("open_config"),
  toggle: () => invoke<void>("toggle"),
  /** Whether TrayList is set to start with Windows. */
  getAutostart: () => invoke<boolean>("get_autostart"),
  /** Turns autostart on or off and returns what it ended up as. */
  setAutostart: (enabled: boolean) => invoke<boolean>("set_autostart", { enabled }),
  /** Version, build stamp and shell build. */
  about: () => invoke<About>("about"),
  /** Opens a link in the user's own browser. Only `https` is accepted. */
  openUrl: (url: string) => invoke<void>("open_url", { url }),
};

export const events = {
  /** Fires every time the flyout has been read, i.e. every time the panel opens. */
  onList: (handler: (list: TrayList) => void): Promise<UnlistenFn> =>
    listen<TrayList>("tl:list", (event) => handler(event.payload)),
  /**
   * Fires with every list, carrying whether the shell is dark.
   *
   * Riding along with the list is deliberate: the panel is only on screen right
   * after a read, so there is nothing to update in between.
   */
  onTheme: (handler: (dark: boolean) => void): Promise<UnlistenFn> =>
    listen<boolean>("tl:theme", (event) => handler(event.payload)),
};