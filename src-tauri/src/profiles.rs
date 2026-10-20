use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
pub fn discover(executable: &Path) -> Result<Vec<PathBuf>> {
    let mut profiles = Vec::new();
    for root in candidate_roots(executable) {
        let root = match root.canonicalize() {
            Ok(root) => root,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        profiles.extend(in_root(&root)?);
    }
    profiles.sort();
    profiles.dedup();
    Ok(profiles)
}

fn in_root(root: &Path) -> Result<Vec<PathBuf>> {
    let mut profiles = Vec::new();
    for entry in fs::read_dir(root).context("Cannot discover Chrome profiles")? {
        let entry = entry?;
        if entry.file_name() == "Guest Profile" || entry.file_name() == "System Profile" {
            continue;
        }
        if entry.path().join("Preferences").is_file() {
            let profile = entry.path().canonicalize()?;
            anyhow::ensure!(
                profile.parent() == Some(root),
                "Chrome profile points outside its User Data folder"
            );
            profiles.push(profile);
        }
    }
    Ok(profiles)
}

fn candidate_roots(executable: &Path) -> Vec<PathBuf> {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    let executable = executable.to_string_lossy().to_lowercase();
    let mut roots = Vec::new();
    #[cfg(target_os = "windows")]
    if let Some(local) = env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        let product = if executable.contains("chrome for testing") {
            "Google/Chrome for Testing/User Data"
        } else if executable.contains("chrome sxs") {
            "Google/Chrome SxS/User Data"
        } else if executable.contains("chrome beta") {
            "Google/Chrome Beta/User Data"
        } else if executable.contains("chrome dev") {
            "Google/Chrome Dev/User Data"
        } else if executable.contains("chromium") {
            "Chromium/User Data"
        } else {
            "Google/Chrome/User Data"
        };
        roots.push(local.join(product));
    }
    #[cfg(target_os = "linux")]
    if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
        let config = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        let product = if executable.contains("chromium") {
            "chromium"
        } else if executable.contains("google-chrome-beta") {
            "google-chrome-beta"
        } else if executable.contains("google-chrome-unstable") {
            "google-chrome-unstable"
        } else {
            "google-chrome"
        };
        roots.push(config.join(product));
    }
    #[cfg(target_os = "macos")]
    if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
        let support = home.join("Library/Application Support");
        roots.push(support.join("Google/Chrome"));
        roots.push(support.join("Chromium"));
    }
    roots
}
