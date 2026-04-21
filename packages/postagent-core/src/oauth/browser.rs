use std::io::Write;
use std::path::PathBuf;

/// Opens `url` in the system browser. Returns `false` when the platform can't
/// launch a browser so the caller can fall back to a manual flow.
pub fn open(url: &str) -> bool {
    webbrowser::open(url).is_ok()
}

/// Persists the full authorize URL in a 0600 temp file so the user can open
/// it manually without echoing sensitive query parameters to stderr.
pub fn write_manual_url(url: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let mut file = tempfile::Builder::new()
        .prefix("postagent-authorize-url-")
        .suffix(".txt")
        .tempfile()?;
    writeln!(file, "{}", url)?;
    let (_file, path) = file.keep()?;
    Ok(path)
}
