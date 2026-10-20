#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    path::{Path, PathBuf},
    process::{Child, Command},
    sync::Mutex,
};

use anyhow::{Context, Result, bail, ensure};
use sysinfo::System;
use tauri::{Manager, State};
use tauri_plugin_opener::OpenerExt;

struct Browser {
    executable: PathBuf,
    pak: PathBuf,
}

#[derive(Default)]
struct Session(Mutex<Option<Child>>);

fn browser(path: &str) -> Result<Browser> {
    let mut executable = PathBuf::from(path)
        .canonicalize()
        .context("Select a Chrome executable")?;
    if cfg!(target_os = "linux")
        && let Some(parent) = executable.parent()
    {
        let binary = parent.join("chrome");
        if binary.is_file() {
            executable = binary.canonicalize()?;
        }
    }
    ensure!(
        executable.is_file(),
        "Select the executable inside the Chrome installation"
    );
    let parent = executable
        .parent()
        .context("Executable has no parent folder")?;
    let adjacent = parent.join("resources.pak");
    if adjacent.is_file() {
        return Ok(Browser {
            executable,
            pak: adjacent,
        });
    }

    #[cfg(target_os = "windows")]
    {
        let mut versions = Vec::new();
        for entry in std::fs::read_dir(parent)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Ok(version) = name
                .split('.')
                .map(str::parse::<u32>)
                .collect::<std::result::Result<Vec<_>, _>>()
            {
                let pak = entry.path().join("resources.pak");
                if version.len() == 4 && pak.is_file() {
                    versions.push((version, pak));
                }
            }
        }
        versions.sort_by(|a, b| b.0.cmp(&a.0));
        if let Some((_, pak)) = versions.into_iter().next() {
            return Ok(Browser { executable, pak });
        }
    }
    #[cfg(target_os = "macos")]
    {
        let frameworks = parent
            .parent()
            .context("Missing app Contents folder")?
            .join("Frameworks");
        for entry in std::fs::read_dir(frameworks)? {
            let pak = entry?
                .path()
                .join("Versions/Current/Resources/resources.pak");
            if pak.is_file() {
                return Ok(Browser { executable, pak });
            }
        }
    }
    bail!(
        "No resources.pak found for this executable. Use an unpacked Chrome for Testing installation"
    )
}

fn ensure_closed(executable: &Path) -> Result<()> {
    let system = System::new_all();
    let name = executable
        .file_name()
        .context("Executable has no filename")?;
    for process in system.processes().values() {
        if process.exe() != Some(executable) && !process.name().eq_ignore_ascii_case(name) {
            continue;
        }
        let path = process.exe().context("Cannot inspect a running Chrome process. Close Chrome or run ChromeThemer outside the container")?;
        let path = match path.canonicalize() {
            Ok(path) => path,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let filename = path.to_string_lossy();
                PathBuf::from(filename.strip_suffix(" (deleted)").unwrap_or(&filename))
            }
            Err(e) => return Err(e).context("Cannot resolve a running Chrome executable"),
        };
        ensure!(
            path != executable,
            "Close this Chrome installation before applying or restoring a theme"
        );
    }
    Ok(())
}

#[tauri::command]
async fn inspect_browser(
    executable: String,
) -> std::result::Result<chromethemer::Inspection, String> {
    tauri::async_runtime::spawn_blocking(move || chromethemer::inspect(&browser(&executable)?.pak))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn apply_theme(
    executable: String,
    theme: String,
    allow_unsafe_theme_files: bool,
) -> std::result::Result<chromethemer::Inspection, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let browser = browser(&executable)?;
        ensure_closed(&browser.executable)?;
        let files = if allow_unsafe_theme_files {
            chromethemer::ThemeFiles::Unsafe
        } else {
            chromethemer::ThemeFiles::CssOnly
        };
        chromethemer::apply(&browser.pak, Path::new(&theme), files)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn restore_browser(
    executable: String,
) -> std::result::Result<chromethemer::Inspection, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let browser = browser(&executable)?;
        ensure_closed(&browser.executable)?;
        chromethemer::restore(&browser.pak)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn launch_browser(
    executable: String,
    session: State<'_, Session>,
) -> std::result::Result<(), String> {
    let launch = || -> Result<()> {
        let browser = browser(&executable)?;
        ensure_closed(&browser.executable)?;
        let mut slot = session
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("Browser session lock failed"))?;
        if let Some(child) = slot.as_mut() {
            ensure!(child.try_wait()?.is_some(), "Chrome is already open");
        }
        *slot = Some(
            Command::new(browser.executable)
                .args([
                    "--enable-features=Webium,SurfaceEmbed,ExtensionsMenuAccessControl",
                    "--disable-features=WebUIBundledCodeCache",
                ])
                .spawn()
                .context("Cannot launch Chrome")?,
        );
        Ok(())
    };
    launch().map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn open_theme(theme: String, app: tauri::AppHandle) -> std::result::Result<(), String> {
    let folder = PathBuf::from(theme)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !folder.is_dir() {
        return Err("Select a theme folder".into());
    }
    app.opener()
        .open_path(folder.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(Session::default())
        .invoke_handler(tauri::generate_handler![
            inspect_browser,
            apply_theme,
            restore_browser,
            launch_browser,
            open_theme
        ])
        .build(tauri::generate_context!())
        .expect("Could not start ChromeThemer")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event
                && let Ok(mut session) = app.state::<Session>().0.lock()
                && let Some(child) = session.as_mut()
            {
                // The launched browser outlives this window; only reap an exited launcher.
                if let Err(e) = child.try_wait() {
                    eprintln!("Cannot reap Chrome launcher: {e}");
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_executable_is_rejected() {
        assert!(
            browser("/missing-chromethemer-test/chrome")
                .err()
                .unwrap()
                .to_string()
                .contains("Select a Chrome executable")
        );
    }

    #[test]
    fn a_browser_uses_its_adjacent_archive() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("chrome");
        std::fs::write(&exe, b"executable").unwrap();
        std::fs::write(dir.path().join("resources.pak"), b"archive").unwrap();
        assert_eq!(
            browser(exe.to_str().unwrap()).unwrap().pak,
            dir.path().canonicalize().unwrap().join("resources.pak")
        );
    }

    #[test]
    fn a_selected_running_executable_is_rejected() {
        let exe = std::env::current_exe().unwrap().canonicalize().unwrap();
        assert!(
            ensure_closed(&exe)
                .unwrap_err()
                .to_string()
                .contains("Close this Chrome installation")
        );
    }
}
