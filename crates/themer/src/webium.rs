use std::io::Write;

use anyhow::{Context, Result, ensure};
use flate2::{Compression, GzBuilder};

use crate::pak::{Pak, decode};

const LOADER: &str = include_str!("../../../runtime/theme.js");
const PATCH_MARKER: &str = "/* ChromeThemer bootstrap v1 */";

pub fn locate(pak: &Pak) -> Result<u16> {
    let mut html_found = false;
    let mut scripts = Vec::new();
    for (id, stored) in &pak.resources {
        let decoded = decode(stored).with_context(|| format!("Cannot decode resource {id}"))?;
        let Ok(source) = std::str::from_utf8(&decoded) else {
            continue;
        };
        html_found |= source.contains("<webui-browser-app") && source.contains("webui_browser.js");
        if source.contains("webui-browser-app") && source.contains("customElements.define(") {
            ensure!(
                !source.contains(PATCH_MARKER),
                "This archive is already patched; select its original backup"
            );
            scripts.push(*id);
        }
    }
    ensure!(
        html_found,
        "This resources.pak does not contain Webium's browser page"
    );
    ensure!(
        scripts.len() == 1,
        "Expected one Webium app module, found {}. This build is unsupported",
        scripts.len()
    );
    Ok(scripts[0])
}

pub fn patch(pak: &mut Pak, css: &str, script: Option<&str>) -> Result<u16> {
    let id = locate(pak)?;
    let stored = &pak.resources[&id];
    let source = String::from_utf8(decode(stored)?)?;
    let bootstrap = LOADER.replace("__CHROMETHEMER_CSS__", &serde_json::to_string(css)?);
    let mut patched = format!("{PATCH_MARKER}\n{bootstrap}\n{source}");
    if let Some(script) = script {
        patched.push_str("\n;(async () => {\nawait customElements.whenDefined('webui-browser-app');\nawait document.querySelector('webui-browser-app').updateComplete;\n");
        patched.push_str(script);
        patched.push_str(
            "\n})().catch(error => console.error('ChromeThemer userChrome.js:', error));\n",
        );
    }
    let encoded = if stored.starts_with(&[0x1f, 0x8b]) {
        let mut gzip = GzBuilder::new()
            .mtime(0)
            .write(Vec::new(), Compression::default());
        gzip.write_all(patched.as_bytes())?;
        gzip.finish()?
    } else if stored.starts_with(&[0x1e, 0x9b]) {
        let mut compressed = vec![0x1e, 0x9b];
        compressed.extend_from_slice(&(patched.len() as u64).to_le_bytes()[..6]);
        {
            let mut writer = brotli::CompressorWriter::new(&mut compressed, 4096, 6, 22);
            writer.write_all(patched.as_bytes())?;
        }
        compressed
    } else {
        patched.into_bytes()
    };
    pak.replace(id, encoded);
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unrelated_browser_archive_is_rejected() {
        let bytes = [5, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 18, 0, 0, 0];
        assert!(
            locate(&Pak::parse(&bytes).unwrap())
                .unwrap_err()
                .to_string()
                .contains("does not contain Webium")
        );
    }
}
