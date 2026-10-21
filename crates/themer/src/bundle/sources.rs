use std::{
    collections::HashMap,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use cap_std::{ambient_authority, fs::Dir};

#[derive(Debug)]
pub(super) struct Sources {
    dir: Dir,
    cache: HashMap<PathBuf, Arc<[u8]>>,
}

impl Sources {
    pub fn open(root: &Path) -> Result<Self> {
        Ok(Self {
            dir: Dir::open_ambient_dir(root, ambient_authority())
                .context("Cannot open theme folder")?,
            cache: HashMap::new(),
        })
    }

    pub fn resolve(&self, origin: &Path, specifier: &str) -> Result<PathBuf> {
        ensure!(
            !specifier.is_empty() && !specifier.contains(['\\', ':', '\0']),
            "Use a relative theme path with forward slashes: {specifier}"
        );
        let mut path = origin.parent().unwrap_or(Path::new("")).to_path_buf();
        for component in Path::new(specifier).components() {
            match component {
                Component::Normal(part) => path.push(part),
                Component::CurDir => (),
                Component::ParentDir => {
                    ensure!(path.pop(), "Path leaves the theme folder: {specifier}")
                }
                _ => bail!("Path leaves the theme folder: {specifier}"),
            }
        }
        self.dir
            .canonicalize(&path)
            .with_context(|| format!("Cannot resolve theme file {}", path.display()))
    }

    pub fn optional(&mut self, path: &str) -> Result<Option<Arc<[u8]>>> {
        match self.dir.open(path) {
            Ok(file) => self.read_open(Path::new(path), file).map(Some),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).with_context(|| format!("Cannot read theme file {path}")),
        }
    }

    pub fn read(&mut self, path: &Path) -> Result<Arc<[u8]>> {
        if let Some(bytes) = self.cache.get(path) {
            return Ok(bytes.clone());
        }
        let file = self
            .dir
            .open(path)
            .with_context(|| format!("Cannot read theme file {}", path.display()))?;
        self.read_open(path, file)
    }

    fn read_open(&mut self, path: &Path, mut file: cap_std::fs::File) -> Result<Arc<[u8]>> {
        ensure!(
            file.metadata()?.is_file(),
            "Not a theme file: {}",
            path.display()
        );
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let bytes: Arc<[u8]> = bytes.into();
        self.cache.insert(path.to_path_buf(), bytes.clone());
        Ok(bytes)
    }

    pub fn text(&mut self, path: &Path) -> Result<String> {
        String::from_utf8(self.read(path)?.to_vec())
            .with_context(|| format!("Theme file must be UTF-8: {}", path.display()))
    }

    pub fn asset(&mut self, path: &Path) -> Result<String> {
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let mime = match extension.as_str() {
            "png" => "image/png",
            "apng" => "image/apng",
            "jpg" | "jpeg" | "jfif" => "image/jpeg",
            "svg" => "image/svg+xml",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "avif" => "image/avif",
            "ico" | "cur" => "image/x-icon",
            "bmp" => "image/bmp",
            "woff" => "font/woff",
            "woff2" => "font/woff2",
            "ttf" => "font/ttf",
            "otf" => "font/otf",
            "ttc" | "otc" => "font/collection",
            "mp3" => "audio/mpeg",
            "wav" => "audio/wav",
            "ogg" | "oga" | "opus" => "audio/ogg",
            "flac" => "audio/flac",
            "aac" => "audio/aac",
            "m4a" => "audio/mp4",
            "mp4" | "m4v" => "video/mp4",
            "webm" => "video/webm",
            "ogv" => "video/ogg",
            "wasm" => "application/wasm",
            _ => bail!("Unsupported theme asset: {}", path.display()),
        };
        Ok(format!(
            "data:{mime};base64,{}",
            STANDARD.encode(self.read(path)?)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn directories_are_not_theme_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("folder")).unwrap();
        let mut sources = Sources::open(dir.path()).unwrap();
        assert!(sources.read(Path::new("folder")).is_err());
    }

    #[test]
    fn repeated_reads_share_the_cached_bytes() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("large.txt"), "too long").unwrap();
        let mut sources = Sources::open(dir.path()).unwrap();
        let first = sources.read(Path::new("large.txt")).unwrap();
        let second = sources.read(Path::new("large.txt")).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_cannot_read_files_outside_the_theme() {
        let theme = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("private.css"), ".private {}").unwrap();
        std::os::unix::fs::symlink(outside.path(), theme.path().join("escape")).unwrap();
        let sources = Sources::open(theme.path()).unwrap();
        assert!(
            sources
                .resolve(Path::new("userChrome.css"), "escape/private.css")
                .is_err()
        );
    }
}
