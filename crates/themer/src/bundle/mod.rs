mod css;
mod js;
mod sources;

#[cfg(test)]
mod large_themes;

use std::path::Path;

use anyhow::{Context, Result, ensure};

use crate::ThemeFiles;
use sources::Sources;

pub(super) struct Theme {
    pub css: String,
    pub script: Option<String>,
}

pub(super) fn load(root: &Path, files: ThemeFiles) -> Result<Theme> {
    let mut sources = Sources::open(root)?;
    let entry = sources
        .resolve(Path::new(""), "userChrome.css")
        .context("Theme folder must contain a UTF-8 userChrome.css")?;
    let mut css = css::bundle(&mut sources, &entry)?;
    let script = match files {
        ThemeFiles::CssOnly => None,
        ThemeFiles::Unsafe => {
            let mut script = None;
            for name in [
                "userChrome.js",
                "userChrome.mjs",
                "userChrome.ts",
                "userChrome.mts",
                "userChrome.jsx",
                "userChrome.tsx",
                "userChrome.cjs",
                "userChrome.cts",
            ] {
                if let Some(bytes) = sources.optional(name)? {
                    ensure!(script.is_none(), "Use only one userChrome script entry");
                    script = Some((
                        name,
                        String::from_utf8(bytes.to_vec())
                            .with_context(|| format!("Cannot read UTF-8 {name}"))?,
                    ));
                }
            }
            match script {
                Some((name, source)) => {
                    let compiled = js::bundle(sources, Path::new(name), source)?;
                    css.push('\n');
                    css.push_str(&compiled.css);
                    Some(compiled.script)
                }
                None => None,
            }
        }
    };
    Ok(Theme { css, script })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn duplicate_script_entries_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["userChrome.css", "userChrome.js", "userChrome.ts"] {
            fs::write(dir.path().join(name), "").unwrap();
        }
        assert!(load(dir.path(), ThemeFiles::Unsafe).is_err());
    }

    #[test]
    fn css_only_does_not_read_a_typescript_entry() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("userChrome.css"), "").unwrap();
        fs::write(dir.path().join("userChrome.ts"), [0xff]).unwrap();
        assert!(load(dir.path(), ThemeFiles::CssOnly).is_ok());
    }
}
