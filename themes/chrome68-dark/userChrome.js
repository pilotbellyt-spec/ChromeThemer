const strip = document.querySelector('webui-browser-app').shadowRoot
  .querySelector('webui-browser-tab-strip').shadowRoot;
const events = new AbortController();

strip.addEventListener('pointermove', event => {
  const tab = event.composedPath().find(node =>
    node instanceof Element && node.localName === 'webui-browser-tab');
  if (!tab) return;
  const bounds = tab.getBoundingClientRect();
  tab.style.setProperty('--classic-hover-x', `${event.clientX - bounds.left}px`);
  tab.style.setProperty('--classic-hover-y', `${event.clientY - bounds.top}px`);
}, { signal: events.signal });

window.addEventListener('pagehide', () => events.abort(), { once: true });
