import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertTriangle,
  ArrowDownUp,
  Pin,
  PinOff,
  Search,
  Settings2,
  X,
} from "lucide-react";

import { api, events, type About, type Prefs, type TrayItem, type TrayList } from "@/lib/ipc";
import {
  faultText,
  LANG_CHOICES,
  resolveLang,
  translator,
  type Translate,
} from "@/lib/i18n";
import { cn, squeeze } from "@/lib/utils";

const EMPTY: TrayList = { items: [], error: null, source: "", opening: false };

/** How long a settings change waits before it is written.
 *
 * A slider drag is a dozen changes a second and every save re-places the panel, so
 * the write happens once the hand comes off. */
const SETTINGS_SAVE_DELAY = 180;

/** The shell's build number as one line, in whichever language is in use. */
function shellLabel(build: string, t: Translate): string {
  return build ? t("settings.shellBuild", { build }) : t("settings.shellUnknown");
}

/** Row height and fixed chrome, which must stay in step with `overlay.rs`: the
 * window is sized from those numbers, and this is what fills that height. */
const ROW_HEIGHT = 38;
const CHROME = 104;

/** An IPC failure in the shape both ends agreed on.
 *
 * Commands answer with a code and, when there is one, what Windows said about it.
 * Anything else arriving here was not meant to be shown, so it gets a code too. */
function asFault(error: unknown): { code: string; detail?: string } {
  if (error && typeof error === "object" && "code" in error) {
    return error as { code: string; detail?: string };
  }
  return { code: "unknown", detail: String(error) };
}

export default function App() {
  const [list, setList] = useState<TrayList>(EMPTY);
  const [prefs, setPrefs] = useState<Prefs | null>(null);
  const [query, setQuery] = useState("");
  const [cursor, setCursor] = useState(0);
  const [busy, setBusy] = useState<number | null>(null);
  const [about, setAbout] = useState<About | null>(null);
  const [dark, setDark] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [dirty, setDirty] = useState(false);
  // The shell's own answer, until it arrives: English is the fallback language, so
  // it is also the fallback before anything is known.
  const [systemLang, setSystemLang] = useState<string>("en");

  const searchRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const lang = resolveLang(prefs?.lang, systemLang);
  const t = useMemo(() => translator(lang), [lang]);

  // The document's language is set along with its words: it is what a screen reader
  // picks its pronunciation from.
  useEffect(() => {
    document.documentElement.lang = lang;
  }, [lang]);

  // Every opening publishes a fresh list, so that is also the moment to reset
  // the filter and hand the keyboard to the search box.
  useEffect(() => {
    const unsubscribe = events.onList((payload) => {
      setList(payload);
      // Only a fresh read is an opening. The same list arriving after a settings
      // change must leave the filter and the keyboard alone.
      if (!payload.opening) {
        return;
      }
      setQuery("");
      setCursor(0);
      setBusy(null);
      setSettingsOpen(false);
      requestAnimationFrame(() => searchRef.current?.focus());
    });
    return () => {
      void unsubscribe.then((off) => off());
    };
  }, []);

  // The panel replaces the shell's own flyout, so it follows the shell's colour
  // scheme instead of keeping one of its own: a preference that disagreed with
  // the taskbar would look like a bug. `prefers-color-scheme` is deliberately not
  // used, because it reports the *app* scheme, which Windows lets users set
  // separately from the shell scheme the flyout is drawn in.
  useEffect(() => {
    const apply = (value: boolean) => {
      setDark(value);
      document.documentElement.classList.toggle("dark", value);
    };
    void api.darkTheme().then(apply).catch(() => apply(false));
    const unsubscribe = events.onTheme(apply);
    return () => {
      void unsubscribe.then((off) => off());
    };
  }, []);

  useEffect(() => {
    void api.systemLang().then(setSystemLang);
    void api
      .getPrefs()
      .then(setPrefs)
      .catch(() => setPrefs(null));
    void api
      .about()
      .then(setAbout)
      .catch(() => setAbout(null));
    // A window that was opened without a fresh read still has something to show.
    void api.current().then((payload) => {
      if (payload.items.length > 0) {
        setList(payload);
      }
    });
  }, []);

  const visible = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) {
      return list.items;
    }
    return list.items.filter((item) =>
      `${item.title} ${item.detail} ${item.tooltip}`.toLowerCase().includes(needle),
    );
  }, [list.items, query]);

  // A filter can shrink the list under the cursor, so keep it in range.
  useEffect(() => {
    setCursor((current) => Math.min(current, Math.max(visible.length - 1, 0)));
  }, [visible.length]);

  // Keep the highlighted row on screen while walking with the arrow keys.
  useEffect(() => {
    const container = listRef.current;
    if (!container) {
      return;
    }
    const row = container.querySelector<HTMLElement>(`[data-row="${cursor}"]`);
    row?.scrollIntoView({ block: "nearest" });
  }, [cursor]);

  const activate = useCallback(async (item: TrayItem, button: "left" | "right") => {
    setBusy(item.index);
    try {
      await api.activate(item.index, button);
    } catch {
      setBusy(null);
    }
  }, []);

  const togglePin = useCallback(async (item: TrayItem) => {
    const next = !item.promoted;
    try {
      await api.setPinned(item.index, next);
      setList((current) => ({
        ...current,
        items: current.items.map((candidate) =>
          candidate.index === item.index ? { ...candidate, promoted: next } : candidate,
        ),
      }));
    } catch (error) {
      console.warn("TrayList:", faultText(asFault(error), t));
    }
  }, [t]);

  // Preferences are edited locally first and written a moment later, so that
  // dragging a slider is one save rather than a dozen, each of which would re-place
  // the panel on screen.
  const editPrefs = useCallback((patch: Partial<Prefs>) => {
    setPrefs((current) => (current ? { ...current, ...patch } : current));
    setDirty(true);
  }, []);

  useEffect(() => {
    if (!dirty || !prefs) {
      return;
    }
    const timer = window.setTimeout(() => {
      setDirty(false);
      void api.setPrefs(prefs).catch((error) => {
        console.warn("TrayList:", faultText(asFault(error), t));
      });
    }, SETTINGS_SAVE_DELAY);
    return () => window.clearTimeout(timer);
  }, [prefs, dirty, t]);

  const toggleSort = useCallback(() => {
    editPrefs({ sort: prefs?.sort === "name" ? "tray" : "name" });
  }, [prefs, editPrefs]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      // The settings dialog is a dialog: Escape closes it, and the list's own keys
      // must not act on a list the user cannot see.
      if (settingsOpen) {
        if (event.key === "Escape") {
          event.preventDefault();
          setSettingsOpen(false);
        }
        return;
      }
      switch (event.key) {
        case "Escape":
          event.preventDefault();
          void api.dismiss();
          break;
        case "ArrowDown":
          event.preventDefault();
          setCursor((current) => Math.min(current + 1, visible.length - 1));
          break;
        case "ArrowUp":
          event.preventDefault();
          setCursor((current) => Math.max(current - 1, 0));
          break;
        case "Enter": {
          event.preventDefault();
          const item = visible[cursor];
          if (item) {
            void activate(item, "left");
          }
          break;
        }
        case "F2": {
          event.preventDefault();
          const item = visible[cursor];
          if (item?.registryKey) {
            void togglePin(item);
          }
          break;
        }
        default:
          break;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [visible, cursor, activate, togglePin, settingsOpen]);

  const total = list.items.length;

  return (
    <div className="tl-frame">
      <div className="tl-card relative">
        <div
          className={cn(
            "flex items-center gap-1.5 px-2 pb-1.5 pt-2",
            prefs?.showSearch === false && "justify-end",
          )}
        >
          <div
            className={cn(
              "relative min-w-0 flex-1",
              prefs?.showSearch === false && "hidden",
            )}
          >
            <Search
              size={13}
              className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-ink-faint"
            />
            <input
              ref={searchRef}
              value={query}
              onChange={(event) => {
                setQuery(event.target.value);
                setCursor(0);
              }}
              placeholder={t("search.placeholder")}
              spellCheck={false}
              autoComplete="off"
              className="h-[30px] w-full rounded-md border border-line bg-elevated pl-7 pr-7 text-[12.5px] text-ink placeholder:text-ink-faint focus:border-accent focus:outline-none"
            />
            {query.length > 0 && (
              <button
                type="button"
                title={t("search.clear")}
                onClick={() => {
                  setQuery("");
                  searchRef.current?.focus();
                }}
                className="absolute right-1 top-1/2 flex h-6 w-6 -translate-y-1/2 items-center justify-center rounded text-ink-faint hover:bg-surface hover:text-ink"
              >
                <X size={12} />
              </button>
            )}
          </div>

          <IconButton
            title={prefs?.sort === "name" ? t("sort.toName") : t("sort.toTray")}
            active={prefs?.sort === "name"}
            onClick={() => void toggleSort()}
          >
            <ArrowDownUp size={13} />
          </IconButton>

          <IconButton
            title={t("toolbar.settings")}
            active={settingsOpen}
            onClick={() => {
              setSettingsOpen((open) => !open);
              searchRef.current?.blur();
            }}
          >
            <Settings2 size={13} />
          </IconButton>
        </div>

        {list.error ? (
          <div className="mx-2 mb-1.5 flex items-start gap-2 rounded-md border border-line bg-elevated px-2.5 py-2 text-[11.5px] leading-snug text-warn">
            <AlertTriangle size={13} className="mt-0.5 shrink-0" />
            <span>{faultText(list.error, t)}</span>
          </div>
        ) : null}

        <div ref={listRef} className="min-h-0 flex-1 overflow-y-auto px-1.5 pb-1.5">
          {visible.map((item, position) => (
            <Row
              key={`${item.index}-${item.title}`}
              item={item}
              position={position}
              highlighted={position === cursor}
              busy={busy === item.index}
              t={t}
              onHover={() => setCursor(position)}
              onActivate={(button) => void activate(item, button)}
              onTogglePin={() => void togglePin(item)}
            />
          ))}

          {visible.length === 0 ? (
            <div className="flex h-full flex-col items-center justify-center gap-1 px-6 text-center">
              <span className="text-[12.5px] text-ink-muted">
                {total === 0 ? t("list.none") : t("list.noMatch")}
              </span>
              <span className="text-[11px] leading-snug text-ink-faint">
                {total === 0
                  ? t("list.noneHint")
                  : t("list.noMatchHint", { query, total })}
              </span>
            </div>
          ) : null}
        </div>

        <div className="flex shrink-0 items-center justify-between gap-2 border-t border-line px-2.5 py-1.5 text-[10.5px] text-ink-faint">
          <span className="flex min-w-0 items-center gap-2">
            {query.trim() ? (
              // While filtering, how many icons are left is the useful number.
              // The rest of the time it is the build this panel came from.
              <span className="whitespace-nowrap" title={shellLabel(list.source, t)}>
                {t("list.count", { shown: visible.length, total })}
              </span>
            ) : about ? (
              <span
                className="truncate"
                title={`${shellLabel(about.shell, t)} / v${about.version} ${about.built}`}
              >
                v{about.version} {about.built}
              </span>
            ) : null}
          </span>
          <span className="flex items-center gap-2.5">
            {/* The keys only, with what they do on hover: the build stamp needs
                the room more than three words nobody reads twice do. */}
            <Hint keys="↑↓" title={t("keys.move")} />
            <Hint keys="Enter" title={t("keys.open")} />
            <Hint keys="Esc" title={t("keys.close")} />
          </span>
        </div>

        {settingsOpen ? (
          <SettingsPanel
            prefs={prefs}
            about={about}
            dark={dark}
            t={t}
            onChange={editPrefs}
            onOpenConfig={() => void api.openConfig()}
            onClose={() => setSettingsOpen(false)}
          />
        ) : null}
      </div>
    </div>
  );
}

function Row({
  item,
  position,
  highlighted,
  busy,
  t,
  onHover,
  onActivate,
  onTogglePin,
}: {
  item: TrayItem;
  position: number;
  highlighted: boolean;
  busy: boolean;
  t: Translate;
  onHover: () => void;
  onActivate: (button: "left" | "right") => void;
  onTogglePin: () => void;
}) {
  return (
    <div
      data-row={position}
      title={item.tooltip}
      onMouseEnter={onHover}
      onClick={() => onActivate("left")}
      onContextMenu={(event) => {
        event.preventDefault();
        onActivate("right");
      }}
      className={cn(
        "tl-row group flex cursor-default items-center gap-2.5 rounded-md px-2",
        highlighted ? "bg-accent-soft" : "hover:bg-elevated",
      )}
    >
      <span className="flex h-4 w-4 shrink-0 items-center justify-center">
        {item.icon ? (
          <img
            src={item.icon}
            alt=""
            width={16}
            height={16}
            draggable={false}
            style={{ background: item.iconBackground, borderRadius: 3 }}
          />
        ) : (
          <span className="h-1.5 w-1.5 rounded-full bg-ink-faint" />
        )}
      </span>

      <span className="min-w-0 flex-1 leading-tight">
        <span className="block truncate text-[12.5px] font-medium text-ink">
          {item.title || t("row.unnamed")}
        </span>
        {/* Only real information here. Every icon in this list is behind the
            chevron by definition, so saying so would be noise on every row; a
            second line appears only when the tooltip carries one or when the
            icon is pinned in the visible part of the tray. */}
        {item.detail || item.promoted ? (
          <span className="block truncate text-[10.5px] text-ink-faint">
            {item.detail ? squeeze(item.detail, 96) : t("row.promoted")}
          </span>
        ) : null}
      </span>

      {busy ? <span className="shrink-0 text-[10px] text-ink-faint">…</span> : null}

      {item.registryKey ? (
        <button
          type="button"
          title={item.promoted ? t("row.unpin") : t("row.pin")}
          onMouseDown={(event) => event.stopPropagation()}
          onClick={(event) => {
            event.stopPropagation();
            onTogglePin();
          }}
          className={cn(
            "flex h-6 w-6 shrink-0 items-center justify-center rounded",
            item.promoted
              ? "text-accent"
              : "text-ink-faint opacity-0 group-hover:opacity-100 hover:text-ink",
          )}
        >
          {item.promoted ? <Pin size={12} /> : <PinOff size={12} />}
        </button>
      ) : null}
    </div>
  );
}

function IconButton({
  children,
  title,
  active,
  onClick,
}: {
  children: React.ReactNode;
  title: string;
  active?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      title={title}
      onClick={onClick}
      className={cn(
        "flex h-[30px] w-[30px] shrink-0 items-center justify-center rounded-md border transition-colors",
        active
          ? "border-accent bg-accent-soft text-accent"
          : "border-line bg-elevated text-ink-muted hover:text-ink",
      )}
    >
      {children}
    </button>
  );
}

function Hint({
  keys,
  title,
  children,
}: {
  keys: string;
  title: string;
  children?: React.ReactNode;
}) {
  return (
    <span className="inline-flex items-center gap-1" title={title}>
      <kbd className="rounded border border-line bg-elevated px-1 py-px font-mono text-[9.5px] text-ink-muted">
        {keys}
      </kbd>
      {children}
    </span>
  );
}

/**
 * The settings dialog.
 *
 * It lives inside the panel rather than in a window of its own. The panel is
 * exactly as big as it needs to be, so a second window would have to be sized,
 * placed and dismissed separately — and every change made here is visible in the
 * list behind it the moment it is made.
 */
function SettingsPanel({
  prefs,
  about,
  dark,
  t,
  onChange,
  onOpenConfig,
  onClose,
}: {
  prefs: Prefs | null;
  about: About | null;
  dark: boolean;
  t: Translate;
  onChange: (patch: Partial<Prefs>) => void;
  onOpenConfig: () => void;
  onClose: () => void;
}) {
  // `null` until the answer is in, which also disables the switch: a check box
  // that does not know its own state would be a guess.
  const [autostart, setAutostart] = useState<boolean | null>(null);

  useEffect(() => {
    void api
      .getAutostart()
      .then(setAutostart)
      .catch(() => setAutostart(null));
  }, []);

  const toggleAutostart = async () => {
    if (autostart === null) {
      return;
    }
    try {
      // Read back rather than assumed: if the registry refused the write, the
      // switch has to show that instead of pretending it worked.
      setAutostart(await api.setAutostart(!autostart));
    } catch (error) {
      console.warn("TrayList:", faultText(asFault(error), t));
    }
  };

  // How many rows the height limit works out to, which is the number the user is
  // really choosing between.
  const rows = prefs ? Math.max(Math.floor((prefs.panelMaxHeight - CHROME) / ROW_HEIGHT), 1) : 0;

  return (
    <div className="absolute inset-0 z-10 flex flex-col bg-surface">
      <div className="flex shrink-0 items-center justify-between border-b border-line px-2.5 py-2">
        <span className="text-[12.5px] font-medium text-ink">{t("settings.title")}</span>
        <button
          type="button"
          title={t("settings.close")}
          onClick={onClose}
          className="flex h-6 w-6 items-center justify-center rounded text-ink-faint hover:bg-elevated hover:text-ink"
        >
          <X size={13} />
        </button>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-3 py-3">
        {prefs ? (
          <>
            <Group title={t("settings.appearance")}>
              <Slider
                label={t("settings.height")}
                value={prefs.panelMaxHeight}
                min={320}
                max={1400}
                step={20}
                hint={rows > 0 ? t("settings.heightHint", { rows }) : ""}
                onChange={(value) => onChange({ panelMaxHeight: value })}
              />
              <Slider
                label={t("settings.gap")}
                value={prefs.edgeGap}
                min={0}
                max={40}
                step={1}
                hint={prefs.edgeGap === 0 ? t("settings.flush") : `${prefs.edgeGap} px`}
                onChange={(value) => onChange({ edgeGap: value })}
              />
            </Group>

            <Group title={t("settings.behaviour")}>
              <Switch
                label={t("settings.autostart")}
                checked={autostart === true}
                disabled={autostart === null}
                onChange={() => void toggleAutostart()}
              />
              <Switch
                label={t("settings.pinnedFirst")}
                checked={prefs.pinnedFirst}
                onChange={(value) => onChange({ pinnedFirst: value })}
              />
              <Switch
                label={t("settings.showSearch")}
                checked={prefs.showSearch}
                onChange={(value) => onChange({ showSearch: value })}
              />
              <Switch
                label={t("settings.sortByName")}
                checked={prefs.sort === "name"}
                onChange={(value) => onChange({ sort: value ? "name" : "tray" })}
              />
            </Group>

            <Group title={t("settings.language")}>
              {/* The three choices rather than a switch: `system` is a third state,
                  and a two-state control would force a guess about which language
                  the shell is actually in. */}
              <Choice
                options={LANG_CHOICES}
                value={prefs.lang}
                onChange={(value) => onChange({ lang: value })}
              />
            </Group>
          </>
        ) : (
          <p className="text-[11.5px] text-ink-muted">
            {t("settings.unavailable")}
          </p>
        )}

        <Group title={t("settings.about")}>
          {/* The badge is the site's, kept as it is served: ink on a dark panel,
              paper on a light one. Hotlinking the original would leave the panel
              dependent on the network for its own credit line, so a copy lives in
              `public/badges`, and clicking it goes back to the site that explains
              it. */}
          <a
            href="https://notbyhumans.fyi"
            title="Developed by AI, not by humans"
            onClick={(event) => {
              event.preventDefault();
              void api.openUrl("https://notbyhumans.fyi");
            }}
            className="inline-flex"
          >
            <img
              src={dark ? "/badges/developed-ink.svg" : "/badges/developed-paper.svg"}
              alt="Developed by AI, not by humans"
              width={132}
              height={43}
              draggable={false}
            />
          </a>

          <dl className="mt-2.5">
            <Fact label={t("settings.version")}>
              v{about?.version ?? "?"} {about?.built ?? ""}
            </Fact>
            <Fact label={t("settings.shell")}>{shellLabel(about?.shell ?? "", t)}</Fact>
          </dl>

          <button
            type="button"
            onClick={onOpenConfig}
            className="mt-2 text-[10.5px] text-ink-muted underline decoration-line-strong underline-offset-2 hover:text-ink"
          >
            {t("settings.openConfig")}
          </button>
        </Group>
      </div>

      <div className="flex shrink-0 items-center justify-end border-t border-line px-2.5 py-2">
        <button
          type="button"
          onClick={onClose}
          className="rounded-md border border-accent bg-accent-soft px-2.5 py-1 text-[11.5px] font-medium text-accent"
        >
          {t("settings.done")}
        </button>
      </div>
    </div>
  );
}

/** A titled block inside the settings dialog. */
function Group({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="mb-3.5 last:mb-0">
      <h2 className="mb-1.5 text-[10px] font-medium uppercase tracking-wide text-ink-faint">
        {title}
      </h2>
      <div className="space-y-2">{children}</div>
    </section>
  );
}

/** One row of the facts at the bottom of the dialog. */
function Fact({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex gap-2 text-[10.5px]">
      <dt className="w-14 shrink-0 text-ink-faint">{label}</dt>
      <dd className="min-w-0 truncate text-ink-muted">{children}</dd>
    </div>
  );
}

/** A labelled range control, with what the value currently means beside it. */
function Slider({
  label,
  hint,
  value,
  min,
  max,
  step,
  onChange,
}: {
  label: string;
  hint: string;
  value: number;
  min: number;
  max: number;
  step: number;
  onChange: (value: number) => void;
}) {
  return (
    <label className="block">
      <span className="flex items-baseline justify-between gap-2">
        <span className="text-[11.5px] text-ink">{label}</span>
        <span className="text-[10.5px] text-ink-faint">{hint}</span>
      </span>
      <input
        type="range"
        value={value}
        min={min}
        max={max}
        step={step}
        onChange={(event) => onChange(Number(event.target.value))}
        className="mt-1 h-1.5 w-full accent-accent"
      />
    </label>
  );
}

/** A check box drawn as a switch, with a switch's keyboard behaviour. */
function Switch({
  label,
  checked,
  disabled,
  onChange,
}: {
  label: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className="flex w-full items-center justify-between gap-2 text-left"
    >
      <span className={cn("text-[11.5px]", disabled ? "text-ink-faint" : "text-ink")}>
        {label}
      </span>
      <span
        className={cn(
          "flex h-[18px] w-[30px] shrink-0 items-center rounded-full border px-0.5 transition-colors",
          checked ? "border-accent bg-accent-soft" : "border-line-strong bg-elevated",
          disabled && "opacity-50",
        )}
      >
        <span
          className={cn(
            "h-[12px] w-[12px] rounded-full transition-transform",
            checked ? "translate-x-[12px] bg-accent" : "bg-ink-faint",
          )}
        />
      </span>
    </button>
  );
}

/** A row of small buttons, for a setting with a handful of named values.
 *
 * All of them are on screen at once, which is the point: the alternative for
 * something like the language would be a drop-down that hides the choices behind
 * one more click. */
function Choice<T extends string>({
  options,
  value,
  onChange,
}: {
  options: { value: T; label: string }[];
  value: T;
  onChange: (value: T) => void;
}) {
  return (
    <div className="flex gap-1">
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          aria-pressed={option.value === value}
          onClick={() => onChange(option.value)}
          className={cn(
            "min-w-0 flex-1 truncate rounded-md border px-1.5 py-1 text-[11.5px]",
            option.value === value
              ? "border-accent bg-accent-soft text-accent"
              : "border-line bg-elevated text-ink-muted hover:text-ink",
          )}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}