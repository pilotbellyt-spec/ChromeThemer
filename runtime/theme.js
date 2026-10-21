(() => {
  const sheet = new CSSStyleSheet();
  sheet.replaceSync(__CHROMETHEMER_CSS__ + `
    :host(webui-browser-app) .tabstripDiv {
      app-region: drag;
      -webkit-app-region: drag;
    }
  `);
  installFonts(sheet);
  const roots = new WeakMap();

  function installFonts(styles, media = []) {
    for (let i = styles.cssRules.length - 1; i >= 0; --i) {
      const rule = styles.cssRules[i];
      if (rule instanceof CSSMediaRule) {
        installFonts(rule, [...media, matchMedia(rule.conditionText)]);
      } else if (rule instanceof CSSSupportsRule) {
        if (CSS.supports(rule.conditionText)) installFonts(rule, media);
      } else if (rule instanceof CSSFontFaceRule) {
        const encoded = rule.style.getPropertyValue('src').match(/data:font\/[\w-]+;base64,([A-Za-z0-9+/=]+)/);
        if (!encoded) continue;
        const descriptors = {};
        for (const [name, property] of Object.entries({
          style: 'font-style', weight: 'font-weight', stretch: 'font-stretch', variant: 'font-variant',
          unicodeRange: 'unicode-range', featureSettings: 'font-feature-settings',
          variationSettings: 'font-variation-settings', display: 'font-display',
          ascentOverride: 'ascent-override', descentOverride: 'descent-override',
          lineGapOverride: 'line-gap-override', sizeAdjust: 'size-adjust',
        })) {
          const value = rule.style.getPropertyValue(property);
          if (value) descriptors[name] = value;
        }
        // Webium's font-src blocks data URLs. Buffer-backed FontFace needs no URL fetch.
        const bytes = Uint8Array.from(atob(encoded[1]), byte => byte.charCodeAt(0));
        let font;
        try {
          font = new FontFace(rule.style.getPropertyValue('font-family'), bytes, descriptors);
        } catch (error) {
          console.warn('ChromeThemer font:', error);
          continue;
        }
        styles.deleteRule(i);
        const sync = () => {
          if (media.every(query => query.matches)) document.fonts.add(font);
          else document.fonts.delete(font);
        };
        for (const query of media) query.addEventListener('change', sync);
        font.loaded.catch(error => console.warn('ChromeThemer font:', error));
        sync();
      } else if (rule.cssRules && typeof rule.deleteRule === 'function') {
        installFonts(rule, media);
      }
    }
  }

  function adopt(root) {
    if (!root.adoptedStyleSheets.includes(sheet)) {
      root.adoptedStyleSheets = [...root.adoptedStyleSheets, sheet];
    }
  }

  function discover(node) {
    if (node instanceof Element && node.shadowRoot) watch(node.shadowRoot);
    for (const el of node.querySelectorAll?.('*') ?? []) {
      if (el.shadowRoot) watch(el.shadowRoot);
    }
  }

  function watch(root) {
    adopt(root);
    if (roots.has(root)) return;
    const observer = new MutationObserver(records => {
      adopt(root);
      for (const record of records) {
        for (const node of record.addedNodes) discover(node);
      }
    });
    roots.set(root, observer);
    observer.observe(root, { childList: true, subtree: true });
    discover(root);
  }

  const attachShadow = Element.prototype.attachShadow;
  Element.prototype.attachShadow = function (options) {
    const root = attachShadow.call(this, options);
    watch(root);
    // Lit installs its own sheets after attachShadow returns.
    queueMicrotask(() => adopt(root));
    return root;
  };
  const define = customElements.define;
  customElements.define = function (name, constructor, options) {
    if (name === 'webui-browser-app') {
      installOmniboxFocus(constructor.prototype);
      const maximize = constructor.prototype.onMaximizeClick_;
      constructor.prototype.onMaximizeClick_ = function (event) {
        if (Math.max(window.innerWidth, window.outerWidth) >= screen.availWidth &&
            Math.max(window.innerHeight, window.outerHeight) >= screen.availHeight) {
          return this.onRestoreClick_(event);
        }
        return maximize.call(this, event);
      };
      customElements.define = define;
    }
    return define.call(this, name, constructor, options);
  };

  function installOmniboxFocus(prototype) {
    const focusLocation = prototype.setFocusToLocationBar;
    let focusBridge;
    prototype.setFocusToLocationBar = function (isUserInitiated) {
      if (!focusBridge) {
        focusBridge = document.createElement('iframe');
        focusBridge.tabIndex = -1;
        focusBridge.setAttribute('aria-hidden', 'true');
        focusBridge.style.cssText = 'position:fixed;width:1px;height:1px;opacity:0;pointer-events:none';
        document.body.append(focusBridge);
      }
      // SurfaceEmbed keeps routing keys to the page until the focused frame changes.
      focusBridge.contentWindow.focus();
      return focusLocation.call(this, isUserInitiated);
    };
  }

  function middleTab(event) {
    if (event.button !== 1) return;
    return event.composedPath().find(node =>
      node instanceof Element && node.localName === 'webui-browser-tab');
  }

  document.addEventListener('mousedown', event => {
    if (!middleTab(event)) return;
    event.preventDefault();
    event.stopPropagation();
  }, true);
  document.addEventListener('auxclick', event => {
    const tab = middleTab(event);
    if (!tab) return;
    event.preventDefault();
    event.stopPropagation();
    tab.shadowRoot.querySelector('.close').click();
  }, true);
  watch(document);
  document.documentElement.dataset.chromeThemer = 'active';
})();
