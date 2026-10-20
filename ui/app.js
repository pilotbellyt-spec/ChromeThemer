import { invoke, isTauri } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';

const browser = document.querySelector('#browser');
const theme = document.querySelector('#theme');
const unsafeTheme = document.querySelector('#unsafe-theme');
const message = document.querySelector('#message');
const status = document.querySelector('#status');
const desktop = isTauri();
let busy = false;

browser.value = localStorage.getItem('chrome-executable') ?? '';
theme.value = localStorage.getItem('theme-folder') ?? '';
if (!desktop) say('Desktop app required.');

function say(text, state = '') {
  message.textContent = text;
  status.dataset.state = state;
}

function save() {
  localStorage.setItem('chrome-executable', browser.value);
  localStorage.setItem('theme-folder', theme.value);
}

async function run(command, args, success) {
  if (busy) return;
  if (!desktop) {
    say('Desktop app required.');
    return;
  }
  busy = true;
  for (const control of document.querySelectorAll('button, input')) control.disabled = true;
  say('Working…');
  try {
    await invoke(command, args);
    save();
    say(success, 'success');
  } catch (error) {
    say(String(error), 'error');
  } finally {
    busy = false;
    for (const control of document.querySelectorAll('button, input')) control.disabled = false;
  }
}

async function choose(input, directory) {
  if (!desktop) { say('Desktop app required.'); return; }
  try {
    const path = await open({ directory, multiple: false, title: directory ? 'Choose your theme folder' : 'Choose the Chrome executable' });
    if (typeof path !== 'string') return;
    input.value = path;
    save();
    if (!directory) {
      await run('inspect_browser', { executable: path }, 'Webium found.');
    }
  } catch (error) { say(String(error), 'error'); }
}

document.querySelector('#choose-browser').addEventListener('click', () => choose(browser, false));
document.querySelector('#choose-theme').addEventListener('click', () => choose(theme, true));
document.querySelector('#theme-form').addEventListener('submit', event => {
  event.preventDefault();
  run('apply_theme', { executable: browser.value.trim(), theme: theme.value.trim(), allowUnsafeThemeFiles: unsafeTheme.checked }, 'Theme applied. Restart Chrome.');
});
document.querySelector('#launch').addEventListener('click', () => run('launch_browser', { executable: browser.value.trim() }, 'Chrome launched.'));
document.querySelector('#restore').addEventListener('click', () => run('restore_browser', { executable: browser.value.trim() }, 'Original restored. Restart Chrome.'));
document.querySelector('#open-theme').addEventListener('click', () => run('open_theme', { theme: theme.value.trim() }, 'Theme folder opened.'));
browser.addEventListener('input', save);
theme.addEventListener('input', save);
