import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

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

export interface TrayList {
  items: TrayItem[];
  error: string | null;
  source: string;
  /**
   * Whether this is a fresh read, i.e. the panel just opened, rather than the same
   * list being re-sent after a settings change. Only a fresh one resets the filter
   * and takes the keyboard.
   */
  opening: boolean;
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
}

export type MouseButton = "left" | "right";

export const api = {
  /** Replays a click on an icon. Left opens it, right opens its menu. */
  activate: (index: number, button: MouseButton) =>
    invoke<void>("activate", { index, button }),
  dismiss: () => invoke<void>("dismiss"),
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