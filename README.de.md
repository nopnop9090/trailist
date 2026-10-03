# TrayList

Zeigt die versteckten Windows-Tray-Symbole als **scrollbare Liste mit Namen**
statt als Raster aus Piktogrammen, die man einzeln anfahren muss. Die
englischsprachige Fassung steht in [README.md](README.md).

*[English](README.md) · Deutsch*

Klick auf den Pfeil in der Taskleiste, wie immer. TrayList liest das Flyout, das
Windows gerade geöffnet hat, legt es weg und zeigt an dessen Stelle ein eigenes
Panel: eine Zeile pro Symbol, links das Piktogramm, rechts Name und aktueller
Zustand, dazu ein Filterfeld für den Fall, dass es vierzig sind.

```
  ▸ du klickst ^ im Tray
  ▸ TrayList liest die Symbole und ihre Tooltips und holt sich deren Bitmaps
  ▸ das Windows-Raster verschwindet, die Liste erscheint an seiner Stelle
  ▸ Klick auf eine Zeile  -> das eigene Menü der App öffnet sich, genau wie vorher
  ▸ Esc / danebenklicken / nochmal der Pfeil -> die Liste geht weg
```

Gebaut als [Tauri v2](https://tauri.app)-App: ein Rust-Kern, der mit der Shell
spricht, und ein React-Panel für die Liste. Keine Injektion, keine Shell-Hooks,
keine Erhöhung der Rechte.

![Die Liste: eine Zeile pro verstecktem Symbol, mit Namen, Zuständen und Filterfeld](docs/screenshot-list.png)

## Was, wer, wann, wie

| | |
|---|---|
| **Was** | Der Windows-11-Tray-Überlauf als benannte und filterbare Liste statt als Raster aus 16×16-Piktogrammen. |
| **Wer** | Geschrieben für einen sehr vollen Tray, mit einem KI-Assistenten als Tippkraft — darum geht es beim Badge unten. |
| **Wann** | Version `0.3.0`. Der Build-Stempel wird beim Kompilieren eingebacken und im Footer des Panels, im Einstellungsdialog und für `trailist-probe theme` angezeigt. |
| **Wie** | Ein Rust-Kern liest das Flyout der Shell über UI Automation und nimmt dessen Platz ein; ein React-Panel zeichnet die Liste. Die Einstellungen liegen in `config/settings.json` neben der Exe. |

Der Einstellungsdialog, samt Badge und Version:

![Der Einstellungsdialog: Höhengrenze, Abstand zur Taskleiste, Autostart und der Über-Block](docs/screenshot-settings.png)

## Warum es das gibt

Windows 11 versteckt neue Tray-Symbole hinter einem Pfeil und zeigt sie in einem
festen Raster unbeschrifteter 16×16-Piktogramme. Bei ein paar Dutzend Symbolen
heißt das: anfahren, auf den Tooltip warten, raten. Die *Namen* gibt es — es sind
die Tooltips — sie stehen nur nirgends auf einmal. Genau das löst dieses Programm.

## Funktionen

**Die Liste**
- Piktogramm, Name und aktueller Zustand pro Zeile — der Tooltip ist der Name und
  oft der nützlichste Text auf dem Rechner (`GPU: Eco | 250 W | 51 C`,
  `AdGuard VPN — Türkei (Istanbul): Getrennt`, `TrayMaster - 3/3 running`)
- Filterfeld mit Fokus: ein paar Buchstaben tippen, die Liste zieht sich zusammen
- Sortierung: die Reihenfolge der Shell selbst oder alphabetisch
- Tastatur: `↑`/`↓` bewegen, `Enter` öffnet, `Esc` schließt
- Rechtsklick auf eine Zeile öffnet das Kontextmenü des Symbols
- Panel passend zum Inhalt, auf den geklickten Pfeil zentriert und auf der
  Taskleiste aufsitzend, immer innerhalb des Arbeitsbereichs des richtigen Monitors
- Folgt dem Farbschema der Shell: die Palette kommt aus `SystemUsesLightTheme`,
  derselben Einstellung, der Taskleiste und Flyout folgen — das Panel stimmt also
  auch dann mit dem Tray überein, wenn Windows für App-Fenster etwas anderes
  eingestellt hat

**Einstellungen** (das Zahnrad im Panel oder `config/settings.json`)
- Wie hoch die Liste werden darf, bis gut 20 Zeilen ohne Scrollen
- Wie viel Luft zwischen Panel und Taskleiste bleibt, von bündig bis 40 px
- Mit Windows starten, über den Autostart-Eintrag, den Windows selbst liest
- Filterfeld an oder aus, immer sichtbare zuerst, alphabetisch oder Tray-Reihenfolge
- Sprache: `System`, `Deutsch` oder `English`

**Tray-Eigenschaften**
- Optionaler Pin-Knopf pro Zeile: schaltet ein Symbol im *sichtbaren* Teil des Tray
  ein oder aus, indem er das `IsPromoted`-Flag der Shell schreibt
- Symbole, die gerade im Tray sichtbar sind, werden als solche gekennzeichnet

**Hinkommen**
- Klick auf den Pfeil wie gewohnt, oder
- `Alt+Shift+T` von überall, oder
- das Menü des Tray-Symbols

## Voraussetzungen

- Windows 11 (Build 22000 oder neuer). Gebaut ist es gegen den XAML-Tray von
  Windows 11; unter Windows 10 hat das Flyout eine andere Form und die Liste
  findet es nicht.
- WebView2, das mit Windows 11 ausgeliefert wird.

## Loslegen

```powershell
npm install
npm rebuild esbuild   # npm führt esbuilds Postinstall nicht von sich aus aus
npm start             # tauri dev
```

Release-Build (Exe plus NSIS-Installer):

```powershell
.\scripts\build.ps1 -Portable
```

Die portable Exe landet in `dist-app/portable/` und, nach einem Lauf des Skripts,
zusätzlich als `TrayList.exe` im Repo-Wurzelverzeichnis neben `config/`.

## Ohne Oberfläche testen

`trailist-probe` treibt dieselbe Maschinerie von einer Konsole aus, was der
schnellste Weg ist, die Shell-Interaktion nach einem Windows-Update zu prüfen:

```powershell
cargo run --manifest-path .\src-tauri\Cargo.toml --bin trailist-probe -- list
cargo run --manifest-path .\src-tauri\Cargo.toml --bin trailist-probe -- watch
cargo run --manifest-path .\src-tauri\Cargo.toml --bin trailist-probe -- hide
cargo run --manifest-path .\src-tauri\Cargo.toml --bin trailist-probe -- theme
```

`list` gibt aus, was das Panel zeigen würde (und öffnet das Flyout, wenn es
geschlossen ist), `watch` meldet jede Öffnung und wie viele Symbole lesbar waren,
`hide` legt ein herumstehendes Flyout weg, und `theme` gibt das Farbschema aus,
das das Panel verwenden würde — letzteres direkt aus der Registry, damit sich ein
Panel, das falsch aussieht, von einem unterscheiden lässt, das den falschen Wert
gelesen hat.

Mit gesetztem `TRAILIST_DEBUG=1` erzählt die App die Übergabe in
`logs/trailist.log` nach — inklusive, wie lange das Lesen gedauert hat und wie
viele Zellen das gezeichnete Raster hatte — und schreibt jedes Lesen nach
`logs/last-read.txt` (Index, Klickkoordinaten, Symbolgröße und Titel pro Zeile).
Diese beiden Dateien sind der schnellste Weg zu sehen, was die Shell tatsächlich
gemeldet hat.

## Einstellungen

`config/settings.json` neben der Exe (oder `%APPDATA%\TrayList`, wenn dieser
Ordner schreibgeschützt ist). Das Zahnrad im Panel bearbeitet dieselbe Datei; das
Tray-Menü öffnet den Ordner.

| Schlüssel | Standard | Bedeutung |
|---|---|---|
| `panelWidth` | 344 | Breite des Panels in logischen Pixeln. Das native Flyout ist nur ~234 breit, also ist Platz für die Namen. |
| `panelMaxHeight` | 900 | Obergrenze; darüber scrollt die Liste. Absichtlich großzügig: auf einem großen Bildschirm gibt es keinen Grund, eine Liste zu scrollen, die gepasst hätte. |
| `edgeGap` | 0 | Luft zwischen Panel und Taskleiste in logischen Pixeln. `0` setzt das Panel auf die Taskleiste, so wie es das native Flyout tut. |
| `iconSize` | 16 | Symbolgröße in den Zeilen. |
| `sort` | `tray` | `tray` oder `name`. |
| `showSearch` | true | Filterfeld anzeigen. |
| `hotkey` | `Alt+Shift+T` | Globaler Hotkey. `null` schaltet ihn ab. |
| `pinnedFirst` | false | Symbole, die im Tray sichtbar sind, zuerst einsortieren. |
| `lang` | `system` | `de`, `en` oder `system` für die Sprache der Windows-Oberfläche. Alles andere zählt als `system`. |

Der Autostart steht nicht in dieser Datei: er ist ein echter Eintrag unter
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, also dort, wo Windows
nachsieht und wo der Autostart-Tab des Task-Managers ihn wieder abschalten kann.

## Wie es arbeitet

```
watcher-Thread (besitzt UI Automation und jede Shell-Interaktion)
  |
  |- island.rs   Flyout-Fenster finden   TopLevelWindowForOverflowXamlIsland
  |- uia.rs      Symbole lesen           Buttons mit AutomationId=NotifyItemIcon,
  |                                      deren Name der vollständige Tooltip ist
  |- capture.rs  Bitmaps rendern         PrintWindow(PW_RENDERFULLCONTENT) des
  |                                      Flyouts, dann dessen Hintergrund auskeyen
  |- suppress    ShowWindow(island, SW_HIDE)  das Raster geht weg, Explorer-Zustand bleibt
  |- overlay     unser Panel, gesetzt auf das Rechteck des Flyouts selbst
  |- forward.rs  einen Klick nachspielen SW_SHOWNOACTIVATE, SendInput, wieder SW_HIDE
```

Der klassische Windows-10-Weg in den Tray — ein `ToolbarWindow32` in
`TrayNotifyWnd`, gelesen mit `TB_GETBUTTON` und `TRAYDATA` — ist weg. Unter
Windows 11 ist der Infobereich eine XAML-Insel, und in neueren Windows-11-Builds
sind selbst die `SystemTray`-Implementierungsklassen aus `Taskbar.View.dll`
verschwunden. UI Automation ist die Schnittstelle, die übrig blieb, und die
Tooltips der Symbole kommen mit ihr.

Details, die man kennen sollte, weil sie ausmachen, dass es sich nativ anfühlt:

- Das Flyout wird mit `SW_HIDE` versteckt statt geschlossen. Explorer behält seinen
  eigenen Zustand, sodass der nächste Klick auf den Pfeil das Flyout genau wie
  vorher öffnet, statt auf einem halb geschlossenen Fenster zu landen.
- Ein nachgespielter Klick zeigt das Flyout kurz, *ohne es zu aktivieren*, damit
  der Klick auf dem Symbol landet und nicht von einem Fokuswechsel geschluckt wird.
- Die Bitmaps kommen zuerst aus der Shell: `HKCU\Control Panel\NotifyIconSettings`
  hält einen PNG-Schnappschuss jedes Symbols samt seinem ersten Tooltip, und Zeilen
  werden darüber zugeordnet, sodass das Symbol das echte ist, mit Alpha und allem.
  Nur Symbole ohne Schnappschuss werden aus einem
  `PrintWindow(PW_RENDERFULLCONTENT)`-Rendering des Flyouts geschnitten und gegen
  dessen Hintergrund gekeyt.
- Wo ein Symbol *ist*, wird nur der Zeichnung geglaubt. UI Automation meldet
  Rechtecke, die sich während der Flyout-Animation noch bewegen können — ein
  Rechteck eine halbe Zelle daneben schneidet ein Quadrat Hintergrund aus, mit dem
  Symbol unten hineingequetscht. Deshalb wird das Zellenraster aus dem Rendering
  gemessen: „nicht die Hintergrundfarbe" auf beide Achsen projizieren und das
  Ergebnis in gleichmäßige Bänder schneiden. Daher stammt auch die Reihenfolge der
  Liste — aus der Aufzählung der Shell selbst, nicht aus den wackelnden Rechtecken.
- Das Panel holt sich den Vordergrund mit `SetForegroundWindow`, zweimal: Windows
  lehnt den ersten Aufruf oft ab, wenn der Klick, der das Panel öffnete, an die
  Taskleiste ging, und eine wegkippende Auto-Hide-Taskleiste kann den Vordergrund
  wieder zurückgeben.

## Bekannte Kompromisse

- **Das native Raster steht rund 60 ms auf dem Bildschirm.** UI Automation sieht die
  Symbole nur, solange deren Fenster sichtbar ist, die Übergabe kann also nicht
  beginnen, bevor die Shell das Flyout öffnet. Alles danach ist versteckt: das
  Flyout wird vom Bildschirm geschoben, sobald es gefunden ist, und dort gelesen und
  gerendert. Vom Klick bis zur Liste dauert es etwa eine halbe Sekunde, davon das
  meiste Warten darauf, dass die Shell die Symbole fertig angeordnet hat.
- **Ein Klick braucht das Flyout für rund 300 ms zurück.** Die Shell leitet den
  Klick auf ein Symbol nur weiter, solange das Symbol auf dem Bildschirm steht, also
  legt ein nachgespielter Klick das Flyout kurz wieder darunter. Das Panel ist
  vorher versteckt, weshalb das Menü der App erscheint, ohne dass unser Fenster den
  Klick schluckt.
- **Die Symbole sind die der Shell, ein brandneues hat also vielleicht noch keins.**
  Windows hält unter `HKCU\Control Panel\NotifyIconSettings` einen Schnappschuss
  jedes Tray-Symbols; ein Symbol, das die Shell noch nicht gespeichert hat, fällt
  auf eine Zelle aus einem Rendering des Flyouts zurück, gegen dessen Hintergrund
  gekeyt.
- **Hover-Tooltips werden nicht nachgespielt.** Die Maus über eine Zeile zu bewegen,
  lässt die besitzende App ihren Hover-Text nicht zeigen, weil das kein Klick ist.
- **Der Tooltip ist der Name.** Was darin steht, entscheiden die Anwendungen, also
  lesen sich ein paar Zeilen wie ein Status statt wie ein Name. Wo eine Anwendung
  ihren eigenen Namen zweimal schreibt — `TrayMaster TrayMaster - 3/3 running` —
  wird die Wiederholung entfernt, weil nur die Wiederholung dort falsch ist.
- **Kein Vordergrund, keine Tastatur.** Das Panel erscheint, weil du auf die
  *Taskleiste* geklickt hast, also darf Windows uns den Vordergrund verweigern.
  TrayList besteht zweimal darauf; scheitert das, funktioniert die Liste weiterhin
  mit der Maus.
- **Eine versteckte Taskleiste versteckt den Pfeil.** Mit Auto-Hide ist der Pfeil
  nicht im Baum, solange die Taskleiste unten ist, `Alt+Shift+T` hat dann nichts zum
  Auslösen und meldet, dass es ihn nicht gefunden hat. Der Klick auf den Pfeil ist
  nicht betroffen, weil genau der die Taskleiste erst hochholt.
- **Fehlermeldungen sprechen zwei Sprachen.** Der Satz kommt aus dem Panel, also in
  der eingestellten Sprache; was Windows im Detail dazu sagt, wird unverändert
  angehängt — es ist Windows' eigene Formulierung in Windows' eigener Sprache.

## Aufbau

```
src/                     React-Panel (die Liste und der Einstellungsdialog)
src/lib/i18n.ts          die deutschen und englischen Texte des Panels
src-tauri/src/
  watcher.rs             die Zustandsmaschine: erkennen, lesen, verstecken, zeigen, schließen
  overlay.rs             Geometrie und Platzierung des Panels
  commands.rs            die IPC-Oberfläche
  settings.rs            config/settings.json
  types.rs               was über die Brücke geht, und wie aus einem Tooltip eine Zeile wird
  i18n.rs                Sprachauswahl und die Texte des Tray-Menüs
  version.rs             die Versionszeile, die das Panel zeigt
  win/island.rs          das Überlauf-Flyout-Fenster
  win/uia.rs             die Symbole lesen
  win/capture.rs         die Symbol-Bitmaps
  win/forward.rs         Klicks nachspielen
  win/registry.rs        NotifyIconSettings, IsPromoted
  win/theme.rs           welches Farbschema die Shell zeichnet
  win/autostart.rs       der Run-Eintrag des Benutzers
  win/launch.rs          einen Link im Browser öffnen
  win/focus.rs           den Vordergrund holen
  bin/probe.rs           Konsolen-Oberfläche zum Testen
src-tauri/build.rs       stempelt den Build mit der lokalen Zeit des Compiler-Rechners
public/badges/           das not-by-humans-Badge, eines pro Panel-Theme
scripts/build.ps1        Release-Build
scripts/make-icons.ps1   erzeugt den Symbolsatz
docs/PLAN.md             was das ist, warum es so gebaut ist, was als Nächstes kommt
docs/screenshot-*.png    die zwei Bilder oben
```

## Tests

```powershell
cargo test --manifest-path .\src-tauri\Cargo.toml
```

Drei Tests. Einer gilt dem Stück mit echtem Raten: aus den Tooltips, die der Tray
dieses Rechners tatsächlich produziert, einen Namen und einen Zustand zu machen.
Jeder Fall darin kommt von einem echten Tray, auch die unangenehmen — der Name auf
einer Zeile wiederholt, über zwei Zeilen wiederholt, ein Name, der nur ein Wort
teilt, und ein Tooltip, der nichts als der zweimalige Name ist. Die beiden anderen
prüfen die Sprachauswahl: dass eine Einstellung vor der Systemsprache gilt und dass
jede Sprache ein vollständiges Tray-Menü hat.

## Dank

Das Badge ist von [not by humans](https://notbyhumans.fyi) — *developed by AI, not
by humans*. Tinte auf einem dunklen Panel, Papier auf einem hellen; die Kopien in
`public/badges` sind die Dateien, die die Seite ausliefert, und ein Klick auf das
Badge führt zurück zu der Seite, die es erklärt.

<a href="https://notbyhumans.fyi"><img src="https://notbyhumans.fyi/badges/developed-paper.svg" width="165" height="54" alt="Developed by AI, not by humans"></a>

