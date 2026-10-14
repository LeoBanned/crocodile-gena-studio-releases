const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const vm = require('node:vm');

const source = fs.readFileSync(path.join(__dirname, '../ui/vbs.js'), 'utf8');

async function createView(initialReport = {}, options = {}) {
  const nodes = new Map();
  const calls = [];
  const modals = [];
  let report = {
    components: [], warnings: [], canRestore: false,
    testSigningConfigured: false, testSigningActive: false,
    dse: { state: 'enabled', label: 'Включена' }, ...initialReport,
  };
  function node(selector) {
    if (!nodes.has(selector)) {
      const classes = new Set(selector.includes('tab-panel') ? ['active'] : []);
      nodes.set(selector, {
        textContent: '', innerHTML: '', dataset: {}, disabled: false,
        listeners: {},
        classList: {
          contains: name => classes.has(name),
          remove: name => classes.delete(name),
          toggle: (name, enabled) => enabled ? classes.add(name) : classes.delete(name),
        },
        addEventListener(name, handler) { this.listeners[name] = handler; },
      });
    }
    return nodes.get(selector);
  }
  const window = {
    __TAURI__: { core: { invoke: async (command, args) => {
      calls.push({ command, args });
      if (command === 'vbs_scan') return report;
      if (command === 'vbs_set_test_mode') {
        if (options.error) throw new Error(options.error);
        report = { ...report, testSigningConfigured: args.enabled, canRestore: args.enabled };
        return { success: true, changed: 12, restartRequired: true, failed: [] };
      }
      if (command === 'vbs_disable' || command === 'vbs_restore') {
        return { success: true, changed: 12, restartRequired: true, failed: [] };
      }
      throw new Error(`Unexpected command: ${command}`);
    } } },
    cgsShowModal: async (title, message, settings) => {
      modals.push({ title, message, settings });
      if (title === 'Требуется перезапуск') return false;
      if (options.confirm) return options.confirm(title);
      return options.cancel !== true;
    },
  };
  const document = { querySelector: node, addEventListener() {} };
  vm.runInNewContext(source, { window, document, Date, console });
  await new Promise(resolve => setImmediate(resolve));
  return {
    node, calls, modals,
    click: selector => node(selector).listeners.click(),
    setReport(value) { report = { ...report, ...value }; },
  };
}

test('one button enables test mode and cancels it before reboot', async () => {
  const view = await createView();
  assert.equal(view.node('#vbs-test-label').textContent, 'Включить тестовый');
  await view.click('#vbs-test-button');
  assert.equal(view.node('#vbs-test-label').textContent, 'Выключить тестовый');
  assert.equal(view.node('#vbs-test-status').textContent, 'Тестовый режим включится после перезагрузки.');
  await view.click('#vbs-test-button');
  assert.equal(view.node('#vbs-test-label').textContent, 'Включить тестовый');
  const actions = view.calls.filter(call => call.command === 'vbs_set_test_mode');
  assert.deepEqual(actions.map(call => call.args.enabled), [true, false]);
  assert(!view.calls.some(call => call.command === 'vbs_disable' || call.command === 'vbs_restart'));
  assert(!view.modals.filter(modal => modal.title === 'Требуется перезапуск').some(modal => modal.message.includes('дополнительные параметры')));
});

test('running test mode shows pending OFF until the normal reboot', async () => {
  const view = await createView({
    testSigningConfigured: true, testSigningActive: true, canRestore: true,
    dse: { state: 'testsigning', label: 'Test Signing' },
  });
  assert.equal(view.node('#vbs-dse-badge').textContent, 'TEST MODE');
  await view.click('#vbs-test-button');
  assert.equal(view.node('#vbs-test-label').textContent, 'Включить тестовый');
  assert.equal(view.node('#vbs-test-status').textContent, 'Тестовый режим выключится после перезагрузки.');
});

test('Secure Boot rejection keeps the enable action available', async () => {
  const view = await createView({}, { error: 'Отключите Secure Boot в BIOS/UEFI' });
  await view.click('#vbs-test-button');
  assert.equal(view.node('#vbs-test-label').textContent, 'Включить тестовый');
  assert.equal(view.node('#vbs-test-button').disabled, false);
  assert(view.modals.some(modal => modal.title === 'Ошибка VBS' && modal.message.includes('Secure Boot')));
});

test('cancelled confirmation does not change boot settings', async () => {
  const view = await createView({}, { cancel: true });
  await view.click('#vbs-test-button');
  assert(!view.calls.some(call => call.command === 'vbs_set_test_mode'));
  assert.equal(view.node('#vbs-test-button').disabled, false);
});

test('a second click during confirmation cannot submit another change', async () => {
  let accept;
  const view = await createView({}, { confirm: () => new Promise(resolve => { accept = resolve; }) });
  const first = view.click('#vbs-test-button');
  await view.click('#vbs-test-button');
  assert.equal(view.modals.length, 1);
  accept(false);
  await first;
  assert(!view.calls.some(call => call.command === 'vbs_set_test_mode'));
});
