const app = document.querySelector('webui-browser-app');
const tabStrip = app.shadowRoot.querySelector('webui-browser-tab-strip');
await tabStrip.updateComplete;
const strip = tabStrip.shadowRoot;
const events = new AbortController();
const listener = { signal: events.signal };

function updateFrame() {
  app.style.setProperty('--classic-device-pixel', `${1 / window.devicePixelRatio}px`);
  const maximized = Math.max(window.innerWidth, window.outerWidth) >= screen.availWidth &&
      Math.max(window.innerHeight, window.outerHeight) >= screen.availHeight;
  app.toggleAttribute('data-classic-maximized', maximized);
}

const address = app.shadowRoot.querySelector('#address');
const toolbar = app.shadowRoot.querySelector('#searchBar');
// M68 SquareInkDropRipple queues the release burst behind the press animation.
let heldRipple;
function rippleButton(event) {
  return event.composedPath().find(node => node instanceof Element && !node.hasAttribute('disabled') &&
    (node.localName === 'cr-icon-button' || (node.localName === 'cr-button' &&
      node.getRootNode().host?.localName === 'webui-browser-extension')));
}

// M68's LayerAnimationElement defaults to LINEAR. SquareInkDropRipple
// starts explicit sequences, bypassing the implicit animator's tween.
const hoverTimers = new WeakMap();
const latestPress = new WeakMap();

function pressRipple(button) {
  if (button.hasAttribute('data-classic-selected-ink')) return;
  if (heldRipple) finishRipple(false);
  clearTimeout(hoverTimers.get(button));
  const ink = button.getRipple();
  for (const animation of ink.getAnimations()) animation.cancel();
  delete ink.dataset.classicInk;
  getComputedStyle(ink).animationName;
  button.dataset.classicHighlightHidden = '';
  ink.dataset.classicInk = 'pending';
  const pending = ink.getAnimations().find(a => a.animationName === 'classic-ink-pending');
  heldRipple = { button, ink, pending };
  latestPress.set(button, heldRipple);
}

async function finishRipple(triggered) {
  const press = heldRipple;
  if (!press) return;
  heldRipple = undefined;
  const { button, ink, pending } = press;
  if (triggered) {
    await pending?.finished.catch(() => {});
    if (latestPress.get(button) !== press) return;
    ink.dataset.classicInk = 'triggered';
  } else {
    // HIDDEN preempts ACTION_PENDING at its current transform.
    const transform = getComputedStyle(ink).transform;
    ink.dataset.classicInk = 'hidden';
    const reduced = matchMedia('(prefers-reduced-motion: reduce)').matches;
    ink.animate([{ transform }, { transform: 'scale(.75)' }], {
      duration: reduced ? 0 : 200,
    });
    ink.animate([{ opacity: .175 }, { opacity: 0 }], reduced ? 0 : 150);
  }
  await Promise.allSettled(ink.getAnimations().map(a => a.finished));
  if (latestPress.get(button) !== press) return;
  delete ink.dataset.classicInk;
  hoverTimers.set(button, setTimeout(() => {
    delete button.dataset.classicHighlightHidden;
  }, 1000));
}

// SquareInkDropRipple::ACTIVATED keeps the small rounded rectangle until
// the native anchor releases its highlight. DEACTIVATED expands it over
// 200 ms and starts its 150 ms fade after the first 50 ms.
async function selectRipple(button, selected) {
  if (button.hasAttribute('data-classic-selected-ink') === selected) return;
  button.toggleAttribute('data-classic-selected-ink', selected);
  const ink = button.getRipple();
  const token = {};
  latestPress.set(button, token);
  if (heldRipple?.button === button) heldRipple = undefined;
  clearTimeout(hoverTimers.get(button));
  button.dataset.classicHighlightHidden = '';
  const duration = ms => matchMedia('(prefers-reduced-motion: reduce)').matches ? 0 : ms;

  if (selected && (!ink.dataset.classicInk || ink.dataset.classicInk === 'pending')) {
    if (!ink.dataset.classicInk) ink.dataset.classicInk = 'activating';
    await Promise.allSettled(ink.getAnimations().map(a => a.finished));
    if (latestPress.get(button) !== token) return;
  }

  const style = getComputedStyle(ink);
  // Aura interpolates the corner circles' radii and their offsets separately.
  // An inset rounded rectangle describes their union in unscaled DIP space.
  let fromClip = style.clipPath;
  if (fromClip === 'none') {
    const radius = ink.getBoundingClientRect().width / 2;
    fromClip = `inset(${16 - radius}px round ${radius}px)`;
  }
  const opacity = style.opacity;
  for (const a of ink.getAnimations()) a.cancel();
  ink.dataset.classicInk = selected ? 'activated' : 'deactivated';
  const shape = ink.animate([{ clipPath: fromClip }, { clipPath: style.clipPath }],
    duration(selected ? 160 : 200));
  if (!selected) {
    ink.animate([{ opacity }, { opacity: 0 }], {
      duration: duration(150), delay: duration(50), fill: 'backwards',
    });
  }
  await shape.finished.catch(() => {});
  if (selected || latestPress.get(button) !== token) return;
  delete ink.dataset.classicInk;
  hoverTimers.set(button, setTimeout(() => delete button.dataset.classicHighlightHidden, 1000));
}

const {TrackedElementManager} = await import(
  'chrome://resources/js/tracked_element/tracked_element_manager.js');
const {TrackedElementProxyImpl} = await import(
  'chrome://resources/js/tracked_element/tracked_element_proxy.js');
const tracker = TrackedElementManager.getInstance();
const router = TrackedElementProxyImpl.getInstance().callbackRouter;
for (const button of toolbar.querySelectorAll('cr-icon-button[data-native-id], cr-button[data-native-id]')) {
  const {nativeId, secondaryId} = button.dataset;
  tracker.startTracking(button, nativeId, {
    secondaryId, onHighlightChanged: selected => selectRipple(button, selected),
  });
}
// Extension anchors already have a highlight callback that suppresses popup
// re-opening. Subscribe alongside it, including for newly pinned extensions.
const extensionHighlight = router.onElementHighlightChanged.addListener((id, selected) => {
  const anchor = tracker.getElementWithId(id)?.element;
  if (anchor?.localName === 'webui-browser-extension') {
    const button = anchor.shadowRoot.querySelector('cr-button');
    if (button) selectRipple(button, selected);
  } else if (anchor?.id === 'extensionsMenuButton') {
    selectRipple(anchor, selected);
  }
});

const panel = app.shadowRoot.querySelector('#sidePanel');
const bookmarks = toolbar.querySelector('#bookmarksButton');
const panelSelection = new MutationObserver(() => selectRipple(bookmarks,
  !!panel.shadowRoot.querySelector('webui-browser-bookmarks')));
panelSelection.observe(panel.shadowRoot, { childList: true, subtree: true });

const toggleSelection = new MutationObserver(records => {
  for (const {target} of records) {
    if (target.matches('cr-icon-button, cr-button')) {
      selectRipple(target, target.matches('[aria-pressed="true"], [aria-expanded="true"]'));
    }
  }
});
toggleSelection.observe(toolbar, {
  attributes: true, subtree: true, attributeFilter: ['aria-pressed', 'aria-expanded'],
});

// WebUIBrowserPageHandler::OpenAppMenu passes base::DoNothing() as its
// close callback. The native menu takes input until it dismisses. Resume
// the anchor's animation when input is delivered to this window again.
let nativeMenuAnchor;
function holdNativeMenu(button) {
  nativeMenuAnchor = button;
  selectRipple(button, true);
}
function releaseNativeMenu(event) {
  if (!event.isTrusted || !nativeMenuAnchor) return;
  if (event.type === 'keyup' && event.key !== 'Escape') return;
  selectRipple(nativeMenuAnchor, false);
  nativeMenuAnchor = undefined;
}
for (const type of ['pointermove', 'pointerdown', 'keydown', 'keyup', 'blur']) {
  window.addEventListener(type, releaseNativeMenu, { ...listener, capture: true });
}

toolbar.addEventListener('pointerdown', event => {
  const button = rippleButton(event);
  if (!button || event.button !== 0 || event.pointerType === 'touch') return;
  if (button.id === 'appMenuButton') {
    selectRipple(button, true);
    const release = e => {
      if (e.pointerId !== event.pointerId) return;
      window.removeEventListener('pointerup', release, true);
      window.removeEventListener('pointercancel', release, true);
      if (e.type === 'pointercancel' || rippleButton(e) !== button) selectRipple(button, false);
    };
    window.addEventListener('pointerup', release, { ...listener, capture: true });
    window.addEventListener('pointercancel', release, { ...listener, capture: true });
  } else {
    pressRipple(button);
  }
}, { ...listener, capture: true });
window.addEventListener('pointerup', event => {
  if (event.button === 0) finishRipple(rippleButton(event) === heldRipple?.button);
}, { ...listener, capture: true });
window.addEventListener('pointercancel', () => finishRipple(false), listener);
window.addEventListener('blur', () => finishRipple(false), listener);
toolbar.addEventListener('keydown', event => {
  const button = rippleButton(event);
  if (button && event.key === ' ' && !event.repeat) pressRipple(button);
}, { ...listener, capture: true });
toolbar.addEventListener('pointerover', event => {
  const button = rippleButton(event);
  if (button?.hasAttribute('data-classic-highlight-hidden') && !button.getRipple().dataset.classicInk) {
    clearTimeout(hoverTimers.get(button));
    delete button.dataset.classicHighlightHidden;
  }
}, listener);
toolbar.addEventListener('click', event => {
  const button = rippleButton(event);
  if (button?.id === 'appMenuButton') {
    holdNativeMenu(button);
    return;
  }
  if (button && event.detail === 0) {
    if (!heldRipple) pressRipple(button);
    finishRipple(true);
  }
}, { ...listener, capture: true });

const popupBounds = new ResizeObserver(() => {
  const field = address.getBoundingClientRect();
  const bar = toolbar.getBoundingClientRect();
  const rtl = getComputedStyle(toolbar).direction === 'rtl';
  address.style.setProperty('--classic-popup-start', `${rtl ? bar.right - field.right : field.left - bar.left}px`);
  address.style.setProperty('--classic-popup-end', `${rtl ? field.left - bar.left : bar.right - field.right}px`);
});
popupBounds.observe(address);
popupBounds.observe(toolbar);

// M158 components/tabs/public/mojom/tabs.mojom values in M68 tab_utils.cc priority order.
const alerts = [
  { states: [6, 16], icon: 'capture', label: 'Sharing screen' },
  { states: [2, 13, 18], icon: 'record', label: 'Recording' },
  { states: [4], icon: 'bluetooth', label: 'Bluetooth connected' },
  { states: [17], icon: 'usb', label: 'USB connected' },
  { states: [14], icon: 'pip', label: 'Picture in picture' },
  { states: [1], icon: 'muted', label: 'Audio muted' },
  { states: [3], icon: 'audio', label: 'Playing audio' },
];

function updateAlert(tab) {
  const alert = alerts.find(({ states }) => states.some(state => tab.tabData.alertStates.includes(state)));
  let badge = tab.shadowRoot?.querySelector('.classic-alert');
  if (!alert) {
    badge?.remove();
    tab.removeAttribute('data-classic-alert');
    return;
  }
  const close = tab.shadowRoot?.querySelector('.close');
  if (!close) return;
  if (!badge) {
    badge = document.createElement('span');
    badge.className = 'classic-alert';
    badge.setAttribute('role', 'img');
    close.before(badge);
  }
  tab.dataset.classicAlert = alert.icon;
  badge.setAttribute('aria-label', alert.label);
}

async function updateTabs() {
  // Webium dispatches tab-updated before Lit publishes the new tabData to its children.
  await tabStrip.updateComplete;
  let selectedCount = 0;
  for (const tab of strip.querySelectorAll('webui-browser-tab')) {
    await tab.updateComplete;
    const data = tab.tabData;
    updateAlert(tab);
    tab.toggleAttribute('data-classic-selected', !!data?.isSelected);
    if (data?.isSelected || data?.isActive) selectedCount++;
  }
  tabStrip.toggleAttribute('data-classic-multiselect', selectedCount > 1);
  updateTabVisibility();
}

for (const type of ['tab-added', 'tab-updated', 'tab-closed']) {
  tabStrip.addEventListener(type, updateTabs, listener);
}
updateTabs();
updateFrame();
window.addEventListener('resize', updateFrame, listener);

// M68 TabStrip::AnimateToIdealBounds uses BoundsAnimator's 200 ms EASE_OUT.
// RectValueBetween rounds each interpolated position and size to a whole DIP.
const tabRow = strip.querySelector('#tabstrip');
const newTabButton = strip.querySelector('#newTabButton');
const reducedMotion = matchMedia('(prefers-reduced-motion: reduce)');
const boundsDuration = 200;
const tabUpdate = tabStrip.update;
let movingTabs = [];
let boundsFrame;
let layoutVersion = 0;

// M68 TabStrip::ShouldTabBeVisible reserves the new-tab button's width
// minus its 5-DIP overlap. A tab before the active one must still fit if
// selected, when its minimum width increases from 32 to 48 DIPs.
function updateTabVisibility() {
  const tabs = Array.from(tabRow.querySelectorAll(':scope > webui-browser-tab'));
  const activeIndex = tabs.findIndex(tab => tab.active);
  const row = tabRow.getBoundingClientRect();
  const activeWidth = tabs[activeIndex]?.getBoundingClientRect().width ?? 0;
  const available = row.width - newTabButton.getBoundingClientRect().width + 5;
  const rtl = getComputedStyle(tabRow).direction === 'rtl';
  const clipped = tabs.map((tab, index) => {
    const rect = tab.getBoundingClientRect();
    const end = rtl ? row.right - rect.left : rect.right - row.left;
    const reserve = index < activeIndex ? Math.max(0, activeWidth - rect.width) : 0;
    // Flex positions and widths can both end in half a DIP. Round the
    // resulting edge once, as M68 does with its integer tab bounds.
    return Math.round(end + reserve) > Math.round(available);
  });
  tabs.forEach((tab, index) => tab.toggleAttribute('data-classic-clipped', clipped[index]));
}
const stripSize = new ResizeObserver(updateTabVisibility);
stripSize.observe(tabRow);

function tabBounds() {
  return new Map(Array.from(tabRow.children)
    .filter(el => el === newTabButton || el.matches('webui-browser-tab, webui-browser-tab-group'))
    .map(el => [el, el.getBoundingClientRect()]));
}

function stopTabMotion() {
  layoutVersion++;
  cancelAnimationFrame(boundsFrame);
  for (const {el} of movingTabs) {
    el.removeAttribute('data-classic-bounds');
    for (const prop of ['x', 'width', 'margin']) el.style.removeProperty('--classic-motion-' + prop);
  }
  movingTabs = [];
}

function retireTab(el, rect, row, rtl, started) {
  // Lit has already released this node. Keep its last rendered contents out
  // of Webium's tab queries and hit testing until the close animation ends.
  const shell = document.createElement('div');
  shell.className = 'classic-closing-tab';
  shell.inert = true;
  shell.style.insetInlineStart = (rtl ? row.right - rect.right : rect.left - row.left) + 'px';
  shell.style.width = rect.width + 'px';
  shell.attachShadow({ mode: 'open' }).append(el);
  el.active = false;
  el.setAttribute('data-classic-closing', '');
  tabRow.append(shell);
  function collapse(now) {
    if (!shell.isConnected) return;
    const remaining = remainingBounds(now, started);
    if (!remaining || reducedMotion.matches) return shell.remove();
    shell.style.width = Math.round(rect.width * remaining) + 'px';
    requestAnimationFrame(collapse);
  }
  requestAnimationFrame(collapse);
}

function remainingBounds(now, started) {
  return (1 - Math.min(1, Math.max(0, (now - started) / boundsDuration))) ** 2;
}

function animateTabBounds(before) {
  const after = tabBounds();
  const row = tabRow.getBoundingClientRect();
  const rtl = getComputedStyle(tabRow).direction === 'rtl';
  const started = performance.now();
  let previous;
  for (const [el, target] of after) {
    const start = before.get(el);
    const width = start?.width ?? 0;
    const leading = rtl ? 'right' : 'left';
    const origin = start?.[leading] ?? (previous
      ? rtl ? previous.left + 16 : previous.right - 16 : target[leading]);
    previous = start ?? target;
    if (el.tabData && el.tabData.id === tabStrip.dragDelegate_.session_?.draggedTabId) continue;
    if (origin === target[leading] && width === target.width) continue;
    movingTabs.push({ el, width, target, offset: origin - target[leading],
      margin: parseFloat(getComputedStyle(el).marginInlineEnd) || 0 });
    el.setAttribute('data-classic-bounds', '');
  }
  for (const [el, rect] of before) {
    if (!after.has(el) && el.localName === 'webui-browser-tab' && !el.isConnected) {
      retireTab(el, rect, row, rtl, started);
    }
  }
  if (!movingTabs.length) return;
  function paint(now) {
    const remaining = remainingBounds(now, started);
    if (remaining === 0) {
      stopTabMotion();
      updateTabVisibility();
      return;
    }
    for (const {el, width, target, offset, margin} of movingTabs) {
      const w = Math.round(target.width + (width - target.width) * remaining);
      el.style.setProperty('--classic-motion-x', Math.round(offset * remaining) + 'px');
      if (!el.tabData) continue;
      el.style.setProperty('--classic-motion-width', w + 'px');
      el.style.setProperty('--classic-motion-margin', (margin + target.width - w) + 'px');
    }
    boundsFrame = requestAnimationFrame(paint);
  }
  paint(started);
}

let mouseCloseId;
strip.addEventListener('pointerdown', event => {
  const path = event.composedPath();
  const closing = event.button === 1 || (event.button === 0 && path.some(el => el.classList?.contains('close')));
  mouseCloseId = closing ? path.find(el => el.tabData)?.tabData.id : undefined;
}, { ...listener, capture: true });

function releaseCloseWidth() {
  if (!tabRow.style.getPropertyValue('--classic-max-tab-width')) return;
  const before = tabBounds();
  stopTabMotion();
  tabRow.style.removeProperty('--classic-max-tab-width');
  if (!reducedMotion.matches) animateTabBounds(before);
}
window.addEventListener('pointermove', event => {
  if (!tabRow.style.getPropertyValue('--classic-max-tab-width')) return;
  const r = tabStrip.getBoundingClientRect();
  if (event.clientX < r.left || event.clientX >= r.right || event.clientY < r.top || event.clientY >= r.bottom + 40) {
    releaseCloseWidth();
  }
}, listener);
window.addEventListener('pointerout', event => {
  if (!event.relatedTarget) releaseCloseWidth();
}, listener);
// The embedded page receives its own pointer events below the toolbar.
app.shadowRoot.querySelector('#topContainer').addEventListener('pointerleave', releaseCloseWidth, listener);
window.addEventListener('blur', releaseCloseWidth, listener);

let dragReleaseBounds;
tabStrip.update = function(changes) {
  const old = changes.get('items_');
  const items = this.items_;
  // Webium swaps entries in the existing items array during a drag. Lit's
  // old value already contains the swap, but the row still has its old order.
  const changedOrder = old?.length && (old.length !== items.length ||
    items.some((item, i) => {
      const el = tabRow.children[i];
      return (el?.tabData?.id ?? el?.collectionId) !== item.id;
    }));
  const release = dragReleaseBounds;
  dragReleaseBounds = undefined;
  if ((!changedOrder && !release) || document.hidden || reducedMotion.matches) return tabUpdate.call(this, changes);
  const before = release ?? tabBounds();
  stopTabMotion();
  if (changedOrder) {
    const closingIndex = old.findIndex(item => item.id === mouseCloseId);
    if (closingIndex >= 0 && closingIndex < old.length - 1 && !items.some(item => item.id === mouseCloseId)) {
      const closing = this.getTabElement_(mouseCloseId);
      if (closing) tabRow.style.setProperty('--classic-max-tab-width', closing.getBoundingClientRect().width + 'px');
    } else {
      tabRow.style.removeProperty('--classic-max-tab-width');
    }
    mouseCloseId = undefined;
  }
  const version = layoutVersion;
  tabUpdate.call(this, changes);
  // Newly inserted Lit tabs install their shadow styles after the parent
  // renders. Measure their themed width before the next frame is painted.
  Promise.all(Array.from(tabRow.children, el => el.updateComplete)).then(() => {
    if (version === layoutVersion) animateTabBounds(before);
  });
};
window.addEventListener('resize', stopTabMotion, listener);
reducedMotion.addEventListener('change', stopTabMotion, listener);

// Webium enters its native drag session on mouse-down, even for tab selection.
// M68 TabDragController::CanStartDrag waits for movement beyond 10 DIPs.
let tabPress;
let dragOrigin;
strip.addEventListener('mousedown', event => {
  const path = event.composedPath();
  const tab = path.find(node => node.localName === 'webui-browser-tab');
  tabPress = event.button === 0 && tab && !path.some(node => node.classList?.contains('close'))
    ? { id: tab.tabData.id, x: event.clientX, y: event.clientY } : undefined;
}, { ...listener, capture: true });

function updateDragVisibility(point) {
  if (!dragOrigin || Math.hypot(point.x - dragOrigin.x, point.y - dragOrigin.y) > 10) {
    tabStrip.setAttribute('data-classic-tab-moved', '');
  }
}

// The Mojo receiver binds the strip's entry points before the theme loads.
// Those bound methods look up the delegate on each native drag update.
const dragDelegate = tabStrip.dragDelegate_;
const onDragEntered = dragDelegate.onMojoDragEntered;
const onDrag = dragDelegate.onMojoDrag;
const clearDragState = dragDelegate.clearDragState_;
dragDelegate.clearDragState_ = function() {
  if (this.session_ && tabStrip.hasAttribute('data-classic-tab-moved')) {
    dragReleaseBounds = tabBounds();
  }
  return clearDragState.call(this);
};
dragDelegate.onMojoDragEntered = function(id, point, ratio) {
  dragOrigin = tabPress?.id === id ? tabPress : undefined;
  tabPress = undefined;
  tabStrip.removeAttribute('data-classic-tab-moved');
  updateDragVisibility(point);
  return onDragEntered.call(this, id, point, ratio);
};
dragDelegate.onMojoDrag = function(point) {
  updateDragVisibility(point);
  return onDrag.call(this, point);
};

strip.addEventListener('pointermove', event => {
  const tab = event.composedPath().find(node =>
    node instanceof Element && node.localName === 'webui-browser-tab');
  if (!tab) return;
  const bounds = tab.getBoundingClientRect();
  tab.style.setProperty('--classic-hover-x', `${event.clientX - bounds.left}px`);
  tab.style.setProperty('--classic-hover-y', `${event.clientY - bounds.top}px`);
}, listener);

window.addEventListener('pagehide', () => {
  events.abort();
  stopTabMotion();
  tabStrip.update = tabUpdate;
  dragDelegate.onMojoDragEntered = onDragEntered;
  dragDelegate.onMojoDrag = onDrag;
  dragDelegate.clearDragState_ = clearDragState;
  popupBounds.disconnect();
  stripSize.disconnect();
  panelSelection.disconnect();
  toggleSelection.disconnect();
  router.removeListener(extensionHighlight);
}, { once: true });
