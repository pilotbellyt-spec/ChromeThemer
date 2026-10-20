use std::{
    path::Path,
    sync::mpsc,
    thread::{self, JoinHandle},
    time::Duration,
};

use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum FrameStatus {
    Native,
    Active { backend: &'static str },
    Unsupported { reason: String },
}

pub struct FrameController {
    stop: mpsc::Sender<()>,
    worker: Option<JoinHandle<()>>,
}

impl FrameController {
    pub fn start(executable: &Path, marker: &str) -> (FrameStatus, Option<Self>) {
        match platform::start(executable, marker) {
            Ok(controller) => (
                FrameStatus::Active {
                    backend: platform::BACKEND,
                },
                Some(controller),
            ),
            Err(error) => (
                FrameStatus::Unsupported {
                    reason: format!("{error:#}"),
                },
                None,
            ),
        }
    }

    pub fn stop(self) {
        drop(self);
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    fn confirm(
        self,
        response: std::sync::mpsc::Receiver<std::result::Result<(), String>>,
    ) -> Result<Self> {
        use anyhow::Context;
        let confirmation = response
            .recv_timeout(Duration::from_secs(15))
            .context("Chrome's window was not found")
            .and_then(|notice| notice.map_err(anyhow::Error::msg));
        match confirmation {
            Ok(()) => Ok(self),
            Err(error) => {
                self.stop();
                Err(error)
            }
        }
    }
}

impl Drop for FrameController {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub(crate) fn browser_pids(executable: &Path, marker: &str) -> std::collections::BTreeSet<u32> {
    let system = sysinfo::System::new_with_specifics(
        sysinfo::RefreshKind::nothing().with_processes(
            sysinfo::ProcessRefreshKind::nothing()
                .with_exe(sysinfo::UpdateKind::Always)
                .with_cmd(sysinfo::UpdateKind::Always),
        ),
    );
    system
        .processes()
        .values()
        .filter(|process| process.cmd().iter().any(|arg| arg == marker))
        .filter(|process| {
            !process
                .cmd()
                .iter()
                .any(|arg| arg.to_string_lossy().starts_with("--type="))
        })
        .filter(|process| {
            process
                .exe()
                .and_then(|path| path.canonicalize().ok())
                .as_deref()
                == Some(executable)
        })
        .map(|process| process.pid().as_u32())
        .collect()
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use anyhow::{Context, ensure};
    use std::collections::BTreeSet;
    use windows_sys::Win32::{
        Foundation::{FILETIME, FreeLibrary, HMODULE, HWND, LPARAM},
        System::{
            LibraryLoader::{
                GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32,
                LoadLibraryExW,
            },
            Threading::{
                GetCurrentThread, GetCurrentThreadId, GetThreadTimes, OpenProcess,
                PROCESS_SYNCHRONIZE,
            },
        },
        UI::WindowsAndMessaging::{
            EnumWindows, GetClassNameW, GetPropW, GetWindowThreadProcessId, HHOOK, HOOKPROC,
            IsWindow, IsWindowVisible, RegisterWindowMessageW, RemovePropW, SMTO_ABORTIFHUNG,
            SMTO_BLOCK, SendMessageTimeoutW, SetPropW, SetWindowsHookExW, UnhookWindowsHookEx,
            WH_CALLWNDPROC,
        },
    };
    use windows_sys::w;

    pub const BACKEND: &str = "windows";

    struct Search {
        pid: u32,
        windows: Vec<HWND>,
    }

    unsafe extern "system" fn collect(hwnd: HWND, parameter: LPARAM) -> i32 {
        let search = unsafe { &mut *(parameter as *mut Search) };
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        if pid != search.pid || unsafe { IsWindowVisible(hwnd) } == 0 {
            return 1;
        }
        let mut class = [0_u16; 64];
        let length = unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32) };
        if length > 0 && String::from_utf16_lossy(&class[..length as usize]) == "Chrome_WidgetWin_1"
        {
            search.windows.push(hwnd);
        }
        1
    }

    fn windows_for(pid: u32) -> Vec<HWND> {
        let mut search = Search {
            pid,
            windows: Vec::new(),
        };
        unsafe {
            let _ = EnumWindows(Some(collect), (&mut search as *mut Search) as LPARAM);
        }
        search.windows
    }

    struct Helper {
        module: HMODULE,
        hook: HOOKPROC,
        pending_hooks: Vec<HHOOK>,
    }

    struct AttachRequest(HWND);

    impl Drop for AttachRequest {
        fn drop(&mut self) {
            unsafe { RemovePropW(self.0, w!("ChromeThemer.Frame.Lease.v1")) };
        }
    }

    impl Drop for Helper {
        fn drop(&mut self) {
            let mut can_unload = true;
            for hook in &self.pending_hooks {
                if unsafe { UnhookWindowsHookEx(*hook) } == 0 {
                    can_unload = false;
                    eprintln!(
                        "A Windows frame bootstrap hook could not be removed; retaining its DLL"
                    );
                }
            }
            if can_unload {
                unsafe { FreeLibrary(self.module) };
            }
        }
    }

    impl Helper {
        fn load() -> Result<Self> {
            let path = std::env::current_exe()?.with_file_name("ChromeThemerFrame.dll");
            let bytes = std::fs::read(&path).context("ChromeThemerFrame.dll is missing")?;
            ensure!(
                bytes == include_bytes!(env!("CHROMETHEMER_FRAME_DLL")),
                "ChromeThemerFrame.dll does not match this build"
            );
            use std::os::windows::ffi::OsStrExt;
            let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            let module = unsafe {
                LoadLibraryExW(
                    path.as_ptr(),
                    std::ptr::null_mut(),
                    LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
                )
            };
            ensure!(
                !module.is_null(),
                "Cannot load the Windows frame helper: {}",
                std::io::Error::last_os_error()
            );
            let address = unsafe { GetProcAddress(module, c"FrameHook".as_ptr().cast()) };
            let Some(address) = address else {
                unsafe { FreeLibrary(module) };
                anyhow::bail!("The Windows frame helper has no FrameHook export");
            };
            // FrameHook is the matching DLL's exported Windows hook ABI.
            let hook = Some(unsafe {
                std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    unsafe extern "system" fn(i32, usize, isize) -> isize,
                >(address)
            });
            Ok(Self {
                module,
                hook,
                pending_hooks: Vec::new(),
            })
        }

        fn attach(&mut self, hwnd: HWND, pid: u32, owner: u32) -> Result<()> {
            let mut current_pid = 0;
            let thread = unsafe { GetWindowThreadProcessId(hwnd, &mut current_pid) };
            ensure!(
                thread != 0 && current_pid == pid,
                "Chrome's window changed before attaching"
            );
            let (mut created, mut exited, mut kernel, mut user) = (
                FILETIME::default(),
                FILETIME::default(),
                FILETIME::default(),
                FILETIME::default(),
            );
            ensure!(
                unsafe {
                    GetThreadTimes(
                        GetCurrentThread(),
                        &mut created,
                        &mut exited,
                        &mut kernel,
                        &mut user,
                    )
                } != 0,
                "Cannot identify the frame controller thread"
            );
            let stamp = ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64;
            ensure!(
                unsafe {
                    SetPropW(
                        hwnd,
                        w!("ChromeThemer.Frame.Lease.v1"),
                        stamp as usize as *mut std::ffi::c_void,
                    )
                } != 0,
                "Cannot prepare Chrome's frame request"
            );
            let _request = AttachRequest(hwnd);
            let hook = unsafe { SetWindowsHookExW(WH_CALLWNDPROC, self.hook, self.module, thread) };
            ensure!(
                !hook.is_null(),
                "Cannot attach the Windows frame helper: {}",
                std::io::Error::last_os_error()
            );
            self.pending_hooks.push(hook);
            let message = unsafe { RegisterWindowMessageW(w!("ChromeThemer.Frame.Attach.v1")) };
            let sent = unsafe {
                SendMessageTimeoutW(
                    hwnd,
                    message,
                    std::process::id() as usize,
                    owner as isize,
                    SMTO_ABORTIFHUNG | SMTO_BLOCK,
                    2000,
                    std::ptr::null_mut(),
                )
            };
            ensure!(
                unsafe { UnhookWindowsHookEx(hook) } != 0,
                "Cannot remove the Windows frame bootstrap hook: {}",
                std::io::Error::last_os_error()
            );
            self.pending_hooks.pop();
            ensure!(
                sent != 0 && active_owner(hwnd) == owner,
                "Chrome did not accept the Windows frame helper"
            );
            Ok(())
        }
    }

    fn active_owner(hwnd: HWND) -> u32 {
        unsafe { GetPropW(hwnd, w!("ChromeThemer.Frame.Owner.v1")) as usize as u32 }
    }

    fn restore(hwnd: HWND, owner: u32, timeout: u32) {
        let message = unsafe { RegisterWindowMessageW(w!("ChromeThemer.Frame.Detach.v1")) };
        unsafe {
            SendMessageTimeoutW(
                hwnd,
                message,
                owner as usize,
                0,
                SMTO_ABORTIFHUNG | SMTO_BLOCK,
                timeout,
                std::ptr::null_mut(),
            )
        };
        if active_owner(hwnd) == owner {
            eprintln!("Chrome's frame will restore when its window responds");
        }
    }

    pub fn start(executable: &Path, marker: &str) -> Result<FrameController> {
        let executable = executable.to_owned();
        let marker = marker.to_owned();
        let (stop, shutdown) = mpsc::channel();
        let (ready, response) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            let mut attached = BTreeSet::<(u32, isize)>::new();
            let owner = unsafe { GetCurrentThreadId() };
            let mut confirmed = false;
            let watch = (|| -> Result<()> {
                let mut helper = Helper::load()?;
                loop {
                    for pid in browser_pids(&executable, &marker) {
                        for hwnd in windows_for(pid) {
                            let key = hwnd as isize;
                            if active_owner(hwnd) == owner {
                                continue;
                            }
                            let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
                            ensure!(
                                !process.is_null(),
                                "Chrome closed before its frame could be attached"
                            );
                            use std::os::windows::io::{FromRawHandle, OwnedHandle};
                            let _process = unsafe { OwnedHandle::from_raw_handle(process) };
                            ensure!(
                                browser_pids(&executable, &marker).contains(&pid),
                                "Chrome's process changed before attaching"
                            );
                            helper.attach(hwnd, pid, owner)?;
                            attached.insert((pid, key));
                            if !confirmed {
                                let _ = ready.send(Ok(()));
                                confirmed = true;
                            }
                        }
                    }
                    if !matches!(
                        shutdown.recv_timeout(Duration::from_millis(250)),
                        Err(mpsc::RecvTimeoutError::Timeout)
                    ) {
                        break;
                    }
                }
                Ok(())
            })();
            if let Err(error) = watch {
                if confirmed {
                    eprintln!("Windows frame helper stopped: {error:#}");
                }
                let _ = ready.send(Err(format!("{error:#}")));
            }
            let pids = browser_pids(&executable, &marker);
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            for (pid, key) in attached {
                let hwnd = key as HWND;
                let mut window_pid = 0;
                if !pids.contains(&pid) || unsafe { IsWindow(hwnd) } == 0 {
                    continue;
                }
                unsafe { GetWindowThreadProcessId(hwnd, &mut window_pid) };
                if window_pid != pid {
                    continue;
                }
                if active_owner(hwnd) == owner
                    && let Some(remaining) =
                        deadline.checked_duration_since(std::time::Instant::now())
                {
                    restore(hwnd, owner, remaining.as_millis().clamp(1, 2000) as u32);
                }
            }
        });
        FrameController {
            stop,
            worker: Some(worker),
        }
        .confirm(response)
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::collections::BTreeMap;
    use std::process::Command;

    use anyhow::Context;

    pub const BACKEND: &str = "x11";

    fn output(args: &[&str]) -> Result<String> {
        let output = Command::new("xprop")
            .args(args)
            .output()
            .context("Experimental titlebar mode requires xprop")?;
        anyhow::ensure!(output.status.success(), "xprop failed");
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn windows_for(pids: &std::collections::BTreeSet<u32>) -> Result<Vec<String>> {
        let clients = output(&["-root", "_NET_CLIENT_LIST"])?;
        let mut windows = Vec::new();
        for token in clients.split_whitespace() {
            let id = token.trim_end_matches(',');
            if !id.starts_with("0x") {
                continue;
            }
            let owner = output(&["-id", id, "_NET_WM_PID"])?;
            if owner
                .split_whitespace()
                .last()
                .and_then(|value| value.parse::<u32>().ok())
                .is_some_and(|pid| pids.contains(&pid))
            {
                windows.push(id.to_owned());
            }
        }
        Ok(windows)
    }

    pub fn start(executable: &Path, marker: &str) -> Result<FrameController> {
        if std::env::var("XDG_SESSION_TYPE")
            .is_ok_and(|value| value.eq_ignore_ascii_case("wayland"))
        {
            anyhow::bail!("Native Wayland windows cannot be reframed by ChromeThemer");
        }
        output(&["-root", "_NET_SUPPORTING_WM_CHECK"])?;
        let executable = executable.to_owned();
        let marker = marker.to_owned();
        let (stop, shutdown) = mpsc::channel();
        let (ready, response) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            let mut originals = BTreeMap::<String, Option<String>>::new();
            let mut confirmed = false;
            loop {
                if let Ok(windows) = windows_for(&browser_pids(&executable, &marker)) {
                    for id in windows {
                        if originals.contains_key(&id) {
                            continue;
                        }
                        let original =
                            output(&["-id", &id, "_MOTIF_WM_HINTS"])
                                .ok()
                                .and_then(|value| {
                                    value.split_once('=').map(|(_, rhs)| rhs.trim().to_owned())
                                });
                        let applied = Command::new("xprop")
                            .args([
                                "-id",
                                &id,
                                "-f",
                                "_MOTIF_WM_HINTS",
                                "32c",
                                "-set",
                                "_MOTIF_WM_HINTS",
                                "2, 0, 0, 0, 0",
                            ])
                            .status()
                            .is_ok_and(|status| status.success());
                        if applied {
                            originals.insert(id, original);
                            if !confirmed {
                                let _ = ready.send(Ok(()));
                                confirmed = true;
                            }
                        }
                    }
                }
                if !matches!(
                    shutdown.recv_timeout(Duration::from_millis(250)),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    break;
                }
            }
            let owned = windows_for(&browser_pids(&executable, &marker)).unwrap_or_default();
            for (id, original) in originals {
                if !owned.contains(&id) {
                    continue;
                }
                let mut command = Command::new("xprop");
                command.args(["-id", &id]);
                match original {
                    Some(value) => {
                        command.args([
                            "-f",
                            "_MOTIF_WM_HINTS",
                            "32c",
                            "-set",
                            "_MOTIF_WM_HINTS",
                            &value,
                        ]);
                    }
                    None => {
                        command.args(["-remove", "_MOTIF_WM_HINTS"]);
                    }
                }
                let _ = command.status();
            }
        });
        FrameController {
            stop,
            worker: Some(worker),
        }
        .confirm(response)
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
mod platform {
    use super::*;

    pub const BACKEND: &str = "unsupported";

    pub fn start(_executable: &Path, _marker: &str) -> Result<FrameController> {
        anyhow::bail!("Experimental titlebar mode is available only on Windows and X11")
    }
}
