import { invoke, isTauri } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';

const browser = document.querySelector('#browser');
const theme = document.querySelector('#theme');
const frameless = document.querySelector('#frameless');
const unsafeTheme = document.querySelector('#unsafe-theme');
const message = document.querySelector('#message');
const status = document.querySelector('#status');
const desktop = isTauri();
let busy = false;
let environmentReady = !desktop;

browser.value = localStorage.getItem('chrome-executable') ?? '';
theme.value = localStorage.getItem('theme-folder') ?? '';
frameless.checked = localStorage.getItem('experimental-frameless') === 'true';
unsafeTheme.checked = localStorage.getItem('unsafe-theme') === 'true';
if (!desktop) say('Desktop app required.');

function say(text, state = '') {
  message.textContent = text;
  status.dataset.state = state;
}

function save() {
  localStorage.setItem('chrome-executable', browser.value);
  localStorage.setItem('theme-folder', theme.value);
  localStorage.setItem('experimental-frameless', frameless.checked);
  localStorage.setItem('unsafe-theme', unsafeTheme.checked);
}

async function run(command, args, success) {
  if (busy) return undefined;
  if (!desktop) {
    say('Desktop app required.');
    return undefined;
  }
  if (!environmentReady) {
    say('ChromeThemer must be run as administrator. Close it, then right-click ChromeThemer and choose "Run as administrator".', 'error');
    return undefined;
  }
  busy = true;
  for (const control of document.querySelectorAll('button, input')) control.disabled = true;
  say('Working…');
  try {
    const result = await invoke(command, args);
    save();
    const outcome = typeof success === 'function' ? success(result) : { text: success, state: 'success' };
    say(outcome.text, outcome.state ?? 'success');
    return result;
  } catch (error) {
    say(String(error), 'error');
    return undefined;
  } finally {
    busy = false;
    for (const control of document.querySelectorAll('button, input')) control.disabled = false;
  }
}

function operationMessage(result, action) {
  const failures = [];
  if (result.pak.status === 'failed') failures.push(`Chrome UI: ${result.pak.message}`);
  if (result.palette.status === 'failed') failures.push(`Chrome colors: ${result.palette.message}`);
  if (failures.length) {
    return {
      text: failures.join(' '),
      state: 'warning',
    };
  }
  return { text: `${action}.`, state: 'success' };
}

function launchMessage(result) {
  if (result.frame.status === 'unsupported') {
    return {
      text: `Chrome launched. Combined titlebar failed: ${result.frame.reason}`,
      state: 'warning',
    };
  }
  return { text: 'Chrome launched.', state: 'success' };
}

async function choose(input, directory, title) {
  if (!desktop) {
    say('Desktop app required.');
    return;
  }
  try {
    const path = await open({ directory, multiple: false, title });
    if (typeof path !== 'string') return;
    input.value = path;
    save();
    if (input === browser) {
      await run('inspect_browser', { executable: path }, 'Webium found.');
    }
  } catch (error) {
    say(String(error), 'error');
  }
}

document.querySelector('#choose-browser').addEventListener('click', () =>
  choose(browser, false, 'Choose the Chrome executable'));
document.querySelector('#choose-theme').addEventListener('click', () =>
  choose(theme, true, 'Choose your theme folder'));

document.querySelector('#theme-form').addEventListener('submit', event => {
  event.preventDefault();
  run('apply_theme', {
    executable: browser.value.trim(),
    theme: theme.value.trim(),
    allowUnsafeThemeFiles: unsafeTheme.checked,
  }, result => operationMessage(result, 'Theme applied'));
});

document.querySelector('#launch').addEventListener('click', () => run('launch_browser', {
  executable: browser.value.trim(),
  experimentalFrameless: frameless.checked,
}, launchMessage));

document.querySelector('#shortcut').addEventListener('click', () => run('create_shortcut', {
  executable: browser.value.trim(),
  experimentalFrameless: frameless.checked,
}, created => ({ text: created ? 'Shortcut created.' : 'Cancelled.' })));

document.querySelector('#restore').addEventListener('click', () => run('restore_browser', {
  executable: browser.value.trim(),
}, result => operationMessage(result, 'Original theme restored')));

document.querySelector('#open-theme').addEventListener('click', () =>
  run('open_theme', { theme: theme.value.trim() }, 'Theme folder opened.'));

for (const control of [browser, theme, frameless, unsafeTheme]) {
  control.addEventListener('input', save);
}

async function checkEnvironment() {
  if (!desktop) return;
  try {
    const platform = await invoke('check_environment');
    const supportsCombinedTitlebar = platform === 'windows' || platform === 'linux';
    document.querySelector('#titlebar-option').hidden = !supportsCombinedTitlebar;
    document.querySelector('#shortcut').hidden = !supportsCombinedTitlebar;
    if (!supportsCombinedTitlebar) frameless.checked = false;
    environmentReady = true;
  } catch (error) {
    say(String(error), 'error');
    for (const control of document.querySelectorAll('button, input')) control.disabled = true;
  }
}

checkEnvironment();
