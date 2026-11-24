//! Self-update from GitHub Releases, the way PartyLine does it: at start, in the background, an
//! installed copy looks for a newer release, and when there is one it downloads this system's
//! binary and swaps it in for the running file (which keeps running). The next run is the new
//! version. Trust is HTTPS to this repo's releases, the same as running the install line.
//!
//! Copies that never update: dev builds (anything under a cargo `target` folder), and any run
//! with `HOSTLINE_NO_UPDATE` set, which the npm package sets because npm picks its version.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;

const REPO: &str = "luannzin/hostline";
/// This system's binary on every release (named by .github/workflows/release.yml).
const ASSET: &str = if cfg!(windows) {
    "hostline-windows-x86_64.exe"
} else if cfg!(target_os = "macos") {
    "hostline-macos"
} else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
    "hostline-linux-x86_64"
} else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
    "hostline-linux-arm64"
} else {
    ""
};

/// The release tag once a newer version is in place. Any failure (no internet, a folder it
/// can't write) is a run without an update, never an error.
pub async fn run() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let _ = std::fs::remove_file(old(&exe)); // left behind by the last update on Windows
    let dev = exe.ancestors().any(|a| a.file_name().is_some_and(|n| n == "target"));
    if dev || ASSET.is_empty() || std::env::var_os("HOSTLINE_NO_UPDATE").is_some() {
        return None;
    }
    let part = exe.with_extension("part");
    match update(&exe, &part, env!("CARGO_PKG_VERSION")).await {
        Ok(tag) => tag,
        Err(_) => {
            let _ = std::fs::remove_file(&part);
            None
        }
    }
}

/// Replaces `exe` with the latest release's binary when that release is newer than `current`.
async fn update(exe: &Path, part: &Path, current: &str) -> Result<Option<String>> {
    // /releases/latest redirects to /releases/tag/<tag>: no API call, no rate limit.
    let latest = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .build()?
        .get(format!("https://github.com/{REPO}/releases/latest"))
        .send()
        .await?;
    let tag = latest
        .headers()
        .get("location")
        .and_then(|l| l.to_str().ok())
        .and_then(|l| l.rsplit('/').next())
        .unwrap_or_default()
        .to_string();
    if !newer(&tag, current) {
        return Ok(None);
    }
    let binary = reqwest::Client::builder()
        .timeout(Duration::from_secs(600))
        .build()?
        .get(format!("https://github.com/{REPO}/releases/download/{tag}/{ASSET}"))
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    tokio::fs::write(part, &binary).await?;
    swap(part, exe)?;
    Ok(Some(tag))
}

/// Whether release tag `tag` ("v1.2.3") is a newer version than `current` ("1.2.0"). A tag that
/// isn't a plain version never is.
fn newer(tag: &str, current: &str) -> bool {
    let parse = |v: &str| v.split('.').map(|n| n.parse::<u64>().ok()).collect::<Option<Vec<_>>>();
    match (tag.strip_prefix('v').and_then(parse), parse(current)) {
        (Some(new), Some(cur)) => new > cur,
        _ => false,
    }
}

fn old(exe: &Path) -> PathBuf {
    exe.with_extension("old")
}

#[cfg(unix)]
fn swap(new: &Path, exe: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(new, std::fs::Permissions::from_mode(0o755))?;
    // The running process keeps the replaced file open; rename is atomic.
    std::fs::rename(new, exe)
}

#[cfg(windows)]
fn swap(new: &Path, exe: &Path) -> std::io::Result<()> {
    // A running exe can't be overwritten, but it can be renamed out of the way.
    std::fs::rename(exe, old(exe))?;
    std::fs::rename(new, exe).inspect_err(|_| {
        let _ = std::fs::rename(old(exe), exe);
    })
}

#[cfg(test)]
mod tests {
    use super::newer;

    #[test]
    fn only_plain_newer_versions_update() {
        assert!(newer("v0.2.1", "0.2.0"));
        assert!(newer("v0.10.0", "0.9.9"));
        assert!(!newer("v0.2.0", "0.2.0"));
        assert!(!newer("v0.1.9", "0.2.0"));
        assert!(!newer("v0.3.0-beta", "0.2.0"));
        assert!(!newer("", "0.2.0"));
        assert!(!newer("0.3.0", "0.2.0"));
    }
}
