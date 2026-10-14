const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const vm = require('node:vm');

const app = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const functions = app.slice(app.indexOf('  function dlcExe()'), app.indexOf('  function steamHeaderUrls('));
const game = { appid: 10, name: 'Example Game', gameDir: 'E:\\SteamLibrary\\steamapps\\common\\Game', headerImage: '' };
const exe = { path: game.gameDir + '\\Game\\Binaries\\Win64\\Game-Win64-Shipping.exe', relativePath: 'Game\\Binaries\\Win64\\Game-Win64-Shipping.exe', bitness: 'x64', recommended: true, proxy: null, configuredDlcs: {} };
const scan = { ...game, detected: true, targets: [{ dir: '.', api: 'steam_api64.dll', bitness: 'x64' }], installed: false, exes: [exe], configuredDlcs: [], unlockAll: false };
const catalog = { entries: [{ id: '11', name: 'Pack One' }, { id: '12', name: 'Pack Two' }], sources: ['Steam Store'], warnings: [], online: true };

function harness(options = {}) {
  const nodes = new Map(); const calls = []; const modals = [];
  function element() {
    const classes = new Set(); const queries = new Map();
    return {
      textContent: '', innerHTML: '', value: '', checked: false, disabled: false, dataset: {}, children: [], listeners: {},
      isConnected: true, removed: false,
      classList: { add: c => classes.add(c), remove: c => classes.delete(c), contains: c => classes.has(c), toggle(c, enabled) { if (enabled ?? !classes.has(c)) classes.add(c); else classes.delete(c); } },
      append(...children) { this.children.push(...children); }, appendChild(child) { this.children.push(child); }, replaceChildren(...children) { this.children = children; },
      remove() { this.removed = true; this.isConnected = false; },
      setAttribute(name, value) { this[name] = value; }, removeAttribute(name) { delete this[name]; },
      addEventListener(name, fn) { this.listeners[name] = fn; },
      querySelector(selector) { if (!queries.has(selector)) queries.set(selector, element()); return queries.get(selector); },
      get parentElement() { return node('parent'); },
    };
  }
  function node(selector) { if (!nodes.has(selector)) nodes.set(selector, element()); return nodes.get(selector); }
  node('#dlc-unlock-all').checked = true;
  const state = { lua: { steamPath: '' }, dlc: { gameDir: '', appid: null, name: '', banner: '', source: '', targets: [], installed: false, catalog: [], loaded: false, scanning: false, games: [], libraryScanned: false, libraryScanning: false, extraLibrary: '', detail: false, method: 'cream', exes: [], exePath: '', proxy: 'version.dll', requestId: 0, catalogLoading: false, catalogError: '', catalogStatus: '', operating: false, configuredDlcs: [], configUnlockAll: true, coverObjectUrls: [] } };
  let scanResult = options.scan || scan;
  const context = {
    state, document: { createElement: element }, window: {}, localStorage: { setItem() {} },
    URL: { createObjectURL: () => 'blob:steam-cover', revokeObjectURL() {} }, Blob: class Blob {},
    $: node, $$: () => [], basename: value => value.split(/[\\/]/).pop(), errorText: error => error.message || String(error), log() {}, toast() {},
    setBusy(button, busy) { button.disabled = busy; }, consumeResult: result => result.success,
    showModal: async (...args) => { modals.push(args); return options.confirm ? options.confirm(...args) : options.cancel !== true; },
    call: async (command, args) => {
      calls.push({ command, args });
      if (command === 'scan_installed_steam_games') return { games: [game], libraries: ['E:\\SteamLibrary'], warnings: [] };
      if (command === 'steam_library_cover') return { bytes: [255, 216, 255], mime: 'image/jpeg' };
      if (command === 'scan_dlc_target') return structuredClone(scanResult);
      if (command === 'scan_dlc_catalog') { if (options.catalogError) throw new Error(options.catalogError); return options.catalog ? options.catalog() : catalog; }
      if (command === 'install_smaki') { scanResult = { ...scan, exes: [{ ...exe, proxy: args.request.proxy, configuredDlcs: args.request.dlcs, unlockAll: args.request.unlockAll }] }; return { success: true, message: 'ok', logs: [] }; }
      if (command === 'uninstall_smaki') { scanResult = scan; return { success: true, message: 'ok', logs: [] }; }
      throw new Error('Unexpected command: ' + command);
    },
  };
  vm.createContext(context);
  vm.runInContext(functions + '\n globalThis.api = { scanDlcLibrary, renderDlcLibrary, scanDlcDirectory, renderDlcMethod, loadDlcCatalog, updateDlcSelection, installDlcUnlocker, uninstallDlcUnlocker };', context);
  return { ...context.api, node, state, calls, modals };
}

test('library scan shows only backend-confirmed installed games and supports filtering', async () => {
  const view = harness(); await view.scanDlcLibrary();
  assert.equal(view.state.dlc.games.length, 1); assert.equal(view.node('#dlc-game-list').children.length, 1);
  view.node('#dlc-library-search').value = 'not installed'; view.renderDlcLibrary();
  assert.equal(view.node('#dlc-game-list').children.length, 0); assert.equal(view.node('#dlc-library-count').textContent, 1);
});

test('failed Store cover falls back to the local Steam library cache', async () => {
  const view = harness(); await view.scanDlcLibrary();
  const card = view.node('#dlc-game-list').children[0];
  const image = card.children[0].children[1];
  await image.onerror();
  assert.equal(image.src, 'blob:steam-cover'); assert.equal(image.removed, false);
  assert(view.calls.some(call => call.command === 'steam_library_cover' && call.args.appid === 10));
});

test('Smoke proxy destination is the selected nested EXE directory and x86 is enabled', async () => {
  const view = harness(); await view.scanDlcDirectory(game.gameDir);
  view.state.dlc.method = 'smaki'; view.state.dlc.proxy = 'winhttp.dll'; view.renderDlcMethod();
  assert.equal(view.node('#dlc-proxy-destination').textContent, exe.path.replace(/[^\\/]+$/, 'winhttp.dll'));
  assert.equal(view.node('#dlc-install-button').disabled, false);
  view.state.dlc.exes[0] = { ...exe, bitness: 'x86' }; view.renderDlcMethod();
  assert.equal(view.node('#dlc-install-button').disabled, false); assert.match(view.node('#dlc-smaki-note').textContent, /x86/);
});

test('existing selective installation survives catalog loading and manual selection survives retry', async () => {
  const view = harness({ scan: { ...scan, installed: true, configuredDlcs: [{ id: '11', name: 'Pack One' }], unlockAll: false } });
  await view.scanDlcDirectory(game.gameDir);
  assert.equal(view.node('#dlc-unlock-all').checked, false);
  assert.equal(view.state.dlc.catalog.find(item => item.id === '11').enabled, true);
  assert.equal(view.state.dlc.catalog.find(item => item.id === '12').enabled, false);
  view.state.dlc.catalog.find(item => item.id === '12').enabled = true;
  await view.loadDlcCatalog(10);
  assert.equal(view.state.dlc.catalog.find(item => item.id === '12').enabled, true);
});

test('network failure reports an unavailable catalog instead of claiming no DLC', async () => {
  const view = harness({ catalogError: 'Steam unavailable' }); await view.scanDlcDirectory(game.gameDir);
  assert.equal(view.state.dlc.loaded, false); assert.equal(view.node('#dlc-empty h3').textContent, 'Каталог недоступен');
  assert.match(view.node('#dlc-empty p').textContent, /ошибка сети/); assert.equal(view.node('#dlc-catalog-retry').disabled, false);
});

test('outdated catalog result cannot overwrite the next selected game', async () => {
  let finish;
  const view = harness({ catalog: () => new Promise(resolve => { finish = resolve; }) });
  const pending = view.loadDlcCatalog(10); ++view.state.dlc.requestId;
  view.state.dlc.catalog = [{ id: '99', name: 'Different Game DLC', enabled: true }];
  finish(catalog); await pending; assert.equal(view.state.dlc.catalog[0].id, '99');
});

test('Smoke install calls its own command with the EXE and chosen proxy, never CreamAPI', async () => {
  const view = harness(); await view.scanDlcDirectory(game.gameDir); view.state.dlc.method = 'smaki'; view.state.dlc.proxy = 'winmm.dll'; view.renderDlcMethod();
  await view.installDlcUnlocker();
  const command = view.calls.find(call => call.command === 'install_smaki');
  assert.equal(command.args.request.exePath, exe.path); assert.equal(command.args.request.proxy, 'winmm.dll'); assert.equal(command.args.request.appid, 10);
  assert(!view.calls.some(call => call.command === 'install_dlc_unlocker')); assert.equal(view.state.dlc.operating, false);
  await view.uninstallDlcUnlocker(); assert(view.calls.some(call => call.command === 'uninstall_smaki'));
});

test('cancel and repeated clicks during confirmation do not install anything', async () => {
  let accept;
  const view = harness({ confirm: () => new Promise(resolve => { accept = resolve; }) });
  await view.scanDlcDirectory(game.gameDir); view.state.dlc.method = 'smaki';
  const first = view.installDlcUnlocker(); await view.installDlcUnlocker(); assert.equal(view.modals.length, 1);
  accept(false); await first;
  assert(!view.calls.some(call => call.command.startsWith('install_'))); assert.equal(view.state.dlc.operating, false);
});
