fn main() {
    // Re-run whenever the sources change. Cargo caches what a build script
    // printed, so without this the stamp would freeze at the first compilation
    // and a new binary would carry an old date — the one thing a build stamp must
    // not do. When nothing changed there is no new binary either, so staying put
    // is then the correct answer.
    println!("cargo:rerun-if-changed=src");

    // The panel names the build it belongs to, and an exe cannot tell when it was
    // compiled. So the stamp is taken here and handed to the code as an
    // environment variable, which is the one moment it is knowable.
    println!("cargo:rustc-env=TRAILIST_BUILT={}", local_stamp());
    tauri_build::build()
}

/// The current local time as `YYYY-MM-DD HH:MM`.
///
/// Deliberately no date crate: this runs once per build, and the compiler host is
/// always Windows here, so the platform call is both shorter and exact — it
/// already knows the machine's time zone and DST rules.
#[cfg(windows)]
fn local_stamp() -> String {
    use windows::Win32::System::SystemInformation::GetLocalTime;

    let time = unsafe { GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        time.wYear, time.wMonth, time.wDay, time.wHour, time.wMinute
    )
}

#[cfg(not(windows))]
fn local_stamp() -> String {
    "unbekannt".to_string()
}