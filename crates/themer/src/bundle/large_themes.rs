use std::{fs, io::Write};

use super::load;
use crate::{ThemeFiles, pak, palette};

const MIB: usize = 1024 * 1024;

fn theme() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("userChrome.css"), "").unwrap();
    dir
}

#[test]
fn stylesheets_larger_than_four_mib_are_accepted() {
    let dir = theme();
    let css = format!("/*{}*/", "x".repeat(5 * MIB));
    fs::write(dir.path().join("userChrome.css"), &css).unwrap();
    assert_eq!(load(dir.path(), ThemeFiles::CssOnly).unwrap().css, css);
}

#[test]
fn scripts_larger_than_four_mib_are_accepted() {
    let dir = theme();
    let script = format!("/*{}*/", "x".repeat(5 * MIB));
    fs::write(dir.path().join("userChrome.js"), &script).unwrap();
    assert_eq!(
        load(dir.path(), ThemeFiles::Unsafe)
            .unwrap()
            .script
            .as_deref(),
        Some(script.as_str())
    );
}

#[test]
fn assets_larger_than_sixteen_mib_are_embedded() {
    let dir = theme();
    fs::write(
        dir.path().join("userChrome.css"),
        ".icon { background: url(icon.svg); }",
    )
    .unwrap();
    let svg = format!(
        "<svg xmlns='http://www.w3.org/2000/svg'><!--{}--></svg>",
        "x".repeat(17 * MIB)
    );
    fs::write(dir.path().join("icon.svg"), svg).unwrap();
    assert!(load(dir.path(), ThemeFiles::CssOnly).unwrap().css.len() > 16 * MIB);
}

#[test]
fn total_theme_input_can_exceed_sixty_four_mib() {
    let dir = theme();
    let css = format!("/*{}*/", "x".repeat(4 * MIB - 4));
    let mut imports = String::new();
    for i in 0..17 {
        fs::write(dir.path().join(format!("part{i}.css")), &css).unwrap();
        imports.push_str(&format!("@import 'part{i}.css';\n"));
    }
    fs::write(dir.path().join("userChrome.css"), imports).unwrap();
    assert!(load(dir.path(), ThemeFiles::CssOnly).unwrap().css.len() > 64 * MIB);
}

#[test]
fn expanded_css_can_exceed_sixty_four_mib() {
    let dir = theme();
    fs::write(
        dir.path().join("part.css"),
        format!("/*{}*/", "x".repeat(MIB)),
    )
    .unwrap();
    fs::write(
        dir.path().join("userChrome.css"),
        "@import 'part.css';\n".repeat(65),
    )
    .unwrap();
    assert!(load(dir.path(), ThemeFiles::CssOnly).unwrap().css.len() > 64 * MIB);
}

#[test]
fn themes_can_import_more_than_1024_files() {
    let dir = theme();
    let mut imports = String::new();
    for i in 0..1025 {
        fs::write(dir.path().join(format!("part{i}.css")), "").unwrap();
        imports.push_str(&format!("@import 'part{i}.css';\n"));
    }
    fs::write(dir.path().join("userChrome.css"), imports).unwrap();
    assert!(load(dir.path(), ThemeFiles::CssOnly).is_ok());
}

#[test]
fn css_imports_can_nest_more_than_sixty_four_levels() {
    let dir = theme();
    fs::write(dir.path().join("userChrome.css"), "@import 'part0.css';").unwrap();
    for i in 0..80 {
        fs::write(
            dir.path().join(format!("part{i}.css")),
            format!("@import 'part{}.css';", i + 1),
        )
        .unwrap();
    }
    fs::write(dir.path().join("part80.css"), ".deep {}").unwrap();
    assert!(
        load(dir.path(), ThemeFiles::CssOnly)
            .unwrap()
            .css
            .contains(".deep {}")
    );
}

#[test]
fn colors_json_larger_than_sixteen_kib_is_accepted() {
    let dir = theme();
    let json = format!(
        "{}{{\"seed\":\"#385479\",\"scheme\":\"system\",\"variant\":\"neutral\"}}",
        " ".repeat(17 * 1024)
    );
    fs::write(dir.path().join("colors.json"), json).unwrap();
    assert!(palette::from_theme(dir.path()).unwrap().is_some());
}

#[test]
fn gzip_resources_can_decode_more_than_sixty_four_mib() {
    let source = vec![b'x'; 65 * MIB];
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(&source).unwrap();
    let encoded = encoder.finish().unwrap();
    assert_eq!(pak::decode(&encoded).unwrap(), source);
}

#[test]
fn brotli_resources_can_decode_more_than_sixty_four_mib() {
    let source = vec![b'x'; 65 * MIB];
    let mut encoded = vec![0x1e, 0x9b];
    encoded.extend_from_slice(&(source.len() as u64).to_le_bytes()[..6]);
    {
        let mut encoder = brotli::CompressorWriter::new(&mut encoded, 4096, 0, 22);
        encoder.write_all(&source).unwrap();
    }
    assert_eq!(pak::decode(&encoded).unwrap(), source);
}
