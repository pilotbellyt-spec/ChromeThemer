#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(all(not(debug_assertions), not(feature = "custom-protocol")))]
compile_error!("Release builds require --features custom-protocol to embed the GUI.");

use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use sysinfo::System;
use tauri::{Manager, State};
use tauri_plugin_opener::OpenerExt;

mod launch;
mod native_frame;
mod profiles;
mod shortcut;

const ADMIN_REQUIRED: &str = "ChromeThemer must be run as administrator. Close it, then right-click ChromeThemer and choose \"Run as administrator\".";

struct Browser {
    executable: PathBuf,
    pak: PathBuf,
}

#[derive(Default)]
struct Session(Mutex<Option<launch::RunningBrowser>>);

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
enum PaletteOutcome {
    NotRequested,
    Applied { profiles: usize },
    Restored { profiles: usize },
    Failed { message: String },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ThemeOperation {
    pak: PakOutcome,
    palette: PaletteOutcome,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
enum PakOutcome {
    Applied {
        inspection: chromethemer::Inspection,
    },
    Restored {
        inspection: chromethemer::Inspection,
    },
    Failed {
        message: String,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LaunchResult {
    frame: native_frame::FrameStatus,
}

#[cfg(target_os = "windows")]
fn ensure_elevated() -> Result<()> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    let mut token = std::ptr::null_mut();
    // SAFETY: Windows initializes `token` on success. The handle remains valid until CloseHandle.
    ensure!(
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } != 0,
        "Cannot check whether ChromeThemer is running as administrator"
    );
    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut returned = 0;
    // SAFETY: The buffer points to a TOKEN_ELEVATION value with the size passed to Windows.
    let result = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    };
    // SAFETY: OpenProcessToken returned a valid owned handle above.
    unsafe { CloseHandle(token) };
    ensure!(
        result != 0,
        "Cannot check whether ChromeThemer is running as administrator"
    );
    ensure!(elevation.TokenIsElevated != 0, ADMIN_REQUIRED);
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn ensure_elevated() -> Result<()> {
    Ok(())
}

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
        let path = process.exe().context(
            "Chrome is still running, but ChromeThemer cannot determine which installation it belongs to. Close all Chrome windows and background processes, then try again",
        )?;
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
fn check_environment() -> std::result::Result<&'static str, String> {
    ensure_elevated().map_err(|e| format!("{e:#}"))?;
    Ok(std::env::consts::OS)
}

#[tauri::command]
async fn inspect_browser(
    executable: String,
) -> std::result::Result<chromethemer::Inspection, String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<chromethemer::Inspection> {
        ensure_elevated()?;
        chromethemer::inspect(&browser(&executable)?.pak)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn apply_theme(
    executable: String,
    theme: String,
    allow_unsafe_theme_files: bool,
) -> std::result::Result<ThemeOperation, String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<ThemeOperation> {
        ensure_elevated()?;
        let browser = browser(&executable)?;
        ensure_closed(&browser.executable)?;
        let theme = Path::new(&theme);
        let palette = chromethemer::palette::from_theme(theme)?;
        let profiles = match palette {
            Some(_) => {
                let profiles = profiles::discover(&browser.executable)?;
                ensure!(!profiles.is_empty(), "No Chrome profiles found for this theme's colors.json. Open Chrome once to create a profile");
                profiles
            }
            None => Vec::new(),
        };
        let files = if allow_unsafe_theme_files {
            chromethemer::ThemeFiles::Unsafe
        } else {
            chromethemer::ThemeFiles::CssOnly
        };
        let inspection = chromethemer::apply(&browser.pak, theme, files)?;
        let palette = match palette {
            Some(palette) => {
                let mut failures = Vec::new();
                for profile in &profiles {
                    if let Err(error) = chromethemer::palette::apply(profile, palette) {
                        failures.push(format!("{}: {error:#}", profile.display()));
                    }
                }
                if failures.is_empty() {
                    PaletteOutcome::Applied { profiles: profiles.len() }
                } else {
                    PaletteOutcome::Failed { message: failures.join("; ") }
                }
            }
            None => PaletteOutcome::NotRequested,
        };
        Ok(ThemeOperation {
            pak: PakOutcome::Applied { inspection },
            palette,
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn restore_browser(executable: String) -> std::result::Result<ThemeOperation, String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<ThemeOperation> {
        ensure_elevated()?;
        let browser = browser(&executable)?;
        ensure_closed(&browser.executable)?;
        let palette = match profiles::discover(&browser.executable) {
            Ok(profiles) => {
                let mut failures = Vec::new();
                let mut restored = 0;
                for profile in profiles {
                    if !profile.join(".chromethemer/palette.json").is_file() {
                        continue;
                    }
                    match chromethemer::palette::restore(&profile) {
                        Ok(_) => restored += 1,
                        Err(error) => failures.push(format!("{}: {error:#}", profile.display())),
                    }
                }
                if failures.is_empty() {
                    PaletteOutcome::Restored { profiles: restored }
                } else {
                    PaletteOutcome::Failed {
                        message: failures.join("; "),
                    }
                }
            }
            Err(error) => PaletteOutcome::Failed {
                message: format!("{error:#}"),
            },
        };
        let pak = match chromethemer::restore(&browser.pak) {
            Ok(inspection) => PakOutcome::Restored { inspection },
            Err(error) => PakOutcome::Failed {
                message: format!("{error:#}"),
            },
        };
        Ok(ThemeOperation { pak, palette })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn launch_browser(
    executable: String,
    experimental_frameless: bool,
    session: State<'_, Session>,
) -> std::result::Result<LaunchResult, String> {
    let launch = || -> Result<LaunchResult> {
        let mut slot = session
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("Browser session lock failed"))?;
        let (running, frame) = launch::start(Path::new(&executable), experimental_frameless)?;
        *slot = Some(running);
        Ok(LaunchResult { frame })
    };
    launch().map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn create_shortcut(
    executable: String,
    experimental_frameless: bool,
    app: tauri::AppHandle,
) -> std::result::Result<bool, String> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || -> Result<bool> {
        ensure!(
            !cfg!(target_os = "macos"),
            "Shortcuts are not needed on macOS"
        );
        let browser = browser(&executable)?;
        let extension = if cfg!(target_os = "windows") {
            "lnk"
        } else {
            "desktop"
        };
        let chosen = app
            .dialog()
            .file()
            .set_title("Save Chrome shortcut")
            .set_file_name(format!("Chrome (themed).{extension}"))
            .set_directory(app.path().desktop_dir()?)
            .add_filter("Chrome shortcut", &[extension])
            .blocking_save_file();
        let Some(path) = chosen else { return Ok(false) };
        shortcut::create(
            &path.into_path()?,
            &browser.executable,
            experimental_frameless,
        )?;
        Ok(true)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("{e:#}"))
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
    if !cfg!(target_os = "macos") {
        let startup = launch::Startup::parse(std::env::args_os().skip(1));
        match startup {
            Ok(launch::Startup::Gui) => {}
            Ok(launch::Startup::Launch {
                executable,
                combined,
            }) => {
                let result = launch::start(&executable, combined).and_then(|(running, frame)| {
                    if let native_frame::FrameStatus::Unsupported { reason } = frame {
                        launch::show_error(&anyhow::anyhow!("Combined titlebar failed: {reason}"));
                    }
                    running.wait()
                });
                if let Err(error) = result {
                    launch::show_error(&error);
                    std::process::exit(1);
                }
                return;
            }
            Err(error) => {
                launch::show_error(&error);
                std::process::exit(1);
            }
        }
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(Session::default())
        .invoke_handler(tauri::generate_handler![
            check_environment,
            inspect_browser,
            apply_theme,
            restore_browser,
            launch_browser,
            create_shortcut,
            open_theme
        ])
        .build(tauri::generate_context!())
        .expect("Could not start ChromeThemer")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event
                && let Ok(mut session) = app.state::<Session>().0.lock()
            {
                session.take();
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
