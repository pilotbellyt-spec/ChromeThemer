pub mod pak;
pub mod palette;
mod webium;

use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use pak::Pak;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inspection {
    pub path: PathBuf,
    pub state: PatchState,
    pub resources: usize,
    pub module_id: u16,
    pub sha256: String,
    pub backup: PathBuf,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum PatchState {
    Original,
    Applied,
}

#[derive(Clone, Copy)]
pub enum ThemeFiles {
    CssOnly,
    Unsafe,
}

const MAX_THEME_FILE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    original: String,
    applied: Option<String>,
    pending: Option<String>,
}

struct Archive {
    path: PathBuf,
    backup: PathBuf,
    receipt: PathBuf,
    _lock: File,
}

impl Archive {
    fn open(path: &Path) -> Result<Self> {
        let path = path.canonicalize().context("Cannot open resources.pak")?;
        ensure!(path.is_file(), "Select a resources.pak file");
        let state = path.with_file_name(format!(
            "{}.chromethemer",
            path.file_name()
                .context("Missing PAK filename")?
                .to_string_lossy()
        ));
        fs::create_dir_all(&state).context(
            "Cannot create backup folder beside resources.pak. Use a writable Chrome copy",
        )?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(state.join("lock"))?;
        lock.try_lock_exclusive()
            .context("Another ChromeThemer operation is using this archive")?;
        Ok(Self {
            path,
            backup: state.join("original.pak"),
            receipt: state.join("receipt.json"),
            _lock: lock,
        })
    }

    fn read_receipt(&self) -> Result<Option<Receipt>> {
        match fs::read(&self.receipt) {
            Ok(bytes) => Ok(Some(
                serde_json::from_slice(&bytes).context("Invalid backup receipt")?,
            )),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn original(&self, current: &[u8], receipt: &Receipt) -> Result<Vec<u8>> {
        let hash = sha256(current);
        ensure!(
            hash == receipt.original
                || receipt.applied.as_ref() == Some(&hash)
                || receipt.pending.as_ref() == Some(&hash),
            "Chrome's resources changed outside ChromeThemer, possibly after an update. The archive was left untouched; use a fresh Chrome copy"
        );
        let original = fs::read(&self.backup).context("Cannot read original backup")?;
        ensure!(
            sha256(&original) == receipt.original,
            "Original backup checksum mismatch; archive left untouched"
        );
        Ok(original)
    }

    fn save_receipt(&self, receipt: &Receipt) -> Result<()> {
        atomic_write(&self.receipt, &serde_json::to_vec_pretty(receipt)?, None)
    }
}

pub fn inspect(path: &Path) -> Result<Inspection> {
    let archive = Archive::open(path)?;
    inspect_archive(&archive)
}

fn inspect_archive(archive: &Archive) -> Result<Inspection> {
    let current = fs::read(&archive.path)?;
    let hash = sha256(&current);
    let (original, state) = match archive.read_receipt()? {
        Some(receipt) => {
            let original = archive.original(&current, &receipt)?;
            let state = if hash == receipt.original {
                PatchState::Original
            } else {
                PatchState::Applied
            };
            (original, state)
        }
        None => (current, PatchState::Original),
    };
    let pak = Pak::parse(&original)?;
    let module_id = webium::locate(&pak)?;
    Ok(Inspection {
        path: archive.path.clone(),
        state,
        resources: pak.count(),
        module_id,
        sha256: hash,
        backup: archive.backup.clone(),
    })
}

pub fn apply(path: &Path, theme: &Path, files: ThemeFiles) -> Result<Inspection> {
    let css = fs::read_to_string(theme.join("userChrome.css"))
        .context("Theme folder must contain a UTF-8 userChrome.css")?;
    ensure!(
        css.len() <= MAX_THEME_FILE_BYTES,
        "userChrome.css must be at most 4 MiB"
    );
    let script = match files {
        ThemeFiles::CssOnly => None,
        ThemeFiles::Unsafe => match fs::read_to_string(theme.join("userChrome.js")) {
            Ok(script) => {
                ensure!(
                    script.len() <= MAX_THEME_FILE_BYTES,
                    "userChrome.js must be at most 4 MiB"
                );
                Some(script)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e).context("Cannot read UTF-8 userChrome.js"),
        },
    };
    let archive = Archive::open(path)?;
    let current = fs::read(&archive.path)?;
    let current_hash = sha256(&current);
    let (original, mut receipt) = match archive.read_receipt()? {
        Some(receipt) => (archive.original(&current, &receipt)?, receipt),
        None => (
            current.clone(),
            Receipt {
                original: current_hash.clone(),
                applied: None,
                pending: None,
            },
        ),
    };
    let mut pak = Pak::parse(&original)?;
    webium::patch(&mut pak, &css, script.as_deref())?;
    let patched = pak.encode()?;
    Pak::parse(&patched).context("Generated archive failed validation")?;
    match fs::read(&archive.backup) {
        Ok(backup) => {
            ensure!(
                sha256(&backup) == receipt.original,
                "Existing original backup belongs to another archive"
            );
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut staging = tempfile::NamedTempFile::new_in(
                archive.backup.parent().context("Missing backup folder")?,
            )?;
            staging.write_all(&original)?;
            staging.as_file().sync_all()?;
            staging
                .persist_noclobber(&archive.backup)
                .map_err(|e| e.error)?;
            #[cfg(unix)]
            File::open(archive.backup.parent().context("Missing backup folder")?)?.sync_all()?;
        }
        Err(e) => return Err(e).context("Cannot save original backup"),
    }
    ensure!(
        sha256(&fs::read(&archive.backup)?) == receipt.original,
        "Backup verification failed"
    );
    if receipt.pending.as_ref() == Some(&current_hash) {
        receipt.applied = receipt.pending.take();
    }
    receipt.pending = Some(sha256(&patched));
    archive.save_receipt(&receipt)?;
    ensure!(
        sha256(&fs::read(&archive.path)?) == current_hash,
        "Archive changed during Apply; no replacement was made"
    );
    let permissions = fs::metadata(&archive.path)?.permissions();
    atomic_write(&archive.path, &patched, Some(permissions))
        .context("Cannot replace resources.pak. Close Chrome and check write permissions")?;
    receipt.applied = receipt.pending.take();
    archive.save_receipt(&receipt)?;
    inspect_archive(&archive)
}

pub fn restore(path: &Path) -> Result<Inspection> {
    let archive = Archive::open(path)?;
    let Some(mut receipt) = archive.read_receipt()? else {
        return inspect_archive(&archive);
    };
    let current = fs::read(&archive.path)?;
    let original = archive.original(&current, &receipt)?;
    ensure!(
        fs::read(&archive.path)? == current,
        "Archive changed during Restore; no replacement was made"
    );
    if current != original {
        atomic_write(
            &archive.path,
            &original,
            Some(fs::metadata(&archive.path)?.permissions()),
        )?;
    }
    receipt.applied = None;
    receipt.pending = None;
    archive.save_receipt(&receipt)?;
    inspect_archive(&archive)
}

fn atomic_write(path: &Path, bytes: &[u8], permissions: Option<fs::Permissions>) -> Result<()> {
    let parent = path.parent().context("Path has no parent folder")?;
    let mut staging = tempfile::NamedTempFile::new_in(parent)?;
    staging.write_all(bytes)?;
    if let Some(permissions) = permissions {
        staging.as_file().set_permissions(permissions)?;
    }
    staging.as_file().sync_all()?;
    staging.persist(path).map_err(|e| e.error)?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("resources.pak");
        let theme = dir.path().join("theme");
        fs::create_dir(&theme).unwrap();
        fs::write(theme.join("userChrome.css"), ":root { color: red; }").unwrap();
        let html =
            b"<webui-browser-app></webui-browser-app><script src='webui_browser.js'></script>";
        let js = b"class App {} customElements.define('webui-browser-app', App);";
        let start = 12 + 3 * 6;
        let mut bytes = vec![5, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0];
        for (id, offset) in [
            (1u16, start),
            (2, start + html.len()),
            (0, start + html.len() + js.len()),
        ] {
            bytes.extend(id.to_le_bytes());
            bytes.extend((offset as u32).to_le_bytes());
        }
        bytes.extend(html);
        bytes.extend(js);
        fs::write(&path, bytes).unwrap();
        (dir, path, theme)
    }

    #[test]
    fn applying_twice_does_not_stack_loaders() {
        let (_dir, path, theme) = setup();
        apply(&path, &theme, ThemeFiles::CssOnly).unwrap();
        let first = fs::read(&path).unwrap();
        apply(&path, &theme, ThemeFiles::CssOnly).unwrap();
        assert_eq!(fs::read(path).unwrap(), first);
    }

    #[test]
    fn scripts_are_ignored_without_the_unsafe_option() {
        let (_dir, path, theme) = setup();
        fs::write(theme.join("userChrome.js"), [0xff]).unwrap();
        assert_eq!(
            apply(&path, &theme, ThemeFiles::CssOnly).unwrap().state,
            PatchState::Applied
        );
    }

    #[test]
    fn css_only_leaves_the_theme_script_out_of_the_archive() {
        let (_dir, path, theme) = setup();
        fs::write(
            theme.join("userChrome.js"),
            "globalThis.themeScriptLoaded = true;",
        )
        .unwrap();
        apply(&path, &theme, ThemeFiles::CssOnly).unwrap();
        let pak = Pak::parse(&fs::read(path).unwrap()).unwrap();
        assert!(
            !String::from_utf8(pak.resources[&2].clone())
                .unwrap()
                .contains("themeScriptLoaded")
        );
    }

    #[test]
    fn opting_in_embeds_the_theme_script() {
        let (_dir, path, theme) = setup();
        fs::write(
            theme.join("userChrome.js"),
            "globalThis.themeScriptLoaded = true;",
        )
        .unwrap();
        apply(&path, &theme, ThemeFiles::Unsafe).unwrap();
        let pak = Pak::parse(&fs::read(path).unwrap()).unwrap();
        let script = String::from_utf8(pak.resources[&2].clone()).unwrap();
        assert!(script.contains("globalThis.themeScriptLoaded = true;"));
    }

    #[test]
    fn applying_css_only_removes_a_previously_embedded_script() {
        let (_dir, path, theme) = setup();
        fs::write(
            theme.join("userChrome.js"),
            "globalThis.themeScriptLoaded = true;",
        )
        .unwrap();
        apply(&path, &theme, ThemeFiles::Unsafe).unwrap();
        apply(&path, &theme, ThemeFiles::CssOnly).unwrap();
        let pak = Pak::parse(&fs::read(path).unwrap()).unwrap();
        let script = String::from_utf8(pak.resources[&2].clone()).unwrap();
        assert!(!script.contains("themeScriptLoaded"));
    }

    #[test]
    fn opting_in_rejects_an_unreadable_script_before_changing_the_archive() {
        let (_dir, path, theme) = setup();
        let original = fs::read(&path).unwrap();
        fs::write(theme.join("userChrome.js"), [0xff]).unwrap();
        assert!(
            apply(&path, &theme, ThemeFiles::Unsafe)
                .unwrap_err()
                .to_string()
                .contains("Cannot read UTF-8")
        );
        assert_eq!(fs::read(path).unwrap(), original);
    }

    #[test]
    fn restore_recovers_the_exact_original_archive() {
        let (_dir, path, theme) = setup();
        let original = fs::read(&path).unwrap();
        apply(&path, &theme, ThemeFiles::CssOnly).unwrap();
        restore(&path).unwrap();
        restore(&path).unwrap();
        assert_eq!(fs::read(path).unwrap(), original);
    }

    #[test]
    fn restore_refuses_a_browser_update() {
        let (_dir, path, theme) = setup();
        apply(&path, &theme, ThemeFiles::CssOnly).unwrap();
        fs::write(&path, b"browser update").unwrap();
        assert!(
            restore(&path)
                .unwrap_err()
                .to_string()
                .contains("changed outside")
        );
    }

    #[test]
    fn apply_recovers_after_the_replacement_precedes_receipt_finalization() {
        let (_dir, path, theme) = setup();
        apply(&path, &theme, ThemeFiles::CssOnly).unwrap();
        let archive = Archive::open(&path).unwrap();
        let mut receipt = archive.read_receipt().unwrap().unwrap();
        receipt.pending = receipt.applied.take();
        archive.save_receipt(&receipt).unwrap();
        drop(archive);
        assert_eq!(
            apply(&path, &theme, ThemeFiles::CssOnly).unwrap().state,
            PatchState::Applied
        );
    }

    #[test]
    fn restore_refuses_a_corrupted_backup() {
        let (_dir, path, theme) = setup();
        let applied = apply(&path, &theme, ThemeFiles::CssOnly).unwrap();
        fs::write(applied.backup, b"broken").unwrap();
        assert!(
            restore(&path)
                .unwrap_err()
                .to_string()
                .contains("checksum mismatch")
        );
    }
}
