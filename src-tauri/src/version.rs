//! The version label the panel shows.
//!
//! One build stamp for the whole product, taken when the code was compiled rather
//! than when it runs: "v0.2.0 2026-10-03 15:04" answers "which build am I looking
//! at", which is the question a screenshot in a bug report has to settle.

/// The crate version.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// When this build was made, as `YYYY-MM-DD HH:MM`.
///
/// Written by `build.rs`. The fallback covers the case where the build script did
/// not run, which happens when an editor analyses the sources on their own.
pub fn built() -> &'static str {
    option_env!("TRAILIST_BUILT").unwrap_or("unbekannt")
}

/// Version and build stamp in the one line the panel shows.
pub fn label() -> String {
    format!("v{} {}", version(), built())
}