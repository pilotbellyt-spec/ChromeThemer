# Theme reference

Examples are in the [theme guide](creating-themes.md).

## Theme files

| File | Required | Used by | Limit |
| --- | --- | --- | --- |
| `userChrome.css` | Yes | GUI and CLI Apply | 4 MiB of UTF-8 text |
| `userChrome.js` | No | GUI and CLI Apply with unsafe theme files enabled | 4 MiB of UTF-8 text |
| `colors.json` | No | GUI Apply | 16 KiB of JSON |

With unsafe theme files disabled, `userChrome.js` isn't loaded.

## CSS runtime

Your CSS is shared across Webium's document and shadow roots, including new roots added later. Use `:host(component-name)` for internal rules.

The loader uses `replaceSync()`, so `@import` won't load. Extra assets aren't bundled, and URLs aren't relative to the theme folder.

The loader sets `.tabstripDiv` to `app-region: drag` after your CSS.

## JavaScript runtime

Your script runs after `webui-browser-app` finishes its first `updateComplete`. Nested components may need their own wait.

You can use `await`, but not static `import` declarations. Use `import()` for modules the browser UI can access.

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
| Images don't load | The loader doesn't bundle extra files. Use data URLs. |
| JS doesn't run | Enable unsafe theme files before applying. |
| JS runs before a child is ready | Wait for that component's `updateComplete`. |
| New Tab has no omnibox icon | Don't hide the searchbox icon when the location button is hidden. |
| The omnibox is too tall | Its nested input may still be 32px. See the sizing rules in the guide. |
| Extensions shift the toolbar | Size the extensions bar's buttons separately from the main toolbar. |
| Native menus don't match | CSS only changes Webium. Use `colors.json` to change Chrome's palette too. |
| Window buttons don't work | Check whether the tabstrip's drag region covers them. |
