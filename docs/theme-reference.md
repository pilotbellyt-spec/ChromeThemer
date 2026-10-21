# Theme reference

Examples are in the [theme guide](creating-themes.md).

## Theme files

| File | Required | Used by | Format |
| --- | --- | --- | --- |
| `userChrome.css` | Yes | GUI and CLI Apply | UTF-8 CSS |
| `userChrome.js` | No | GUI and CLI Apply with unsafe theme files enabled | UTF-8 JavaScript |
| `colors.json` | No | GUI Apply | JSON |

The script entry can instead use `.mjs`, `.ts`, `.mts`, `.jsx`, `.tsx`, `.cjs` or `.cts`. Only one `userChrome` script entry is allowed.

## CSS runtime

Your CSS is shared across Webium's document and shadow roots, including new roots added later. Use `:host(component-name)` for internal rules.

Local `@import` supports media queries, `supports()` and cascade layers. Put imports before style rules. Circular imports, remote imports and `@namespace` aren't supported.

Local images and fonts work in `url(...)` and `image-set()`. Paths are relative to the CSS file that uses them. SVG fragments work too.

Local paths use forward slashes and must stay inside the theme folder, including symlink targets. For local `@font-face` files, only the first embedded source is used.

The loader sets `.tabstripDiv` to `app-region: drag` after your CSS.

## Imported file types

| Type | Extensions | Imported as |
| --- | --- | --- |
| JavaScript | `.js`, `.mjs`, `.cjs`, `.jsx` | Code |
| TypeScript | `.ts`, `.mts`, `.cts`, `.tsx` | Code |
| JSON | `.json` | Parsed object or value |
| CSS | `.css` | Applied stylesheet, default export is CSS text |
| Text | `.txt`, `.html`, `.htm`, `.xml`, `.md`, `.csv`, `.yaml`, `.yml`, `.toml`, `.glsl`, `.wgsl` | String, not parsed |
| Images and cursors | `.png`, `.apng`, `.jpg`, `.jpeg`, `.jfif`, `.svg`, `.gif`, `.webp`, `.avif`, `.ico`, `.cur`, `.bmp` | Data URL |
| Fonts | `.woff`, `.woff2`, `.ttf`, `.otf`, `.ttc`, `.otc` | Data URL |
| Audio | `.mp3`, `.wav`, `.ogg`, `.oga`, `.opus`, `.flac`, `.aac`, `.m4a` | Data URL |
| Video | `.mp4`, `.m4v`, `.webm`, `.ogv` | Data URL |
| WebAssembly | `.wasm` | Data URL, not executed |

Import options:

| Query | Default export |
| --- | --- |
| `?raw` | UTF-8 string |
| `?url` | Data URL for a supported asset type |
| `?base64` | Base64 string |
| `?binary` | `Uint8Array` |

`?raw`, `?base64` and `?binary` accept any local file extension. Raw CSS isn't applied automatically. CSS imported normally from scripts is appended after `userChrome.css`, in module execution order. Sass, Less and Stylus aren't compiled.

JSX needs a supplied local factory or runtime. The classic JSX pragmas `@jsxRuntime classic` and `@jsx h` use an imported `h` function. Bare package imports don't resolve.

Webium can block audio and video data URLs.

## JavaScript runtime

Your script runs after `webui-browser-app` finishes its first `updateComplete`. Nested components may need their own wait.

Static relative imports, exports, local CommonJS `require()` and top-level `await` work. Include file extensions in paths.

Static imports of Chrome-native modules aren't supported. Those use `import()` with a literal `/...`, `//resources/...`, `chrome://...` or `chrome-untrusted://...` URL. Local dynamic imports and package imports aren't supported.

Top-level `return` works in `userChrome.js` without static imports or exports.

Scripts run with Chrome UI privileges. Errors appear in the Webium console with the prefix `ChromeThemer userChrome.js:`.

## Webium selectors

These selectors are used by the bundled themes. They can change between Chrome versions.

| Component root | Elements or state | Purpose |
| --- | --- | --- |
| `webui-browser-app` | `.titlebarDiv`, `.tabstripDiv`, `.captionButtonsDiv` | Titlebar, tabstrip area, and window button area |
| `webui-browser-app` | `.caption-button` | Window buttons |
| `webui-browser-app` | `#searchBar` | Main toolbar |
| `webui-browser-app` | `#backButton`, `#forwardButton`, `.icon-refresh`, `.icon-clear` | Navigation and reload/stop controls |
| `webui-browser-app` | `#addressBox`, `#address` | Address bar container and searchbox host |
| `webui-browser-app` | `#locationIconButton`, `#locationIcon` | Page info button and icon |
| `webui-browser-app` | `#extensionsBar`, `#bookmarksButton`, `#avatarButton`, `#appMenuButton` | Toolbar controls and nested extensions bar |
| `webui-browser-app` | `#bookmarkBar`, `content-region`, `#sidePanel` | Bookmarks, page container, and side panel |
| `webui-browser-tab-strip` | `#tabstrip`, `#newTabButton`, `webui-browser-tab` | Tab row and new-tab control |
| `webui-browser-tab` | `.tab`, `#faviconContainer`, `#title`, `.close`, `#bottomCorners` | Tab contents and shape |
| `webui-browser-tab` | `[active]`, `[inactive-frame]`, `[drag-in-progress]` on the host | Tab state |
| `webui-browser-searchbox` | `#inputWrapper`, `#input`, `#matches` | Address bar wrapper, nested input component, and suggestions |
| `cr-searchbox-input` | `#inputInnerContainer`, `#input`, `#icon` | Text input and searchbox icon |
| `webui-browser-extensions-bar` | `#extensionsMenuButton`, `webui-browser-extension` | Extensions menu and pinned extensions |
| `webui-browser-extension` | `cr-button`, `icon-from-table` | Extension button and icon |
| `webui-browser-bookmark` | `.bookmark`, `.bookmarkTitle` | Bookmark control and label |
| `cr-icon-button` | `#icon`, `cr-icon`, `#maskedImage` | Button icon |

## Useful CSS properties

| Set on | Properties |
| --- | --- |
| `cr-icon-button` | `--cr-icon-button-size`, `--cr-icon-button-icon-size`, `--cr-icon-button-margin-start`, `--cr-icon-button-margin-end`, `--cr-icon-button-fill-color` |
| `cr-button` | `--cr-button-height` |
| `webui-browser-searchbox` | `--cr-searchbox-height`, `--cr-searchbox-input-height`, `--cr-searchbox-border-radius`, `--cr-searchbox-font-size`, `--cr-searchbox-shadow`, `--cr-searchbox-icon-size-in-searchbox` |
| `icon-from-table` | `--icon-size` |

For omnibox sizing, see the [guide](creating-themes.md#size-the-omnibox-and-toolbars).

### Chrome color roles

These come from Chrome's `colors.css`, which Webium already loads:

| Purpose | Properties |
| --- | --- |
| Window frame | `--color-frame-active`, `--color-frame-inactive` |
| Toolbar | `--color-toolbar`, `--color-toolbar-text`, `--color-toolbar-button-icon` |
| Active tab | `--color-tab-background-active-frame-active`, `--color-tab-foreground-active-frame-active` |
| Inactive tab | `--color-tab-background-inactive-frame-active`, `--color-tab-foreground-inactive-frame-active` |
| Unfocused window's tabs | Corresponding properties ending in `frame-inactive` |
| Searchbox | `--color-searchbox-background`, `--color-searchbox-foreground`, `--color-searchbox-placeholder`, `--color-searchbox-search-icon-background` |
| Searchbox results | `--color-searchbox-results-background`, `--color-searchbox-results-background-selected`, `--color-searchbox-results-foreground`, `--color-searchbox-results-url` |

## colors.json

All three fields are required. Unknown fields are rejected.

| Field | Accepted values | Meaning |
| --- | --- | --- |
| `seed` | `#RRGGBB` | Base color for the palette |
| `scheme` | `system`, `light`, `dark` | Light or dark mode |
| `variant` | `tonalSpot`, `neutral`, `vibrant`, `expressive` | Palette style |

GUI Apply changes the palette in all discovered regular profiles. It uses default profile folders, not a custom `--user-data-dir`. The CLI ignores this file.

Leave it out to keep the current palette. To undo a previous palette Apply, use Restore.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| Edits don't show up | Apply again. Launch doesn't reread theme files. |
| Images don't load | Use a local CSS `url(...)` or JS asset import. Plain relative strings in JS aren't bundled. |
| JS doesn't run | Enable unsafe theme files before applying. |
| JS runs before a child is ready | Wait for that component's `updateComplete`. |
| New Tab has no omnibox icon | Don't hide the searchbox icon when the location button is hidden. |
| The omnibox is too tall | Its nested input may still be 32px. See the sizing rules in the guide. |
| Extensions shift the toolbar | Size the extensions bar's buttons separately from the main toolbar. |
| Native menus don't match | CSS only changes Webium. Use `colors.json` to change Chrome's palette too. |
| Window buttons don't work | Check whether the tabstrip's drag region covers them. |
