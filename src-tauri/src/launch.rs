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
    Launch { executable: PathBuf, combined: bool },
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
        let combined = match args.next() {
            None => false,
            Some(flag) if flag == "--combined-titlebar" => true,
            Some(_) => bail!("Unknown launch option"),
        };
        anyhow::ensure!(args.next().is_none(), "Too many launch arguments");
        Ok(Self::Launch {
            executable: executable.into(),
            combined,
        })
    }
}

pub fn start(executable: &Path, combined: bool) -> Result<(RunningBrowser, FrameStatus)> {
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
