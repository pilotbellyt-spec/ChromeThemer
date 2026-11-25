use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};

use crate::native_frame::{FrameController, FrameStatus, browser_pids};

pub struct RunningBrowser {
    child: Child,
    frame: Option<FrameController>,
    executable: PathBuf,
    marker: String,
}

pub enum Startup {
    Gui,
    Launch {
        executable: PathBuf,
        combined: bool,
        targets: Vec<OsString>,
    },
}

impl Startup {
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self> {
        let mut args = args.into_iter();
        let Some(mode) = args.next() else {
            return Ok(Self::Gui);
        };
        if mode != "--launch" {
            bail!("Expected --launch <Chrome executable> [--combined-titlebar]");
        }
        let executable = args
            .next()
            .context("Choose a Chrome executable after --launch")?;
        let mut combined = false;
        let mut targets = Vec::new();
        while let Some(flag) = args.next() {
            if flag == "--" {
                targets.extend(args);
                break;
            }
            anyhow::ensure!(
                flag == "--combined-titlebar" && !combined,
                "Unknown launch option"
            );
            combined = true;
        }
        Ok(Self::Launch {
            executable: executable.into(),
            combined,
            targets,
        })
    }
}

pub fn start(executable: &Path, combined: bool) -> Result<(RunningBrowser, FrameStatus)> {
    start_with_targets(executable, combined, &[])
}

pub fn open(executable: &Path, combined: bool, targets: &[OsString]) -> Result<()> {
    let browser = crate::browser(
        executable
            .to_str()
            .context("Chrome path is not valid UTF-8")?,
    )?;
    let executable = browser.executable.as_path();
    if !targets.is_empty() && crate::browser_running(executable)? {
        let status = Command::new(executable)
            .arg("--")
            .args(targets)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("Cannot open link in Chrome")?
            .wait()?;
        anyhow::ensure!(status.success(), "Chrome could not open the link: {status}");
        return Ok(());
    }
    let (running, frame) = start_with_targets(executable, combined, targets)?;
    if let FrameStatus::Unsupported { reason } = frame {
        show_error(&anyhow::anyhow!("Combined titlebar failed: {reason}"));
    }
    running.wait()
}

fn start_with_targets(
    executable: &Path,
    combined: bool,
    targets: &[OsString],
) -> Result<(RunningBrowser, FrameStatus)> {
    let executable = executable
        .to_str()
        .context("Chrome path is not valid UTF-8")?;
    let browser = crate::browser(executable)?;
    crate::ensure_closed(&browser.executable)?;
    let marker = format!(
        "--chromethemer-session={}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    );
    let child = Command::new(&browser.executable)
        .args([
            "--enable-features=Webium,SurfaceEmbed,ExtensionsMenuAccessControl",
            "--disable-features=WebUIBundledCodeCache",
        ])
        .arg(&marker)
        .arg("--")
        .args(targets)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Cannot launch Chrome")?;
    let (status, frame) = if combined && !cfg!(target_os = "macos") {
        FrameController::start(&browser.executable, &marker)
    } else {
        (FrameStatus::Native, None)
    };
    Ok((
        RunningBrowser {
            child,
            frame,
            executable: browser.executable,
            marker,
        },
        status,
    ))
}

impl RunningBrowser {
    pub fn wait(mut self) -> Result<()> {
        const RELAUNCH_GRACE: Duration = Duration::from_secs(3);
        let mut last_alive = Instant::now();
        loop {
            let child_alive = self.child.try_wait()?.is_none();
            if child_alive || !browser_pids(&self.executable, &self.marker).is_empty() {
                last_alive = Instant::now();
            } else if last_alive.elapsed() >= RELAUNCH_GRACE {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }
}

impl Drop for RunningBrowser {
    fn drop(&mut self) {
        if let Some(frame) = self.frame.take() {
            frame.stop();
        }
        if let Err(error) = self.child.try_wait() {
            eprintln!("Cannot reap Chrome launcher: {error}");
        }
    }
}

pub fn show_error(error: &anyhow::Error) {
    rfd::MessageDialog::new()
        .set_title("ChromeThemer")
        .set_description(format!("{error:#}"))
        .set_level(rfd::MessageLevel::Error)
        .show();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_targets_start_after_the_option_separator() {
        let Startup::Launch {
            executable,
            combined,
            targets,
        } = Startup::parse(
            [
                "--launch",
                r"C:\Chrome 空\chrome.exe",
                "--combined-titlebar",
                "--",
                "https://example.com/?a=1&b=2",
                "--looks-like-a-flag",
            ]
            .map(OsString::from),
        )
        .unwrap()
        else {
            panic!("Expected launch")
        };
        assert_eq!(executable, PathBuf::from(r"C:\Chrome 空\chrome.exe"));
        assert!(combined);
        assert_eq!(
            targets,
            ["https://example.com/?a=1&b=2", "--looks-like-a-flag"].map(OsString::from)
        );
        for args in [
            vec!["--launch"],
            vec!["--launch", "chrome", "--unknown"],
            vec!["--launch", "chrome", "https://example.com"],
        ] {
            assert!(Startup::parse(args.into_iter().map(OsString::from)).is_err());
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn links_launch_and_forward_to_a_running_browser() {
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("test-browser.exe");
        let source = dir.path().join("browser.c");
        std::fs::write(&source, r#"
#include <windows.h>
#include <stdio.h>
#include <wchar.h>
int wmain(int argc, wchar_t **argv) {
    int initial = 0;
    for (int i = 1; i < argc; i++) if (wcsstr(argv[i], L"--chromethemer-session=") == argv[i]) initial = 1;
    wchar_t path[MAX_PATH];
    GetModuleFileNameW(NULL, path, MAX_PATH);
    wcscpy(wcsrchr(path, L'\\') + 1, initial ? L"initial.txt" : L"forwarded.txt");
    FILE *out = _wfopen(path, L"wb");
    for (int i = 1; i < argc; i++) {
        fwrite(argv[i], sizeof(wchar_t), wcslen(argv[i]), out);
        wchar_t newline = L'\n';
        fwrite(&newline, sizeof(wchar_t), 1, out);
    }
    fclose(out);
    if (initial) Sleep(15000);
    return 0;
}
"#).unwrap();
        let compiler = std::env::var_os("CHROMETHEMER_FRAME_CC")
            .unwrap_or_else(|| r"C:\msys64\ucrt64\bin\gcc.exe".into());
        assert!(
            Command::new(compiler)
                .arg(&source)
                .args(["-municode", "-static-libgcc", "-o"])
                .arg(&executable)
                .status()
                .unwrap()
                .success()
        );
        std::fs::write(dir.path().join("resources.pak"), b"test").unwrap();
        let executable = executable.canonicalize().unwrap();
        let target = OsString::from("https://example.com/空?q=one&two=2");
        let (mut running, _) = start_with_targets(&executable, false, &[target.clone()]).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !dir.path().join("initial.txt").exists() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(crate::browser_running(&executable).unwrap());
        open(&executable, true, &[target]).unwrap();
        running.child.kill().unwrap();
        running.child.wait().unwrap();
        let read = |name| {
            let bytes = std::fs::read(dir.path().join(name)).unwrap();
            String::from_utf16(
                &bytes
                    .chunks_exact(2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .collect::<Vec<_>>(),
            )
            .unwrap()
        };
        let initial = read("initial.txt");
        assert!(initial.contains("--enable-features=Webium"));
        assert!(initial.contains("--chromethemer-session="));
        assert!(initial.contains("https://example.com/空?q=one&two=2"));
        let forwarded = read("forwarded.txt");
        assert!(forwarded.starts_with("--\n"));
        assert!(forwarded.contains("https://example.com/空?q=one&two=2"));
        assert!(!forwarded.contains("--chromethemer-session="));
    }
}
