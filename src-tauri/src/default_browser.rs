use std::path::Path;

use anyhow::{Context, Result};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegSetValueExW,
};

use crate::shortcut::{windows_path, windows_quote};

const CLIENT: &str = r"Software\Clients\StartMenuInternet\ChromeThemer";
const PROG_ID: &str = "ChromeThemer.HTML";
const APP_NAME: &str = "Chrome (ChromeThemer)";
pub const SETTINGS_URL: &str =
    "ms-settings:defaultapps?registeredAppUser=Chrome%20%28ChromeThemer%29";

pub fn register(launcher: &Path, executable: &Path, combined: bool) -> Result<()> {
    for (key, name, value) in entries(launcher, executable, combined)? {
        set_string(&key, &name, &value)
            .with_context(|| format!("Cannot register default browser at {key}"))?;
    }
    unsafe {
        use windows::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
    }
    Ok(())
}

fn entries(
    launcher: &Path,
    executable: &Path,
    combined: bool,
) -> Result<Vec<(String, String, String)>> {
    let launcher = windows_quote(&windows_path(launcher)?);
    let executable = windows_path(executable)?;
    let launch = format!(
        "{launcher} --launch {}{}",
        windows_quote(&executable),
        if combined { " --combined-titlebar" } else { "" }
    );
    let icon = format!("{},0", windows_quote(&executable));
    let capabilities = format!(r"{CLIENT}\Capabilities");
    let class = format!(r"Software\Classes\{PROG_ID}");
    let mut values = Vec::new();
    let mut add =
        |key: String, name: &str, value: String| values.push((key, name.to_owned(), value));
    add(CLIENT.into(), "", "Chrome (ChromeThemer)".into());
    add(format!(r"{CLIENT}\DefaultIcon"), "", icon.clone());
    add(format!(r"{CLIENT}\shell\open\command"), "", launch.clone());
    add(
        capabilities.clone(),
        "ApplicationName",
        "Chrome (ChromeThemer)".into(),
    );
    add(
        capabilities.clone(),
        "ApplicationDescription",
        "Open web pages with ChromeThemer's selected Chrome and titlebar settings.".into(),
    );
    add(capabilities.clone(), "ApplicationIcon", icon.clone());
    add(
        format!(r"{capabilities}\Startmenu"),
        "StartMenuInternet",
        "ChromeThemer".into(),
    );
    for protocol in ["http", "https"] {
        add(
            format!(r"{capabilities}\URLAssociations"),
            protocol,
            PROG_ID.into(),
        );
    }
    for extension in [".htm", ".html", ".shtml", ".xht", ".xhtml"] {
        add(
            format!(r"{capabilities}\FileAssociations"),
            extension,
            PROG_ID.into(),
        );
    }
    add(class.clone(), "", "ChromeThemer HTML document".into());
    add(class.clone(), "URL Protocol", String::new());
    add(format!(r"{class}\DefaultIcon"), "", icon);
    add(
        format!(r"{class}\Application"),
        "ApplicationName",
        "Chrome (ChromeThemer)".into(),
    );
    add(
        format!(r"{class}\shell\open\command"),
        "",
        format!("{launch} -- \"%1\""),
    );
    // Publish only after all handlers exist. Re-registering updates the saved launch settings.
    add(
        r"Software\RegisteredApplications".into(),
        APP_NAME,
        capabilities,
    );
    Ok(values)
}

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn set_string(path: &str, name: &str, value: &str) -> Result<()> {
    let mut key = std::ptr::null_mut();
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            wide(path).as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        )
    };
    anyhow::ensure!(
        status == 0,
        "{}",
        std::io::Error::from_raw_os_error(status as i32)
    );
    let key = Key(key);
    let value = wide(value);
    let status = unsafe {
        RegSetValueExW(
            key.0,
            wide(name).as_ptr(),
            0,
            REG_SZ,
            value.as_ptr().cast(),
            (value.len() * 2) as u32,
        )
    };
    anyhow::ensure!(
        status == 0,
        "{}",
        std::io::Error::from_raw_os_error(status as i32)
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handlers_preserve_paths_and_titlebar_setting() {
        for combined in [false, true] {
            let entries = entries(
                Path::new(r"C:\Themer 空\ChromeThemer.exe"),
                Path::new(r"C:\Chrome 空\chrome.exe"),
                combined,
            )
            .unwrap();
            let command = entries
                .iter()
                .find(|(key, _, _)| key == r"Software\Classes\ChromeThemer.HTML\shell\open\command")
                .unwrap();
            let expected = format!(
                "\"C:\\Themer 空\\ChromeThemer.exe\" --launch \"C:\\Chrome 空\\chrome.exe\"{} -- \"%1\"",
                if combined { " --combined-titlebar" } else { "" }
            );
            assert_eq!(command.2, expected);
            assert_eq!(
                entries.last().unwrap().0,
                r"Software\RegisteredApplications"
            );
            assert!(!entries.iter().any(|(key, _, _)| key.contains("UserChoice")));
        }
    }

    #[test]
    fn registry_round_trip_preserves_unicode_and_replaces_settings() {
        let path = format!(r"Software\ChromeThemer.Tests\{}", std::process::id());
        for combined in [false, true] {
            for (key, name, value) in entries(
                Path::new(r"C:\Themer 空\ChromeThemer.exe"),
                Path::new(r"C:\Chrome 空\chrome.exe"),
                combined,
            )
            .unwrap()
            {
                let key = format!(r"{path}\{key}");
                set_string(&key, &name, &value).unwrap();
                let mut buffer = vec![0u16; 4096];
                let mut size = (buffer.len() * 2) as u32;
                let status = unsafe {
                    windows_sys::Win32::System::Registry::RegGetValueW(
                        HKEY_CURRENT_USER,
                        wide(&key).as_ptr(),
                        wide(&name).as_ptr(),
                        windows_sys::Win32::System::Registry::RRF_RT_REG_SZ,
                        std::ptr::null_mut(),
                        buffer.as_mut_ptr().cast(),
                        &mut size,
                    )
                };
                assert_eq!(status, 0);
                assert_eq!(
                    String::from_utf16(&buffer[..size as usize / 2 - 1]).unwrap(),
                    value
                );
            }
        }
        unsafe {
            windows_sys::Win32::System::Registry::RegDeleteTreeW(
                HKEY_CURRENT_USER,
                wide(&path).as_ptr(),
            );
        }
    }
}
