#[cfg(target_os = "linux")]
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, ensure};

pub fn create(destination: &Path, executable: &Path, combined: bool) -> Result<()> {
    let launcher = std::env::current_exe()?.canonicalize()?;
    ensure!(
        !destination.exists(),
        "A file already exists there. Choose another shortcut name"
    );
    #[cfg(target_os = "windows")]
    {
        windows_link(destination, &launcher, executable, combined)
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        let icon = linux_icon(executable)?;
        let contents = desktop_entry(&launcher, executable, &icon, combined)?;
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)?;
        file.write_all(contents.as_bytes())?;
        file.set_permissions(fs::Permissions::from_mode(0o755))?;
        Ok(())
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = (launcher, executable, combined);
        anyhow::bail!("Chrome shortcuts are available on Windows and Linux")
    }
}

#[cfg(target_os = "windows")]
fn windows_path(path: &Path) -> Result<String> {
    let text = path.to_str().context("Shortcut path is not valid UTF-8")?;
    Ok(if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        text.strip_prefix(r"\\?\").unwrap_or(text).to_owned()
    })
}

#[cfg(target_os = "windows")]
fn windows_link(
    destination: &Path,
    launcher: &Path,
    executable: &Path,
    combined: bool,
) -> Result<()> {
    use windows::{
        Win32::{System::Com::IPersistFile, UI::Shell::IShellLinkW},
        core::{HSTRING, Interface},
    };
    ensure!(
        destination
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk")),
        "Save the shortcut with a .lnk extension"
    );
    let executable = windows_path(executable)?;
    let args = format!(
        "--launch {}{}",
        windows_quote(&executable),
        if combined { " --combined-titlebar" } else { "" }
    );
    let _apartment = Apartment::enter()?;
    let link: IShellLinkW = unsafe {
        windows::Win32::System::Com::CoCreateInstance(
            &windows::Win32::UI::Shell::ShellLink,
            None,
            windows::Win32::System::Com::CLSCTX_INPROC_SERVER,
        )?
    };
    // Wide shell APIs preserve paths that WScript.Shell converts to the ANSI codepage.
    unsafe {
        link.SetPath(&HSTRING::from(windows_path(launcher)?))?;
        link.SetArguments(&HSTRING::from(args))?;
        link.SetIconLocation(&HSTRING::from(executable), 0)?;
        link.SetWorkingDirectory(&HSTRING::from(windows_path(
            launcher.parent().context("Launcher has no folder")?,
        )?))?;
        let file: IPersistFile = link.cast()?;
        file.Save(&HSTRING::from(windows_path(destination)?), true)?;
    }
    ensure!(destination.is_file(), "Windows did not save the shortcut");
    Ok(())
}

#[cfg(target_os = "windows")]
struct Apartment;

#[cfg(target_os = "windows")]
impl Apartment {
    fn enter() -> Result<Self> {
        use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok()?;
        Ok(Self)
    }
}

#[cfg(target_os = "windows")]
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { windows::Win32::System::Com::CoUninitialize() };
    }
}
#[cfg(target_os = "windows")]
fn windows_quote(arg: &str) -> String {
    let mut quoted = String::from("\"");
    let mut slashes = 0;
    for character in arg.chars() {
        if character == '\\' {
            slashes += 1;
            continue;
        }
        quoted.extend(std::iter::repeat_n(
            '\\',
            if character == '"' {
                slashes * 2 + 1
            } else {
                slashes
            },
        ));
        quoted.push(character);
        slashes = 0;
    }
    quoted.extend(std::iter::repeat_n('\\', slashes * 2));
    quoted.push('"');
    quoted
}

#[cfg(target_os = "linux")]
fn desktop_escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

#[cfg(target_os = "linux")]
fn desktop_arg(text: &str) -> String {
    let mut quoted = String::from("\"");
    for character in text.chars() {
        if matches!(character, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(character);
        if character == '%' {
            quoted.push('%');
        }
    }
    quoted.push('"');
    desktop_escape(&quoted)
}

#[cfg(target_os = "linux")]
fn desktop_entry(launcher: &Path, executable: &Path, icon: &str, combined: bool) -> Result<String> {
    let launcher = launcher
        .to_str()
        .context("Launcher path is not valid UTF-8")?;
    let executable = executable
        .to_str()
        .context("Chrome path is not valid UTF-8")?;
    Ok(format!(
        "[Desktop Entry]\nType=Application\nName=Chrome (themed)\nExec={} --launch {}{}\nIcon={}\nTerminal=false\nCategories=Network;WebBrowser;\n",
        desktop_arg(launcher),
        desktop_arg(executable),
        if combined { " --combined-titlebar" } else { "" },
        desktop_escape(icon)
    ))
}

#[cfg(target_os = "linux")]
fn linux_icon(executable: &Path) -> Result<String> {
    use std::path::PathBuf;
    let mut roots = Vec::new();
    if let Some(root) = std::env::var_os("XDG_DATA_HOME") {
        roots.push(PathBuf::from(root));
    } else if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join(".local/share"));
    }
    roots.extend(std::env::split_paths(
        &std::env::var_os("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into()),
    ));
    for root in roots {
        let entries = match fs::read_dir(root.join("applications")) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error).context("Cannot read desktop application icons"),
        };
        for entry in entries {
            let path = entry?.path();
            if path.extension().is_none_or(|ext| ext != "desktop") {
                continue;
            }
            let text = fs::read_to_string(&path)?;
            if let Some(icon) = matching_desktop_icon(&text, executable) {
                return Ok(icon);
            }
        }
    }
    bundled_icon(executable)
}

#[cfg(target_os = "linux")]
fn matching_desktop_icon(text: &str, executable: &Path) -> Option<String> {
    let mut in_entry = false;
    let mut command = None;
    let mut icon = None;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        }
        if !in_entry {
            continue;
        }
        if let Some(exec) = line.strip_prefix("Exec=") {
            command = Some(exec);
        }
        if let Some(name) = line.strip_prefix("Icon=") {
            icon = Some(name);
        }
    }
    let args = shlex::split(command?)?;
    let binary = args.first()?;
    let path = if Path::new(binary).is_absolute() {
        std::path::PathBuf::from(binary)
    } else {
        std::env::split_paths(&std::env::var_os("PATH")?)
            .map(|dir| dir.join(binary))
            .find(|path| path.is_file())?
    };
    let browser = crate::browser(path.to_str()?).ok()?;
    if browser.executable != executable {
        return None;
    }
    let icon = icon?.trim();
    if icon.is_empty() {
        return None;
    }
    Some(icon.to_owned())
}

#[cfg(target_os = "linux")]
fn bundled_icon(executable: &Path) -> Result<String> {
    let parent = executable
        .parent()
        .context("Chrome executable has no parent folder")?;
    for name in [
        "product_logo_256.png",
        "product_logo_128.png",
        "product_logo_64.png",
        "product_logo_48.png",
        "chromium.png",
        "chrome.png",
    ] {
        let icon = parent.join(name);
        if icon.is_file() {
            return Ok(icon
                .to_str()
                .context("Chrome icon path is not valid UTF-8")?
                .to_owned());
        }
    }
    anyhow::bail!(
        "Cannot find this Chrome installation's icon. Keep its product_logo PNG beside the executable"
    )
}
