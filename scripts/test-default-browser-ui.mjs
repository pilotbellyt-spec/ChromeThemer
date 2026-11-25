import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

const source = readFileSync(new URL('../ui/app.js', import.meta.url), 'utf8')
  .replace(/^import .*;\r?\n/gm, '');

for (const platform of ['windows', 'linux', 'macos']) {
  const elements = new Map();
  const element = selector => {
    if (!elements.has(selector)) elements.set(selector, {
      value: '', checked: false, hidden: true, dataset: {}, listeners: {},
      addEventListener(name, handler) { this.listeners[name] = handler; },
    });
    return elements.get(selector);
  };
  const calls = [];
  const context = vm.createContext({
    document: { querySelector: element, querySelectorAll: () => [...elements.values()] },
    localStorage: { getItem: () => null, setItem() {} },
    isTauri: () => true,
    invoke: async (command, args) => {
      calls.push({ command, args });
      return command === 'check_environment' ? platform : undefined;
    },
  });
  vm.runInContext(source, context);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(element('#make-default').hidden, platform !== 'windows');
  if (platform === 'windows') {
    element('#browser').value = 'C:\\Chrome 空\\chrome.exe';
    element('#frameless').checked = true;
    await element('#make-default').listeners.click();
    const call = calls.find(call => call.command === 'make_default_browser');
    assert.equal(call.args.executable, 'C:\\Chrome 空\\chrome.exe');
    assert.equal(call.args.experimentalFrameless, true);
    assert.match(element('#message').textContent, /Windows Settings/);
    assert.equal(element('#make-default').disabled, false);
  }
}
console.log('Default browser UI checks passed on Windows, Linux, and macOS.');
