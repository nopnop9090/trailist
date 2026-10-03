//! A console front end for the same machinery the app uses.
//!
//! It exists so the shell interaction can be tested without a webview, a build
//! step or a window in the way: `list` prints what the app would show, `hide`
//! puts a stray flyout away, `watch` reports flyout openings as they happen and
//! `theme` prints the colour scheme the panel would paint itself in.

use std::time::Duration;

use trailist_lib::win::{island, uia};

fn main() {
    match std::env::args().nth(1).unwrap_or_else(|| "list".into()).as_str() {
        "list" => list(),
        "hide" => hide(),
        "watch" => watch(),
        "theme" => theme(),
        other => eprintln!("unbekannter Befehl '{other}'. Erwartet: list | hide | watch | theme"),
    }
}

fn reader() -> Option<uia::Reader> {
    match uia::Reader::new() {
        Ok(reader) => Some(reader),
        Err(error) => {
            eprintln!("UI Automation nicht verfuegbar: {error}");
            None
        }
    }
}

/// Prints the colour scheme the panel would paint itself in.
///
/// Worth its own command because the answer comes from the registry rather than
/// from the webview, so a wrong-looking panel can be told apart from a
/// wrong-reading one without opening anything.
fn theme() {
    let dark = trailist_lib::win::theme::is_dark();
    println!("Shell-Theme: {}", if dark { "dunkel" } else { "hell" });
}

/// Prints the list the panel would show, opening the flyout first if needed.
fn list() {
    let Some(reader) = reader() else { return };

    let mut flyout = island::visible().into_iter().next();
    if flyout.is_none() {
        println!("Flyout ist zu, bitte den Pfeil oeffnen lassen ...");
        if let Some(taskbar) = island::find_by_class(island::TASKBAR_CLASS) {
            match reader.chevron(island::raw(taskbar)) {
                Ok(Some(chevron)) => {
                    let _ = reader.invoke(&chevron);
                }
                other => println!("Pfeil nicht gefunden: {other:?}"),
            }
        }
        std::thread::sleep(Duration::from_millis(700));
        flyout = island::visible().into_iter().next();
    }

    let Some(flyout) = flyout else {
        println!("Kein Flyout gefunden.");
        return;
    };

    let window = island::hwnd(island::raw(flyout));
    match island::rect(window) {
        Some(rect) => println!(
            "Flyout 0x{:X} bei {},{} bis {},{}",
            island::raw(flyout),
            rect.left,
            rect.top,
            rect.right,
            rect.bottom
        ),
        None => println!("Flyout 0x{:X} ohne Rechteck", island::raw(flyout)),
    }

    let items = reader
        .items_settled(island::raw(flyout), Duration::from_millis(400))
        .unwrap_or_default();

    println!("{} Symbole:", items.len());
    for (index, item) in items.iter().enumerate() {
        let (centre_x, centre_y) = item.center();
        println!(
            "  {:>3}  ({:>5},{:>5})  {}",
            index,
            centre_x,
            centre_y,
            item.name.replace('\n', " | "),
        );
    }
}

/// Puts any flyout that is currently on screen back into hiding.
fn hide() {
    let open = island::visible();
    if open.is_empty() {
        println!("Kein Flyout offen.");
        return;
    }
    for handle in open {
        println!("verstecke 0x{:X}", island::raw(handle));
        island::hide(handle);
    }
}

/// Reports flyout openings, and how many icons were readable in each.
fn watch() {
    let Some(reader) = reader() else { return };
    println!("beobachte (Strg+C beendet) ...");

    let mut open: Option<isize> = None;
    loop {
        let visible = island::visible().into_iter().next().map(island::raw);

        match (visible, open) {
            (Some(now), None) => {
                println!(">>> Flyout offen (0x{now:X})");
                let items = reader
                    .items_settled(now, Duration::from_millis(300))
                    .unwrap_or_default();
                println!("    {} Symbole gelesen", items.len());
                for item in items.iter().take(5) {
                    println!("      {}", item.name.replace('\n', " | "));
                }
                open = Some(now);
            }
            (None, Some(_)) => {
                println!("<<< Flyout zu");
                open = None;
            }
            _ => {}
        }

        std::thread::sleep(Duration::from_millis(20));
    }
}