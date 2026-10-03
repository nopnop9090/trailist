# TrayList — Plan, Was, Wie, Wann, Warum

Die englische Fassung dieses Dokuments ist [PLAN.en.md](PLAN.en.md); beide sagen
dasselbe. Die Dokumentation für Leser des Projekts sind [README.md](../README.md)
und [README.de.md](../README.de.md).

Kurzfassung: TrayList ersetzt das Raster-Flyout der Windows-11-Tray durch eine
**vertikale, scrollbare Liste mit Namen**. Klick auf den Pfeil wie immer — gelesen
wird das Fenster, das Windows gerade geöffnet hat, danach wird es ausgeblendet und
an seiner Stelle die Liste gezeigt.

---

## Warum

**Problem:** Windows 11 versteckt neue Tray-Symbole hinter dem Pfeil und zeigt sie
als Raster unbeschrifteter 16×16-Glyphen. Bei mehreren Dutzend Symbolen heißt das:
hovern, Tooltip abwarten, raten.

**Ziel:** Ein Panel, in dem alle Symbole **untereinander mit Namen** stehen, mit
Filter, Tastaturbedienung und Scrollen — und das sich anfühlt wie das native
Flyout, nur lesbar.

**Design-Prinzipien:**
- Keine Injektion, keine Shell-Hooks, keine Admin-Rechte
- Der Klick auf den Pfeil bleibt der Einstieg; nichts muss umgelernt werden
- Explorer wird nie in einen kaputten Zustand gebracht (Hide statt "Close")
- Ein Binary, portabel, Konfiguration daneben

---

## Was (Funktionsumfang)

| Feature | Beschreibung |
|--------|----------------|
| Liste | Icon + Name (Tooltip) + aktueller Zustand pro Zeile, Pfeiltasten/Enter/Esc |
| Filter | Suchfeld, automatisch fokussiert, filtert beim Tippen |
| Sortierung | Tray-Reihenfolge oder alphabetisch |
| Klick-Forwarding | Links = App öffnen, Rechtsklick = echtes Kontextmenü des Symbols |
| Pin | "Immer im Tray anzeigen" schreibt `IsPromoted` in `NotifyIconSettings` |
| Hotkey | `Alt+Shift+T` öffnet die Liste ohne Chevron |
| Tray-Menü | Liste zeigen, Einstellungen öffnen, Beenden |
| Geometrie | Panel am Flyout verankert, im Arbeitsbereich des richtigen Monitors, DPI-korrekt |
| Sprache | Deutsch und Englisch, standardmäßig der Sprache der Shell folgend |

**Nicht im Scope:** die Reihenfolge der Icons im echten Tray verändern, Icon-Designs
ersetzen, Windows-10-Tray, Remote-Steuerung.

---

## Wie (Architektur & Technik)

```
Watcher-Thread (besitzt UI Automation und jeden Shell-Zugriff)
  |
  |- island.rs    Flyout finden        TopLevelWindowForOverflowXamlIsland
  |- uia.rs       Symbole lesen        Buttons mit AutomationId=NotifyItemIcon,
  |                                    deren Name der komplette Tooltip ist
  |- capture.rs   Icons holen         vorrangig das PNG-Snapshot des Shell
  |                                  (NotifyIconSettings\IconSnapshot), sonst
  |                                  PrintWindow(PW_RENDERFULLCONTENT) + Chroma-Key
  |- unterdruecken  ShowWindow(island, SW_HIDE)
  |- overlay      eigenes Panel in der Geometrie des Flyouts
  |- forward.rs   Klick nachspielen    SW_SHOWNOACTIVATE, SendInput, SW_HIDE
```

### Die Erkenntnisse, auf denen das aufbaut

Alles auf **Windows 11 25H2, Build 26200** empirisch geprüft:

1. **Der klassische Weg ist tot.** Es gibt kein `ToolbarWindow32` mehr im Tray;
   `TrayNotifyWnd` ist leer. `TB_BUTTONCOUNT`/`TB_GETBUTTON`/`TRAYDATA`
   funktionieren nicht mehr. (Windows 11 22H2 hat das eingeführt.)
2. **Das Flyout ist ein adressierbares Fenster:** `TopLevelWindowForOverflowXamlIsland`,
   bei offenem Flyout sichtbar, sonst versteckt.
3. **UI Automation liefert die Namen**, solange das Flyout offen ist:
   `ClassName=SystemTray.NormalButton`, `AutomationId=NotifyItemIcon`,
   `Name` = kompletter Tooltip — inklusive Zeilenumbrüchen.
4. **`ShowWindow(SW_HIDE)` blendet das Raster sauber aus**, und der nächste
   Klick auf den Pfeil öffnet es wieder normal: Explorers Zustand bleibt intakt.
5. **Klicks lassen sich nachspielen**: Flyout kurz ohne Aktivierung zeigen, per
   `SendInput` klicken, wieder ausblenden → das echte Menü der Ziel-App erscheint.
6. **`HKCU\Control Panel\NotifyIconSettings`** enthält pro Symbol `ExecutablePath`,
   `InitialTooltip`, `IconSnapshot` (PNG) und `IsPromoted`.

### Wichtige technische Entscheidungen

1. **Ein Thread, eine Wahrheit.** UI Automation und jede Win32-Interaktion laufen
   im Watcher-Thread. Die UI ruft nur Kommandos auf; jede Shell-Anfrage geht per
   Channel dorthin. Das vermeidet COM-Apartment-Probleme auf dem Tauri-Main-Thread
   (WebView2 braucht dort STA) und hält 300-ms-Klickvorgänge von der UI fern.
2. **Zustandsautomat statt Event-Salat.** `Idle -> Reading -> Open -> Idle`, mit
   `blackout` (eigene Aktionen ignorieren) und `cooldown` (der Klick, der schließt,
   darf nicht als "öffnen" gelesen werden).
3. **`SetCursorPos` gibt es nicht in den windows-rs-Bindings.** Deshalb absolute
   `SendInput`-Bewegung in Virtual-Desktop-Koordinaten (deckt auch den zweiten
   Monitor ab).
4. **Foreground erzwingen.** Der Klick, der uns öffnet, ging an die Taskleiste —
   wir sind also nicht der Vordergrundprozess. `AttachThreadInput` auf den
   Vordergrund- und den Ziel-Thread ist der dokumentierte Weg dahin, und es wird
   **zweimal** versucht: der erste Versuch scheitert oft, und eine Auto-Hide-
   Taskleiste gibt den Vordergrund beim Einklappen wieder ab.
5. **Lesen erst, wenn das Layout steht.** Das Flyout animiert auf und platziert
   jedes Symbol nacheinander. Ein Read im Moment des Erscheinens liefert deshalb
   Symbole auf Positionen, die sie gerade verlassen, und einige ganz ohne Rechteck.
   Ein Element ohne Rechteck meldet den Ursprung — es würde die Liste vorne
   einsortieren und im Bitmap die falsche Ecke ausschneiden. Gelesen wird erst,
   wenn ein Read nichts Unplatziertes mehr meldet und die Anzahl sich wiederholt.
   Das sind die ~150 ms, die Windows' Raster sichtbar bleibt.
6. **Icons kommen aus der Shell, nicht vom Bildschirm.** `HKCU\Control Panel\`
   `NotifyIconSettings` speichert zu jedem Symbol ein PNG-Snapshot samt
   `InitialTooltip`; die Zeilen werden per Tooltip zugeordnet und bekommen so das
   echte Bitmap mit echter Transparenz. Nur Symbole ohne Snapshot werden aus einem
   `PrintWindow(PW_RENDERFULLCONTENT)`-Rendering des Flyouts als Zelle
   ausgeschnitten und gegen dessen Hintergrund gekeyt.
7. **Positionen kommen aus dem Gezeichneten.** UIA-Rechtecke können während der
   Öffnungsanimation noch wandern; ein um eine halbe Zelle verschobenes Rechteck
   schneidet Hintergrund aus und schiebt das Icon nach unten. Deshalb wird das
   Zellraster aus dem Rendering gemessen (Projektion von „nicht Hintergrund“ auf
   beide Achsen, in gleichmäßige Bänder geschnitten) — und die Reihenfolge kommt
   aus der Liste der Shell statt aus den wackeligen Rechtecken.
8. **Explorer bleibt unangetastet.** Verstecken statt Schließen; keine Hooks, kein
   Neustart, keine Registry-Manipulation außer dem ausdrücklichen Pin.
9. **Das Panel folgt dem Erscheinungsbild der Shell.** Windows führt zwei Schemata
   nebeneinander: `AppsUseLightTheme` (App-Fenster, und genau das meldet
   `prefers-color-scheme` in einer Webview) und `SystemUsesLightTheme` (Taskleiste,
   Tray, Flyout). Das Panel ersetzt das Flyout, also folgt es dem Shell-Schema:
   bei jedem Öffnen im Watcher-Thread gelesen und als `tl:theme` mit der Liste
   verschickt. `index.html` trägt deshalb keine Theme-Klasse mehr — die
   `:root`-Tokens sind die hellen, ein Panel ohne Antwort ist also korrekt statt
   schwarz.
10. **Das Panel sitzt am Chevron und auf der Taskleiste.** Zwei Fehler in der alten
    Geometrie: das Panel wurde rechts an `flyout.right` ausgerichtet und *zusätzlich*
    um den Schattenrand eingerückt, wodurch die sichtbare Karte 44 px von der
    Flyout-Kante weg war (2 × `SHADOW`), und die Breite schob sie per Clamp an den
    Bildschirmrand statt unter den Chevron. Jetzt gilt: horizontal *auf der
    Flyout-Mitte* zentriert — die Shell zentriert das Flyout auf den Chevron, also
    steht das Panel unter dem Button, den der Nutzer geklickt hat — und die
    Fensterunterkante (inklusive Schattenrand) liegt auf `flyout.bottom`, also auf
    der Taskleiste. Der Schattenrand darf die Taskleiste *nicht* überlappen: das
    Fenster ist always-on-top und nicht klickdurchlässig. `edgeGap` verschiebt das
    Panel auf Wunsch nach oben.
11. **Einstellungen im Modal, Autostart als echter Run-Eintrag.** Das Zahnrad
    öffnet einen Dialog im Panel (kein zweites Fenster: das Panel ist genau so groß
    wie nötig, ein eigenes Fenster müsste separat platziert und geschlossen werden).
    Höhe und Abstand wirken sofort: `set_prefs` platziert das offene Panel neu, die
    dafür nötige Flyout-Position merkt sich `Shared`. „Mit Windows starten" schreibt
    `HKCU\...\CurrentVersion\Run` statt ein Plugin zu benutzen — dort schaut Windows
    nach, dort kann der Task-Manager es abschalten.
12. **Version und Build-Stempel.** Ein Panel, das wie das System aussieht, muss
    sagen können, welcher Build es ist. Der Stempel entsteht in `build.rs` aus
    `GetLocalTime` (kein Datums-Crate; der Compiler-Host ist immer Windows) und wird
    als `TRAILIST_BUILT` eingesetzt. Wichtig: `cargo:rerun-if-changed=src`, weil
    Cargo die Ausgabe von Build-Skripten cacht — ohne das trägt ein neuer Build ein
    altes Datum.
13. **Der Name steht nie zweimal in einer Zeile.** Die Tooltips dieses Rechners
    kommen in drei Formen: zweizeilig (`CapsLock` / `shift` als zwei Zeilen),
    einzeilig mit Doppelpunkt (`CapsLock: shift`) und einzeilig mit wiederholtem
    Namen (`TrayMaster TrayMaster - 3/3 running`). Die dritte Form ist der Grund,
    warum in den Zeilen der Name doppelt stand. `TrayItem::compose` nimmt die
    längste führende Wortwiederholung ab und trimmt das Trennzeichen dahinter; der
    Test in `types.rs` läuft gegen die echten Tooltips dieses Rechners.
14. **Zwei Sprachen, und Fehler reisen als Code.** Panel, Tray-Menü und
    Fehlermeldungen folgen einer Einstellung `lang`, deren Standard `system` ist —
    die Sprache der Windows-Oberfläche, mit Englisch als Rückfall für alles, was
    nicht Deutsch ist. Eine Fehlermeldung reist deshalb nicht mehr als fertiger
    deutscher Satz: sie reist als Code plus, wo Windows etwas zu sagen hatte, dessen
    unverändertem Text, und der Satz entsteht dort, wo die Sprache bekannt ist.
    `commands::system_lang` antwortet einmal beim Start, und `retitle_tray` gibt der
    Shell ein neues Menü, wenn die Einstellung wandert.
15. **Pfade verraten die Build-Maschine nicht.** rustc schreibt zu jeder Panic-Stelle
    die Quelldatei mit, und bei Dependencies ist das ein absoluter Pfad unter dem
    Benutzerprofil des Builders. `scripts/build.ps1` biegt diese Präfixe per
    `--remap-path-prefix` um, mit den Präfixen aus der Umgebung, damit kein
    rechnerspezifischer Text im Skript steht. Gleiches Binary, eine Sache weniger
    drin.

### Repo-Struktur

| Pfad | Rolle |
|------|--------|
| `src/` | React-Panel (die Liste) |
| `src/lib/i18n.ts` | Die deutschen und englischen Texte des Panels |
| `src-tauri/src/watcher.rs` | Zustandsautomat |
| `src-tauri/src/overlay.rs` | Geometrie und Platzierung |
| `src-tauri/src/i18n.rs` | Die Sprachauswahl und die Texte des Tray-Menüs |
| `src-tauri/src/win/` | Die Shell-Berührungspunkte |
| `src-tauri/src/win/theme.rs` | Helles oder dunkles Shell-Schema |
| `src-tauri/src/win/autostart.rs` | Der Run-Eintrag für „mit Windows starten" |
| `src-tauri/src/types.rs` | Was über die Brücke geht, und wie aus einem Tooltip eine Zeile wird |
| `src-tauri/build.rs` | Backt den Build-Stempel ein |
| `public/badges/` | Der not-by-humans-Badge, je eine Fassung pro Panel-Theme |
| `src-tauri/src/bin/probe.rs` | Konsolen-Frontend für Tests ohne UI |
| `scripts/build.ps1` | Release-Build |
| `config/settings.json` | Einstellungen (portabel) |

---

## Wann (Ablauf & Nutzung)

### Täglicher Betrieb
1. `TrayList.exe` starten (oder Autostart) — läuft still im Tray.
2. Pfeil in der Tray klicken wie immer.
3. Windows' Raster erscheint kurz, verschwindet, die Liste steht an derselben Stelle.
4. Tippen filtert, Pfeiltasten bewegen, `Enter` öffnet, Rechtsklick zeigt das Menü.
5. `Esc`, Klick daneben oder erneut auf den Pfeil schließt.

### Entwicklung
1. Node 20+, Rust (MSVC), `npm install`, `npm rebuild esbuild`.
2. `npm start` -> Tauri Dev.
3. Shell-Verhalten ohne UI prüfen: `cargo run --bin trailist-probe -- list|watch|hide|theme`.
4. Release: `.\scripts\build.ps1 -Portable`.

### Nach einem Windows-Update zuerst prüfen
`trailist-probe list` — wenn die Symbolzahl stimmt und die Namen plausibel sind,
sind Fensterklasse und Automation-Ids unverändert. Wenn nicht, zeigt `watch` am
schnellsten, was der neue Build liefert.

---

## Plan (Status)

### Erledigt (2026-10)
- [x] Machbarkeit empirisch bewiesen (Erkennung, UIA-Lesen, Hide, Klick-Forwarding)
- [x] Tauri-v2-Gerüst: Rust-Kern + React-Panel, portables Binary, ohne Konsolenfenster
- [x] Watcher-Zustandsautomat, Overlay-Geometrie inkl. DPI und Arbeitsbereich
- [x] Icons aus `IconSnapshot`, Zellraster aus dem gerenderten Flyout gemessen
- [x] Filter, Sortierung, Tastatur, Links- und Rechtsklick-Forwarding
- [x] Tray-Icon mit Menü, globaler Hotkey, `config/settings.json`
- [x] Pin über `IsPromoted`
- [x] `trailist-probe` als Test-Frontend
- [x] Handover von 2,2 s auf ~0,5 s gebracht (Registry-Prüfung einmalig statt pro
      Symbol, Positionen aus dem Raster statt aus UIA-Rechtecken)
- [x] Panel sitzt bündig auf der Taskleiste und mittig unter dem geklickten Chevron
- [x] Einstellungsdialog im Panel: Höhe, Abstand zur Taskleiste, Sortierung,
      Suchfeld, „immer sichtbare zuerst"
- [x] „Mit Windows starten" über den per-user Run-Eintrag
- [x] Version + Build-Stempel in Footer und Dialog (`build.rs`, `TRAILIST_BUILT`)
- [x] Panel folgt dem Shell-Farbschema (`SystemUsesLightTheme`)
- [x] not-by-humans-Badge im Dialog, hell und dunkel, mit Rücklink
- [x] Tooltip-Wiederholung im Namen entfernt, mit Test gegen echte Tooltips
- [x] Deutsch und Englisch in Panel, Tray-Menü und in den Fehlermeldungen
- [x] README mit Screenshots, Version, Lizenz-/Credit-Hinweis — als Paar, eine
      Fassung je Sprache
- [x] Repo veröffentlicht: `nopnop9090/trailist` mit Release `v0.3.0`

### Offen / optional
- [ ] **Reaktion beschleunigen / Raster ganz vermeiden.** Zwei Wege, beide mit
      Preis:
      1. *UIA-Events statt Polling.* Statt alle 40 ms `EnumWindows` zu fragen,
         auf `AutomationFocusChanged`/`WindowOpened` hören (bzw.
         `SetWinEventHook(EVENT_OBJECT_SHOW)` auf die Island-Klasse). Spart die
         ~60 ms, in denen das Raster sichtbar ist, ändert aber nichts an den
         ~200 ms, die die Shell zum Layouten braucht.
      2. *Windhawk-Mod in explorer.exe* (`@include explorer.exe`), der das Raster
         gar nicht erst zeichnet und stattdessen direkt unsere Liste öffnet. Das
         ist der einzige Weg, das Aufblitzen ganz loszuwerden — dafür läuft fremder
         Code im Explorer und die `SystemTray`-Symbole wandern auf neuen Builds
         (siehe windhawk-mods #4071). Vorbilder: `tray-hover-expand`,
         `tray-utility-customizer`.
- [ ] **Klicks ohne Nachspielen** (siehe unten) — dann fällt auch das kurze
      Wiederauftauchen des Flyouts beim Klick weg.
- [ ] Klick-Interception per `WH_MOUSE_LL`: globale Maus-Hooks liefern
      Koordinaten *und* verhindern die Weiterleitung des Klicks
      (`return 1`; nur so bleibt der Klick an unserer Liste hängen). Damit
      müsste man den Flyout nicht mehr zeigen — der Klick ginge an uns und wir
      senden ihn gezielt ans Icon. Haken: läuft im UI-Thread der App, braucht
      einen Nachrichten-Loop, ist pro Klick ein Hook-Roundtrip, und manche Spiele
      mit Raw Input umgehen den Hook trotzdem. `RegisterHotKey`-Hotkey würde
      dadurch sogar global konsumierbar — ein Ansatz für „Pfeil-Klick abfangen".
- [ ] Icon auch aus der Anwendung selbst holen, wenn es kein Snapshot gibt:
      `ExecutablePath` steht in `NotifyIconSettings`, `ExtractIconEx` liefert das
      echte Icon der Exe — scharf in jeder Größe, aber ggf. nicht der live
      Zustand (den das Snapshot ebenfalls nicht garantiert). UI Automation selbst
      hat keine Bilddaten; das Snapshot und die Exe sind die einzigen echten
      Quellen.
- [ ] **Liste per `Strg` + rechte Windows-Taste auslösen** (Wunsch). Die
      Windows-Taste ist als Hotkey heikel: Windows fängt sie selbst ab, und
      `RegisterHotKey` kann sie nicht als Modifier abbilden. Realistisch ist ein
      Low-Level-Tastatur-Hook (`WH_KEYBOARD_LL`) wie beim Klick-Punkt oben, der die
      Kombination erkennt und schluckt — oder eine freie Kombination ohne
      Windows-Taste.
- [ ] Hover-Tooltip nachspielen (die Ziel-App müsste ihren Tooltip zeigen)
- [ ] Flicker beim Klick-Forwarding ganz vermeiden: Flyout offscreen schieben und
      per `PrintWindow(PW_RENDERFULLCONTENT)` capturen, statt es zu zeigen
- [ ] Icons gruppieren/umordnen, Profile

### Risiken / Hinweise
- **Build-Änderungen.** Die Kette hängt an einer Fensterklasse und drei
  Automation-Ids. `trailist-probe` macht die Diagnose zu einer Minute Arbeit.
- **Auto-Hide-Taskleiste.** Das Flyout öffnet nur bei sichtbarer Taskleiste; die
  Geometrie kommt vom Flyout selbst, deshalb stimmt sie trotzdem. Der Hotkey macht
  die Liste davon unabhängig.
- **Vollbild-Apps.** Aktuell blendet TrayList dort nicht aus; ein Guard wie in
  `tray-hover-expand` wäre sinnvoll.
- **Tooltips sind kein Vertrag.** Apps entscheiden, was drinsteht.

---

## Schnellreferenz

```powershell
# Dev
npm start

# Shell-Verhalten ohne UI testen
cargo run --manifest-path .\src-tauri\Cargo.toml --bin trailist-probe -- list
cargo run --manifest-path .\src-tauri\Cargo.toml --bin trailist-probe -- watch

# Release
.\scripts\build.ps1 -Portable

# Einstellungen
notepad .\config\settings.json
```

Siehe auch: [README.md](../README.md) · [README.de.md](../README.de.md) ·
[PLAN.en.md](PLAN.en.md) (englisch).