const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const vm = require('node:vm');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const preferences = app.slice(app.indexOf('  const moduleNames ='), app.indexOf('  function now()'));
const routing = app.slice(app.indexOf('  function activeDropZone()'), app.indexOf('  async function boot()'));
const installing = app.slice(app.indexOf('  let luaInstalling ='), app.indexOf('  async function luaBrowseFiles()'));

function harness(options = {}) {
  const nodes = new Map(); const calls = []; const errors = []; const stored = new Map();
  let clock = 10000;
  if (options.stored !== undefined) stored.set('cgs-hidden-modules', options.stored);
  const node = (selector) => {
    if (!nodes.has(selector)) {
      const classes = new Set();
      nodes.set(selector, { dataset: {}, checked: true, disabled: false, closest() { return this; }, classList: {
        add(c) { classes.add(c); }, remove(c) { classes.delete(c); }, contains(c) { return classes.has(c); },
        toggle(c, on) { if (on) classes.add(c); else classes.delete(c); },
      } });
    }
    return nodes.get(selector);
  };
  const modules = ['fix', 'saves', 'dlc', 'lua', 'hypervisor', 'files', 'utilities'];
  const nav = [...modules, 'home', 'settings'].map(name => { const e = node(`nav-${name}`); e.dataset.view = name; return e; });
  const tiles = modules.map(name => { const e = node(`tile-${name}`); e.dataset.openView = name; return e; });
  const toggles = modules.map(name => { const e = node(`toggle-${name}`); e.dataset.moduleToggle = name; return e; });
  const active = node('[data-view-panel].active'); active.dataset.viewPanel = options.view || 'lua';
  function setView(name) {
    active.dataset.viewPanel = name;
    ['fix', 'dlc', 'lua'].forEach(view => node(`#view-${view}`).classList.toggle('active', view === name));
  }
  setView(active.dataset.viewPanel);
  node('#lua-auto-update').checked = true;
  const context = {
    $: node,
    $$: selector => selector === '[data-module-toggle]' ? toggles : [...nav, ...tiles],
    localStorage: { getItem: key => stored.get(key), setItem: (key, value) => stored.set(key, value) },
    state: { lua: { steamPath: 'E:\\Steam' } },
    Date: { now: () => clock }, Set, JSON,
    switchView: setView, log() {}, toast: message => errors.push(message), errorText: e => e.message,
    consumeResult: result => result.success,
    call: async (command, args) => { calls.push({ command, args }); return options.install ? options.install(command, args) : { success: false }; },
    luaRefresh: async () => {}, showModal: async () => {},
    scanDlcDirectory: async dir => calls.push({ command: 'scan-dlc', dir }),
    scanDirectory: async dir => calls.push({ command: 'scan-fix', dir }),
  };
  vm.createContext(context);
  vm.runInContext(preferences + installing + routing + '\nglobalThis.api = { handleFileDrop, setModuleVisible, applyModuleVisibility };', context);
  return { ...context.api, calls, errors, stored, node, setView, advance: ms => { clock += ms; } };
}

test('CrocLua accepts all ZIP/Lua paths and deduplicates simultaneous native/WebView drops', async () => {
  const h = harness();
  const paths = ['E:\\Мои игры\\манифест.zip', 'E:\\Downloads\\480.lua'];
  await h.handleFileDrop(paths); await h.handleFileDrop(paths);
  assert.equal(h.calls.length, 1);
  assert.equal(h.calls[0].command, 'luatools_install');
  assert.deepEqual(Array.from(h.calls[0].args.paths), paths);
  assert.equal(h.calls[0].args.steamPath, 'E:\\Steam');
  assert.equal(h.calls[0].args.autoUpdate, true);
  assert.equal(h.node('#lua-drop-zone').classList.contains('busy'), false);
});

test('concurrent drops cannot start a second CrocLua install', async () => {
  let release;
  const h = harness({ install: () => new Promise(resolve => { release = resolve; }) });
  const first = h.handleFileDrop(['E:\\one.zip']);
  await h.handleFileDrop(['E:\\two.zip']);
  assert.equal(h.calls.length, 1);
  assert.equal(h.node('#lua-drop-zone').classList.contains('busy'), true);
  release({ success: false }); await first;
  h.advance(1500); const second = h.handleFileDrop(['E:\\two.zip']);
  assert.equal(h.calls.length, 2); release({ success: false }); await second;
});

test('failed imports release the drop zone and can be retried', async () => {
  const h = harness({ install: async () => { throw new Error('Archive unavailable'); } });
  await h.handleFileDrop(['E:\\one.zip']);
  h.advance(1500); await h.handleFileDrop(['E:\\one.zip']);
  assert.equal(h.calls.length, 2); assert.equal(h.errors.length, 2);
  assert.equal(h.node('#lua-drop-zone').classList.contains('busy'), false);
});

test('DLC folder drops stay in DLC and unrelated tabs do not import files', async () => {
  const h = harness({ view: 'dlc' });
  await h.handleFileDrop(['E:\\Games\\Game']);
  assert.equal(h.calls[0].command, 'scan-dlc');
  h.setView('settings'); await h.handleFileDrop(['E:\\one.zip']);
  assert.equal(h.calls.length, 1);
});

test('visibility settings persist and hide both sidebar and home cards', () => {
  const h = harness({ view: 'settings' }); h.setModuleVisible('lua', false);
  assert(h.node('nav-lua').classList.contains('module-hidden'));
  assert(h.node('tile-lua').classList.contains('module-hidden'));
  assert.equal(h.node('toggle-lua').checked, false);
  const restored = harness({ view: 'settings', stored: h.stored.get('cgs-hidden-modules') });
  restored.applyModuleVisibility();
  assert(restored.node('nav-lua').classList.contains('module-hidden'));
  restored.setModuleVisible('lua', true);
  assert(!restored.node('nav-lua').classList.contains('module-hidden'));
  assert(!restored.node('tile-lua').classList.contains('module-hidden'));
});

test('Home and Settings remain reachable and invalid preferences are ignored', () => {
  const h = harness({ stored: '["home","settings","invalid","lua"]' });
  h.applyModuleVisibility();
  assert(!h.node('nav-home').classList.contains('module-hidden'));
  assert(!h.node('nav-settings').classList.contains('module-hidden'));
  assert.equal(h.node('[data-view-panel].active').dataset.viewPanel, 'home');
  const malformed = harness({ stored: 'not json' }); malformed.applyModuleVisibility();
  assert.equal(malformed.node('toggle-lua').checked, true);
});

test('dragging on Home does not reopen a hidden Online Fix module', async () => {
  const h = harness({ view: 'home', stored: '["fix"]' });
  await h.handleFileDrop(['E:\\Games\\Game']); assert.equal(h.calls.length, 0);
});
