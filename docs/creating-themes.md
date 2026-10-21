# Create a ChromeThemer theme

Here's the theme format. Only `userChrome.css` is required:

```text
my-theme/
    userChrome.css
    userChrome.js    # optional
    colors.json      # optional
    styles/          # optional
    scripts/         # optional
    images/          # optional
```

The script entry can also be `userChrome.mjs`, `userChrome.ts`, `userChrome.mts`, `userChrome.jsx`, `userChrome.tsx`, `userChrome.cjs` or `userChrome.cts`. Pick one entry, then import the other files from it.

## Apply a theme

Close Chrome, select its executable and your theme folder in ChromeThemer, then click Apply and Launch. On Windows, the GUI needs Administrator rights.

After edits, close Chrome and apply again. Themes don't update automatically. To include `userChrome.js`, check **Enable loading unsafe theme files** before applying.

ChromeThemer's Launch button and shortcuts already enable Webium. For normal launches, enable the Webium flag in `chrome://flags`.

## Write the CSS

Here's a small `userChrome.css`:

```css
:host(webui-browser-app) webui-browser-searchbox {
	--cr-searchbox-border-radius: 6px;
}

:host(webui-browser-tab[active]) #title {
	font-weight: 600;
}
```

Internal rules use `:host(component-name)`.

The toolbar and extensions bar need separate rules because they're in different roots:

```css
:host(webui-browser-app) #searchBar cr-icon-button {
	--cr-icon-button-size: 28px;
	--cr-icon-button-icon-size: 16px;
}

:host(webui-browser-extensions-bar) cr-icon-button {
	--cr-icon-button-size: 28px;
	--cr-icon-button-icon-size: 16px;
}
```

The [selector list](theme-reference.md#webium-selectors) covers the components used by the bundled themes. These are Webium internals, so they can change between Chrome versions.

## Keep the user's Chrome colors

Leave out `colors.json` if you want the user's current palette. Use Chrome's color properties in your CSS:

```css
:host(webui-browser-app) #searchBar {
	background: var(--color-toolbar);
}

:host(webui-browser-tab[active]) .tab {
	color: var(--color-tab-foreground-active-frame-active);
}

:host(webui-browser-tab[active][inactive-frame]) .tab {
	color: var(--color-tab-foreground-active-frame-inactive);
}
```

Webium already loads `chrome://theme/colors.css?sets=ui,chrome` and updates it when Chrome's palette changes. You don't need an import.

Webium's searchbox uses `--color-searchbox-*` properties. You can map them to Chrome's omnibox colors on the app host:

```css
:host(webui-browser-app) {
	--color-searchbox-background: var(--color-sys-omnibox-container);
	--color-searchbox-foreground: var(--color-omnibox-text);
	--color-searchbox-placeholder: var(--color-omnibox-text-dimmed);
}
```

That's just the input. Suggestions and selection have their own properties, listed in the [reference](theme-reference.md#chrome-color-roles).

## Give the theme its own palette

To pick a palette instead, add `colors.json`:

```json
{
	"seed": "#385479",
	"scheme": "system",
	"variant": "neutral"
}
```

Accepted values are in the [palette reference](theme-reference.md#colorsjson).

## Size the omnibox and toolbars

Setting the omnibox's height variables isn't always enough. Its nested input can stay at 32px even when the outer box is 28px. For a 28px layout:

```css
:host(webui-browser-app) #addressBox {
	height: 28px;
}

:host(webui-browser-app) webui-browser-searchbox {
	--cr-searchbox-height: 28px;
	--cr-searchbox-input-height: 28px;
}

:host(cr-searchbox-input) #inputInnerContainer {
	height: 28px;
	min-height: 28px;
	align-items: center;
}

:host(cr-searchbox-input) #input {
	box-sizing: border-box;
	height: 28px;
	padding-block: 0;
}
```

Pinned extensions use `webui-browser-extension > cr-button > icon-from-table` across shadow roots. Size those separately from `cr-icon-button`. See the [Chrome 68 theme](../themes/chrome68-dark/userChrome.css) for the full set of rules.

Webium also has two omnibox icons: the searchbox's graphic and a separate location button. It hides the location button on New Tab.

## Add JavaScript

`userChrome.js` runs in an async wrapper after the app's first `updateComplete`. Nested components may need their own wait. This example tracks the pointer position within a tab:

```js
await customElements.whenDefined('webui-browser-tab-strip');
const app = document.querySelector('webui-browser-app');
const strip = app.shadowRoot.querySelector('webui-browser-tab-strip');
await strip.updateComplete;
const events = new AbortController();

strip.shadowRoot.addEventListener('pointermove', event => {
	const tab = event.composedPath().find(node =>
		node instanceof Element && node.localName === 'webui-browser-tab');
	if (!tab) return;
	const bounds = tab.getBoundingClientRect();
	tab.style.setProperty('--my-theme-pointer-x', `${event.clientX - bounds.left}px`);
}, {signal: events.signal});

window.addEventListener('pagehide', () => events.abort(), {once: true});
```

You can use `--my-theme-pointer-x` in a tab gradient. The script runs with Chrome UI privileges, and browser-internal APIs aren't stable. Script errors appear in the Webium console with the prefix `ChromeThemer userChrome.js:`.

## Split a theme into files

Import CSS from `userChrome.css`:

```css
@import './styles/tabs.css';
@import './styles/toolbar.css';
```

Paths are relative to the file that uses them. Put imports before other rules. You can use local images and fonts in `url(...)`:

```css
/* styles/toolbar.css */
:host(webui-browser-app) #backButton {
	background-image: url('../images/back.svg');
}
```

JavaScript uses normal modules:

```js
// userChrome.js
import { setupTabs } from './scripts/tabs.js';
import icon from './images/back.svg';

await setupTabs(icon);
```

```js
// scripts/tabs.js
export async function setupTabs(icon) {
	const app = document.querySelector('webui-browser-app');
	app.style.setProperty('--my-back-icon', `url("${icon}")`);
}
```

Keep files inside the theme folder and include extensions in imports. Local `import()`, npm packages and remote CSS imports aren't supported. See the [file type reference](theme-reference.md#imported-file-types) for other supported files and import options.

## Test with the CLI

The CLI patches an explicit `resources.pak` and ignores `colors.json`. Build it from the repo with:

```sh
cargo build --release -p chromethemer
```

For the Windows toolchain, see the [build instructions](building.md#windows-x64). The binary is `target/release/chromethemer.exe` on Windows.

```sh
target/release/chromethemer inspect /path/to/resources.pak
target/release/chromethemer apply /path/to/resources.pak /path/to/my-theme
target/release/chromethemer apply /path/to/resources.pak /path/to/my-theme --allow-unsafe-theme-files
target/release/chromethemer restore /path/to/resources.pak
```

Use `--allow-unsafe-theme-files` to include JS. Chrome needs to be closed when replacing its PAK.

## Inspect Webium

Right-click empty space in Webium and click Inspect.

## Webium quirks worth checking

- Test with pinned extensions. They can make the toolbar grow even when the empty bar looks right.
- Check New Tab as well as a loaded page. The location button and searchbox icon change between those states.
- Check inactive windows too. Chrome has separate color roles for their tabs and frames.

For failures specific to the loader, see [troubleshooting](theme-reference.md#troubleshooting). [Graphite](../themes/graphite/userChrome.css) and [Chrome 68 dark](../themes/chrome68-dark/userChrome.css) are working examples of fixed colors and custom tab geometry.
