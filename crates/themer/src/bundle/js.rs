use std::{
    borrow::Cow,
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use oxc::{
    allocator::Allocator,
    ast::ast::{Expression, ImportExpression},
    ast_visit::{Visit, walk},
    parser::{ParseOptions, Parser},
    span::SourceType,
};
use rolldown::{
    Bundler, BundlerOptions, OutputFormat, Platform, TreeshakeOptions,
    plugin::{
        HookLoadArgs, HookLoadOutput, HookLoadReturn, HookResolveIdArgs, HookResolveIdOutput,
        HookResolveIdReturn, HookUsage, Plugin, PluginContext, SharedLoadPluginContext,
    },
};
use rolldown_common::{ImportKind, ModuleType, Output, side_effects::HookSideEffects};

use super::{css, sources::Sources};

const ENTRY: &str = "\0chromethemer-entry";
const FILE: &str = "\0chromethemer/";

#[derive(Clone, Copy)]
enum ImportAs {
    Default,
    Text,
    Url,
    Base64,
    Binary,
}

fn import_as(specifier: &str) -> Result<(&str, ImportAs)> {
    let Some((path, mode)) = specifier.split_once('?') else {
        return Ok((specifier, ImportAs::Default));
    };
    let mode = match mode {
        "raw" => ImportAs::Text,
        "url" => ImportAs::Url,
        "base64" => ImportAs::Base64,
        "binary" => ImportAs::Binary,
        _ => bail!("Unknown theme import query: ?{mode}"),
    };
    Ok((path, mode))
}

pub(super) struct JavaScript {
    pub script: String,
    pub css: String,
}

pub(super) fn bundle(sources: Sources, entry: &Path, source: String) -> Result<JavaScript> {
    let allocator = Allocator::default();
    let source_type = SourceType::from_path(entry).context("Unsupported script entry")?;
    let parsed = Parser::new(&allocator, &source, source_type)
        .with_options(ParseOptions {
            allow_return_outside_function: true,
            ..Default::default()
        })
        .parse();
    let module = entry.extension().is_none_or(|ext| ext != "js")
        || parsed
            .program
            .body
            .iter()
            .any(|statement| statement.is_module_declaration());
    check_dynamic_imports(&parsed.program, module)?;
    if !module {
        return Ok(JavaScript {
            script: source,
            css: String::new(),
        });
    }
    ensure!(
        parsed.diagnostics.is_empty(),
        "Invalid theme script module: {:?}",
        parsed.diagnostics
    );
    let styles = Arc::new(Mutex::new(HashMap::new()));
    let plugin = ThemeModules {
        sources: Mutex::new(sources),
        styles: styles.clone(),
        entry: entry.to_path_buf(),
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .build()?;
    runtime.block_on(async {
        let mut bundler = Bundler::with_plugins(
            BundlerOptions {
                input: Some(vec![ENTRY.to_owned().into()]),
                platform: Some(Platform::Browser),
                format: Some(OutputFormat::Esm),
                treeshake: TreeshakeOptions::Boolean(false),
                ..Default::default()
            },
            vec![ThemeModules::new_shared(plugin)],
        )
        .context("Cannot configure theme bundler")?;
        let output = bundler
            .generate()
            .await
            .context("Cannot bundle theme script")?;
        ensure!(
            output.assets.len() == 1,
            "Theme must bundle to a single JavaScript module"
        );
        let Some(Output::Chunk(chunk)) = output.assets.first() else {
            bail!("Theme did not produce JavaScript")
        };
        ensure!(
            chunk.imports.is_empty() && chunk.exports.is_empty(),
            "Theme contains an unbundled import or export"
        );
        let styles = styles
            .lock()
            .map_err(|_| anyhow::anyhow!("Theme style lock poisoned"))?;
        let mut css = String::new();
        for id in &chunk.modules.keys {
            if let Some(style) = styles.get(id.as_str()) {
                css.push_str(style);
                css.push('\n');
            }
        }
        Ok(JavaScript {
            script: chunk.code.clone(),
            css,
        })
    })
}

fn native_module(specifier: &str) -> bool {
    specifier.starts_with('/')
        || specifier.starts_with("chrome://")
        || specifier.starts_with("chrome-untrusted://")
}

fn check_dynamic_imports(program: &oxc::ast::ast::Program<'_>, module: bool) -> Result<()> {
    struct Imports {
        invalid: Option<String>,
        module: bool,
    }
    impl<'a> Visit<'a> for Imports {
        fn visit_import_expression(&mut self, expression: &ImportExpression<'a>) {
            match &expression.source {
                Expression::StringLiteral(literal)
                    if !literal.value.as_str().is_some_and(native_module) =>
                {
                    self.invalid = Some(format!(
                        "Use static import for local theme files: {:?}",
                        literal.value
                    ));
                }
                Expression::StringLiteral(_) => (),
                _ if self.module => {
                    self.invalid = Some(
                        "Bundled modules need literal Chrome-native URLs in import()".to_owned(),
                    )
                }
                _ => (),
            }
            walk::walk_import_expression(self, expression);
        }
    }
    let mut imports = Imports {
        invalid: None,
        module,
    };
    imports.visit_program(program);
    if let Some(message) = imports.invalid {
        bail!("{message}");
    }
    Ok(())
}

#[derive(Debug)]
struct ThemeModules {
    sources: Mutex<Sources>,
    styles: Arc<Mutex<HashMap<String, String>>>,
    entry: std::path::PathBuf,
}

impl Plugin for ThemeModules {
    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("chromethemer-files")
    }
    fn register_hook_usage(&self) -> HookUsage {
        HookUsage::ResolveId | HookUsage::Load
    }

    async fn resolve_id(
        &self,
        _ctx: &PluginContext,
        args: &HookResolveIdArgs<'_>,
    ) -> HookResolveIdReturn {
        if args.specifier == ENTRY {
            return Ok(Some(HookResolveIdOutput::from_id(ENTRY)));
        }
        if native_module(args.specifier) {
            ensure!(
                args.kind == ImportKind::DynamicImport,
                "Use await import(...) for Chrome-native modules: {}",
                args.specifier
            );
            return Ok(Some(HookResolveIdOutput {
                id: args.specifier.into(),
                external: Some(true.into()),
                normalize_external_id: Some(false),
                ..Default::default()
            }));
        }
        ensure!(
            matches!(args.kind, ImportKind::Import | ImportKind::Require),
            "Use static import for local theme files: {}",
            args.specifier
        );
        ensure!(
            args.specifier.starts_with("./") || args.specifier.starts_with("../"),
            "Only relative theme imports are supported: {}",
            args.specifier
        );
        let origin = match args.importer {
            Some(ENTRY) => Path::new(""),
            Some(importer) => Path::new(
                importer
                    .strip_prefix(FILE)
                    .context("Invalid theme module origin")?,
            ),
            None => bail!("Missing theme module origin"),
        };
        let sources = self
            .sources
            .lock()
            .map_err(|_| anyhow::anyhow!("Theme source lock poisoned"))?;
        let (specifier, _) = import_as(args.specifier)?;
        let path = sources.resolve(origin, specifier)?;
        let path = path
            .to_str()
            .context("Theme paths must be UTF-8")?
            .replace('\\', "/");
        let query = args
            .specifier
            .find('?')
            .map_or("", |position| &args.specifier[position..]);
        Ok(Some(HookResolveIdOutput {
            id: format!("{FILE}{path}{query}").into(),
            side_effects: Some(HookSideEffects::True),
            ..Default::default()
        }))
    }

    async fn load(&self, _ctx: SharedLoadPluginContext, args: &HookLoadArgs<'_>) -> HookLoadReturn {
        let (code, module_type) = if args.id == ENTRY {
            (
                format!(
                    "import {};",
                    serde_json::to_string(&format!("./{}", self.entry.display()))?
                ),
                ModuleType::Js,
            )
        } else {
            let (path, mode) = import_as(
                args.id
                    .strip_prefix(FILE)
                    .context("Invalid theme module path")?,
            )?;
            let path = Path::new(path);
            let mut sources = self
                .sources
                .lock()
                .map_err(|_| anyhow::anyhow!("Theme source lock poisoned"))?;
            match mode {
                ImportAs::Text => (sources.text(path)?, ModuleType::Text),
                ImportAs::Url => (
                    format!(
                        "export default {};",
                        serde_json::to_string(&sources.asset(path)?)?
                    ),
                    ModuleType::Js,
                ),
                ImportAs::Base64 | ImportAs::Binary => {
                    let encoded = STANDARD.encode(sources.read(path)?);
                    let expression = match mode {
                        ImportAs::Binary => format!(
                            "Uint8Array.from(atob({}), c => c.charCodeAt(0))",
                            serde_json::to_string(&encoded)?
                        ),
                        _ => serde_json::to_string(&encoded)?,
                    };
                    (format!("export default {expression};"), ModuleType::Js)
                }
                ImportAs::Default => {
                    match path.extension().and_then(|extension| extension.to_str()) {
                        Some(
                            extension @ ("js" | "mjs" | "cjs" | "jsx" | "ts" | "mts" | "cts"
                            | "tsx"),
                        ) => {
                            let source = sources.text(path)?;
                            let allocator = Allocator::default();
                            let parsed =
                                Parser::new(&allocator, &source, SourceType::from_path(path)?)
                                    .parse();
                            ensure!(
                                parsed.diagnostics.is_empty(),
                                "Invalid JavaScript {}: {:?}",
                                path.display(),
                                parsed.diagnostics
                            );
                            check_dynamic_imports(&parsed.program, true)?;
                            let module_type = match extension {
                                "jsx" => ModuleType::Jsx,
                                "ts" | "mts" | "cts" => ModuleType::Ts,
                                "tsx" => ModuleType::Tsx,
                                _ => ModuleType::Js,
                            };
                            (source, module_type)
                        }
                        Some("json") => (sources.text(path)?, ModuleType::Json),
                        Some(
                            "txt" | "html" | "htm" | "xml" | "md" | "csv" | "yaml" | "yml" | "toml"
                            | "glsl" | "wgsl",
                        ) => (sources.text(path)?, ModuleType::Text),
                        Some("css") => {
                            let style = css::bundle(&mut sources, path)?;
                            self.styles
                                .lock()
                                .map_err(|_| anyhow::anyhow!("Theme style lock poisoned"))?
                                .insert(args.id.to_owned(), style.clone());
                            (
                                format!("export default {};", serde_json::to_string(&style)?),
                                ModuleType::Js,
                            )
                        }
                        _ => (
                            format!(
                                "export default {};",
                                serde_json::to_string(&sources.asset(path)?)?
                            ),
                            ModuleType::Js,
                        ),
                    }
                }
            }
        };
        Ok(Some(HookLoadOutput {
            code: code.into(),
            module_type: Some(module_type),
            ..Default::default()
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn compile(files: &[(&str, &str)]) -> Result<String> {
        compile_entry("userChrome.js", files).map(|compiled| compiled.script)
    }

    fn compile_entry(entry: &str, files: &[(&str, &str)]) -> Result<JavaScript> {
        let dir = tempfile::tempdir()?;
        for (path, source) in files {
            let path = dir.path().join(path);
            fs::create_dir_all(path.parent().unwrap())?;
            fs::write(path, source)?;
        }
        let mut sources = Sources::open(dir.path())?;
        let source = sources.text(Path::new(entry))?;
        bundle(sources, Path::new(entry), source)
    }

    #[test]
    fn legacy_async_body_is_not_rewritten() {
        let source = "// keep this\nawait Promise.resolve();\nif (true) return;\n";
        assert_eq!(compile(&[("userChrome.js", source)]).unwrap(), source);
    }

    #[test]
    fn static_modules_and_top_level_await_are_bundled() {
        let script = compile(&[
            (
                "userChrome.js",
                "import {count} from './scripts/count.js'; globalThis.answer = count;",
            ),
            (
                "scripts/count.js",
                "await Promise.resolve(); export const count = 42;",
            ),
        ])
        .unwrap();
        assert!(script.contains("await Promise.resolve()"), "{script}");
        assert!(
            !script.contains("import ") && !script.contains("export "),
            "{script}"
        );
    }

    #[test]
    fn javascript_can_import_an_image_url() {
        let script = compile(&[
            (
                "userChrome.js",
                "import icon from './icons/tab.svg'; globalThis.themeIcon = icon;",
            ),
            ("icons/tab.svg", "<svg/>"),
        ])
        .unwrap();
        assert!(
            script.contains("data:image/svg+xml;base64,PHN2Zy8+"),
            "{script}"
        );
    }

    #[test]
    fn native_dynamic_import_keeps_its_chrome_url() {
        let script = compile(&[("userChrome.js", "import './local.js'; globalThis.loadNative = () => import('//resources/js/assert.js');"),
            ("local.js", "globalThis.localLoaded = true;")]).unwrap();
        assert!(
            script.contains("import(\"//resources/js/assert.js\")"),
            "{script}"
        );
    }

    #[test]
    fn local_dynamic_imports_fail_instead_of_changing_execution_order() {
        assert!(
            compile(&[
                (
                    "userChrome.js",
                    "import './local.js'; await import('./lazy.js');"
                ),
                ("local.js", ""),
                ("lazy.js", "globalThis.lazy = true;")
            ])
            .is_err()
        );
    }

    #[test]
    fn imports_cannot_leave_the_theme_folder() {
        assert!(compile(&[("userChrome.js", "import '../outside.js';")]).is_err());
    }

    #[test]
    fn package_imports_do_not_search_node_modules() {
        assert!(compile(&[("userChrome.js", "import 'some-package';")]).is_err());
    }

    #[test]
    fn missing_named_exports_fail_the_build() {
        assert!(
            compile(&[
                (
                    "userChrome.js",
                    "import {missing} from './local.js'; globalThis.bad = missing;"
                ),
                ("local.js", "export const present = 1;")
            ])
            .is_err()
        );
    }

    #[test]
    fn typescript_entries_are_transpiled() {
        let compiled = compile_entry("userChrome.ts", &[("userChrome.ts", "import {count} from './count.ts'; const answer: number = count; globalThis.themeAnswer = answer;"),
            ("count.ts", "export const count: number = 42;")]).unwrap();
        assert!(!compiled.script.contains(": number"), "{}", compiled.script);
    }

    #[test]
    fn tsx_can_use_a_local_jsx_factory() {
        let compiled = compile_entry("userChrome.tsx", &[("userChrome.tsx", "/** @jsxRuntime classic */\n/** @jsx h */\nimport {h} from './dom.js'; const title: string = 'test'; globalThis.themeElement = <button>{title}</button>;"),
            ("dom.js", "export function h(tag, props, text) { return {tag, props, text}; }")]).unwrap();
        assert!(
            compiled.script.contains("h(\"button\""),
            "{}",
            compiled.script
        );
    }

    #[test]
    fn json_imports_become_objects() {
        let script = compile(&[
            (
                "userChrome.js",
                "import palette from './palette.json'; globalThis.themeSeed = palette.seed;",
            ),
            ("palette.json", "{\"seed\":\"#385479\"}"),
        ])
        .unwrap();
        assert!(script.contains("#385479"));
    }

    #[test]
    fn html_imports_become_strings() {
        let script = compile(&[
            (
                "userChrome.js",
                "import template from './panel.html'; globalThis.themeTemplate = template;",
            ),
            ("panel.html", "<button>Panel</button>"),
        ])
        .unwrap();
        assert!(script.contains("<button>Panel</button>"));
    }

    #[test]
    fn css_imported_from_javascript_keeps_module_order() {
        let compiled = compile_entry(
            "userChrome.js",
            &[
                ("userChrome.js", "import './a.js'; import './second.css';"),
                ("a.js", "import './first.css';"),
                ("first.css", ".first {}"),
                ("second.css", ".second {}"),
            ],
        )
        .unwrap();
        assert!(
            compiled.css.find(".first {}").unwrap() < compiled.css.find(".second {}").unwrap(),
            "{}",
            compiled.css
        );
    }

    #[test]
    fn raw_css_is_a_string_without_automatic_application() {
        let compiled = compile_entry(
            "userChrome.js",
            &[
                (
                    "userChrome.js",
                    "import css from './panel.css?raw'; globalThis.panelCss = css;",
                ),
                ("panel.css", ".panel {}"),
            ],
        )
        .unwrap();
        assert!(compiled.css.is_empty());
        assert!(compiled.script.contains(".panel {}"));
    }

    #[test]
    fn binary_imports_produce_a_uint8_array() {
        let script = compile(&[
            (
                "userChrome.js",
                "import bytes from './table.bin?binary'; globalThis.themeBytes = bytes;",
            ),
            ("table.bin", "abc"),
        ])
        .unwrap();
        assert!(
            script.contains("Uint8Array.from(atob(\"YWJj\")"),
            "{script}"
        );
    }

    #[test]
    fn base64_imports_produce_an_encoded_string() {
        let script = compile(&[
            (
                "userChrome.js",
                "import encoded from './table.bin?base64'; globalThis.themeBase64 = encoded;",
            ),
            ("table.bin", "abc"),
        ])
        .unwrap();
        assert!(script.contains("YWJj"));
    }

    #[test]
    fn media_imports_produce_data_urls() {
        let script = compile(&[
            (
                "userChrome.js",
                "import sound from './click.mp3'; globalThis.themeSound = sound;",
            ),
            ("click.mp3", "fixture"),
        ])
        .unwrap();
        assert!(script.contains("data:audio/mpeg;base64,"));
    }

    #[test]
    fn invalid_json_fails_before_the_theme_is_applied() {
        assert!(
            compile(&[
                (
                    "userChrome.js",
                    "import palette from './palette.json'; globalThis.palette = palette;"
                ),
                ("palette.json", "{broken}")
            ])
            .is_err()
        );
    }

    #[test]
    fn local_commonjs_modules_are_bundled() {
        let script = compile(&[
            (
                "userChrome.js",
                "import palette from './palette.cjs'; globalThis.themeSeed = palette.seed;",
            ),
            ("palette.cjs", "module.exports = require('./palette.json');"),
            ("palette.json", "{\"seed\":\"#385479\"}"),
        ])
        .unwrap();
        assert!(script.contains("#385479"));
    }
}
