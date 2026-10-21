use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use cssparser::{Parser, ParserInput, ToCss, Token};

use super::sources::Sources;

pub(super) fn bundle(sources: &mut Sources, entry: &Path) -> Result<String> {
    Css {
        sources,
        stack: Vec::new(),
    }
    .file(entry)
}

struct Css<'a> {
    sources: &'a mut Sources,
    stack: Vec<PathBuf>,
}

impl Css<'_> {
    fn file(&mut self, path: &Path) -> Result<String> {
        ensure!(
            !self.stack.iter().any(|p| p == path),
            "CSS import cycle at {}",
            path.display()
        );
        let source = self.sources.text(path)?;
        self.stack.push(path.to_path_buf());
        let mut input = ParserInput::new(&source);
        let css = self
            .tokens(&mut Parser::new(&mut input), path, true, false)
            .with_context(|| format!("Cannot bundle CSS {}", path.display()));
        self.stack.pop();
        css
    }

    fn url(&mut self, origin: &Path, url: &str) -> Result<String> {
        if url.starts_with('#') || url.starts_with('/') || url.contains(':') {
            ensure!(
                !url.to_ascii_lowercase().starts_with("file:"),
                "file: URLs are not allowed in themes"
            );
            return Ok(url.to_owned());
        }
        let (path_query, fragment) = url.split_once('#').unwrap_or((url, ""));
        let path = path_query
            .split_once('?')
            .map_or(path_query, |(path, _)| path);
        let decoded = percent_encoding::percent_decode_str(path)
            .decode_utf8()
            .context("Invalid UTF-8 asset URL")?;
        let path = self.sources.resolve(origin, &decoded)?;
        let mut embedded = self.sources.asset(&path)?;
        if !fragment.is_empty() {
            embedded.push('#');
            embedded.push_str(fragment);
        }
        Ok(embedded)
    }

    fn tokens(
        &mut self,
        parser: &mut Parser<'_, '_>,
        path: &Path,
        top: bool,
        image_set: bool,
    ) -> Result<String> {
        let mut output = String::new();
        let mut imports_allowed = top;
        let mut layer_statement = false;
        loop {
            let start = parser.position();
            let token = match parser.next_including_whitespace_and_comments() {
                Ok(token) => token.clone(),
                Err(e) if matches!(e.kind, cssparser::BasicParseErrorKind::EndOfInput) => break,
                Err(e) => bail!("Invalid CSS: {e:?}"),
            };
            match token {
                Token::AtKeyword(ref name) if name.eq_ignore_ascii_case("import") => {
                    ensure!(top && imports_allowed, "@import must come before CSS rules");
                    let url = parser
                        .expect_url_or_string()
                        .map_err(|e| anyhow::anyhow!("Invalid @import: {e:?}"))?
                        .to_string();
                    ensure!(
                        !url.contains([':', '#', '?']) && !url.starts_with('/'),
                        "CSS @import must reference a local theme file: {url}"
                    );
                    let qualifier_start = parser.position();
                    let mut terminated = false;
                    while let Ok(token) = parser.next() {
                        if matches!(token, Token::Semicolon) {
                            terminated = true;
                            break;
                        }
                    }
                    ensure!(terminated, "CSS @import needs a semicolon");
                    let qualifiers = parser
                        .slice_from(qualifier_start)
                        .strip_suffix(';')
                        .unwrap()
                        .to_owned();
                    let decoded = percent_encoding::percent_decode_str(&url).decode_utf8()?;
                    let imported = self.sources.resolve(path, &decoded)?;
                    let css = self.file(&imported)?;
                    output.push_str(&qualify(&qualifiers, &css)?);
                }
                Token::AtKeyword(ref name) if name.eq_ignore_ascii_case("namespace") => {
                    bail!("@namespace cannot be merged into a Webium theme")
                }
                Token::AtKeyword(ref name) if name.eq_ignore_ascii_case("charset") => {
                    ensure!(
                        top && imports_allowed,
                        "@charset must come before CSS rules"
                    );
                    let encoding = parser
                        .expect_string()
                        .map_err(|e| anyhow::anyhow!("Invalid @charset: {e:?}"))?;
                    ensure!(
                        encoding.eq_ignore_ascii_case("utf-8"),
                        "Theme CSS must use UTF-8"
                    );
                    parser
                        .expect_semicolon()
                        .map_err(|e| anyhow::anyhow!("Invalid @charset: {e:?}"))?;
                }
                Token::UnquotedUrl(url) => {
                    let embedded = self.url(path, &url)?;
                    output.push_str(&format!("url({})", quoted(&embedded)));
                }
                Token::Function(ref name) if name.eq_ignore_ascii_case("url") => {
                    let url = parser
                        .parse_nested_block(|inner| {
                            let url = inner.expect_string_cloned()?;
                            inner.expect_exhausted()?;
                            Ok::<_, cssparser::ParseError<'_, String>>(url.to_string())
                        })
                        .map_err(|e| anyhow::anyhow!("Invalid url(): {e:?}"))?;
                    let embedded = self.url(path, &url)?;
                    output.push_str(&format!("url({})", quoted(&embedded)));
                }
                Token::QuotedString(url) if image_set => {
                    let embedded = self.url(path, &url)?;
                    output.push_str(&quoted(&embedded));
                }
                Token::Function(_)
                | Token::ParenthesisBlock
                | Token::SquareBracketBlock
                | Token::CurlyBracketBlock => {
                    let prefix = parser.slice_from(start).to_owned();
                    let is_image_set = matches!(&token, Token::Function(name) if name.eq_ignore_ascii_case("image-set") || name.eq_ignore_ascii_case("-webkit-image-set"));
                    let closing = match token {
                        Token::SquareBracketBlock => "]",
                        Token::CurlyBracketBlock => "}",
                        _ => ")",
                    };
                    let nested = parser
                        .parse_nested_block(|inner| {
                            self.tokens(inner, path, false, is_image_set)
                                .map_err(|e| inner.new_custom_error::<_, String>(e.to_string()))
                        })
                        .map_err(|e| anyhow::anyhow!("Invalid CSS block: {e:?}"))?;
                    output.push_str(&prefix);
                    output.push_str(&nested);
                    output.push_str(closing);
                    if matches!(token, Token::CurlyBracketBlock) {
                        imports_allowed = false;
                    }
                }
                Token::BadUrl(_) | Token::BadString(_) => bail!("Invalid CSS URL or string"),
                _ => {
                    match &token {
                        Token::WhiteSpace(_) | Token::Comment(_) => (),
                        Token::AtKeyword(name)
                            if imports_allowed && name.eq_ignore_ascii_case("layer") =>
                        {
                            layer_statement = true
                        }
                        Token::Semicolon if layer_statement => layer_statement = false,
                        _ if layer_statement => (),
                        _ => imports_allowed = false,
                    }
                    output.push_str(parser.slice_from(start));
                }
            }
        }
        Ok(output)
    }
}

fn quoted(text: &str) -> String {
    Token::QuotedString(text.into()).to_css_string()
}

fn qualify(qualifiers: &str, css: &str) -> Result<String> {
    let mut input = ParserInput::new(qualifiers);
    let mut parser = Parser::new(&mut input);
    let mut wrappers = Vec::new();
    if parser
        .try_parse(|p| p.expect_ident_matching("layer"))
        .is_ok()
    {
        wrappers.push("@layer {\n".to_owned());
    } else if parser
        .try_parse(|p| p.expect_function_matching("layer"))
        .is_ok()
    {
        let name = parser
            .parse_nested_block(|p| {
                let start = p.position();
                while p.next().is_ok() {}
                Ok::<_, cssparser::ParseError<'_, String>>(p.slice_from(start).to_owned())
            })
            .map_err(|e| anyhow::anyhow!("Invalid import layer: {e:?}"))?;
        wrappers.push(format!("@layer {name} {{\n"));
    }
    if parser
        .try_parse(|p| p.expect_function_matching("supports"))
        .is_ok()
    {
        let condition = parser
            .parse_nested_block(|p| {
                let start = p.position();
                while p.next().is_ok() {}
                Ok::<_, cssparser::ParseError<'_, String>>(p.slice_from(start).to_owned())
            })
            .map_err(|e| anyhow::anyhow!("Invalid import supports: {e:?}"))?;
        // Import supports() accepts a bare declaration; @supports needs parentheses.
        let condition = condition.trim();
        let condition = if condition.starts_with('(')
            || condition.starts_with("not ")
            || condition.starts_with("selector(")
        {
            condition.to_owned()
        } else {
            format!("({condition})")
        };
        wrappers.push(format!("@supports {condition} {{\n"));
    }
    let start = parser.position();
    while parser.next().is_ok() {}
    let media = parser.slice_from(start).trim();
    if !media.is_empty() {
        wrappers.push(format!("@media {media} {{\n"));
    }
    let mut output = wrappers.concat();
    output.push_str(css);
    for _ in wrappers {
        output.push_str("\n}");
    }
    output.push('\n');
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn compile(files: &[(&str, &str)]) -> Result<String> {
        let dir = tempfile::tempdir()?;
        for (path, source) in files {
            let path = dir.path().join(path);
            fs::create_dir_all(path.parent().unwrap())?;
            fs::write(path, source)?;
        }
        let mut sources = Sources::open(dir.path())?;
        bundle(&mut sources, Path::new("userChrome.css"))
    }

    #[test]
    fn ordinary_css_keeps_its_source_text() {
        let css = "/* url(fake.png) */\n:host(foo) { --text: 'url(fake.png)'; color: var(--color-toolbar); }\n";
        assert_eq!(compile(&[("userChrome.css", css)]).unwrap(), css);
    }

    #[test]
    fn url_strings_use_css_escapes_not_json_escapes() {
        let css = compile(&[(
            "userChrome.css",
            ".a { background: url('data:text/plain,a\\9 b'); }",
        )])
        .unwrap();
        assert!(css.contains("url(\"data:text/plain,a\\9 b\")"), "{css}");
    }

    #[test]
    fn repeated_imports_keep_their_cascade_position() {
        let css = compile(&[
            (
                "userChrome.css",
                "@import 'a.css'; @import 'b.css'; @import 'a.css';",
            ),
            ("a.css", ".a {}"),
            ("b.css", ".b {}"),
        ])
        .unwrap();
        assert_eq!(css.matches(".a {}").count(), 2);
        assert!(
            css.find(".a {}").unwrap() < css.find(".b {}").unwrap()
                && css.rfind(".a {}").unwrap() > css.find(".b {}").unwrap()
        );
    }

    #[test]
    fn import_conditions_wrap_the_imported_rules() {
        let css = compile(&[("userChrome.css", "@layer base.controls; @import 'a.css' layer(base.controls) supports(display: grid) screen and (min-width: 500px);"),
            ("a.css", ".a {}")]).unwrap();
        assert!(css.contains("@layer base.controls {\n@supports (display: grid) {\n@media screen and (min-width: 500px) {\n.a {}"), "{css}");
    }

    #[test]
    fn asset_paths_are_relative_to_the_imported_stylesheet() {
        let css = compile(&[
            ("userChrome.css", "@import 'styles/a.css';"),
            (
                "styles/a.css",
                ".a { background: url('../icons/a%20b.svg#mark'); }",
            ),
            ("icons/a b.svg", "<svg/>"),
        ])
        .unwrap();
        assert!(
            css.contains("data:image/svg+xml;base64,PHN2Zy8+#mark"),
            "{css}"
        );
    }

    #[test]
    fn image_set_string_sources_are_embedded() {
        let css = compile(&[
            (
                "userChrome.css",
                ".a { background: image-set('a.svg' 1x, url(a.svg) 2x); }",
            ),
            ("a.svg", "<svg/>"),
        ])
        .unwrap();
        assert_eq!(css.matches("data:image/svg+xml;base64,").count(), 2);
    }

    #[test]
    fn image_set_type_descriptors_are_not_file_paths() {
        let css = compile(&[
            (
                "userChrome.css",
                ".a { background: image-set(url(a.svg) type('image/svg+xml')); }",
            ),
            ("a.svg", "<svg/>"),
        ])
        .unwrap();
        assert!(css.contains("type('image/svg+xml')"));
    }

    #[test]
    fn font_sources_are_embedded() {
        let css = compile(&[
            (
                "userChrome.css",
                "@font-face { font-family: Test; src: local('Test'), url(font.woff2); }",
            ),
            ("font.woff2", "fixture"),
        ])
        .unwrap();
        assert!(css.contains("local('Test'), url(\"data:font/woff2;base64,"));
    }

    #[test]
    fn circular_imports_fail() {
        let error = compile(&[
            ("userChrome.css", "@import 'a.css';"),
            ("a.css", "@import 'userChrome.css';"),
        ])
        .unwrap_err();
        assert!(format!("{error:#}").contains("CSS import cycle"));
    }

    #[test]
    fn imports_after_style_rules_fail_instead_of_changing_their_meaning() {
        let error = compile(&[
            ("userChrome.css", ".a {} @import 'a.css';"),
            ("a.css", ".b {}"),
        ])
        .unwrap_err();
        assert!(format!("{error:#}").contains("@import must come before CSS rules"));
    }

    #[test]
    fn remote_imports_fail_instead_of_disappearing_in_replace_sync() {
        assert!(
            compile(&[("userChrome.css", "@import 'https://example.com/theme.css';")]).is_err()
        );
    }

    #[test]
    fn import_paths_cannot_escape_the_theme() {
        assert!(compile(&[("userChrome.css", "@import '../outside.css';")]).is_err());
    }

    #[test]
    fn encoded_asset_paths_cannot_escape_the_theme() {
        assert!(
            compile(&[(
                "userChrome.css",
                ".a { background: url('%2e%2e/outside.svg'); }"
            )])
            .is_err()
        );
    }
}
