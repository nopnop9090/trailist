// A tray app must never flash a console window, in any profile. Everything the
// debug tracing says goes to logs/trailist.log instead, which is where it is
// useful anyway.
#![windows_subsystem = "windows"]

fn main() {
    trailist_lib::run()
}