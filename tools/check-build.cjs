// Read-only preflight: does not install tools, modify Windows, or touch game files.
const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const root = path.resolve(__dirname, '..');
let failures = 0;
function fail(message) { failures++; console.error(`[FAIL] ${message}`); }
function ok(message) { console.log(`[OK] ${message}`); }
function walk(dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap(item => {
    const file = path.join(dir, item.name);
    if (item.isSymbolicLink() || ['node_modules', 'target', 'gen'].includes(item.name)) return [];
    return item.isDirectory() ? walk(file) : [file];
  });
}
const pkg = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
const lock = JSON.parse(fs.readFileSync(path.join(root, 'package-lock.json'), 'utf8'));
const config = JSON.parse(fs.readFileSync(path.join(root, 'src-tauri/tauri.conf.json'), 'utf8'));
const cargo = fs.readFileSync(path.join(root, 'src-tauri/Cargo.toml'), 'utf8');
const cargoVersion = cargo.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
if ([lock.version, lock.packages[''].version, config.version, cargoVersion].some(v => v !== pkg.version)) fail('Versions differ between npm, Cargo and Tauri.');
else ok(`Version ${pkg.version}`);
if (Number(process.versions.node.split('.')[0]) < 22) console.warn('[WARN] Use Node.js LTS 22 or newer for this guide.');
else ok(`Node.js ${process.versions.node}`);
for (const command of ['rustc', 'cargo', 'rustup']) {
  const result = spawnSync(command, ['--version'], { encoding: 'utf8', windowsHide: true });
  if (result.error || result.status !== 0) fail(`${command} not found. Install Rust, then reopen PowerShell.`);
  else ok(result.stdout.trim());
}
if (process.platform === 'win32') {
  const rust = spawnSync('rustc', ['-vV'], { encoding: 'utf8', windowsHide: true });
  if (!rust.stdout?.includes('host: x86_64-pc-windows-msvc')) fail('Select the x64 MSVC toolchain: rustup default stable-x86_64-pc-windows-msvc');
} else console.warn('[WARN] BUILD.md and build-windows.ps1 describe Windows x64 builds.');
let checked = 0;
function resource(file) {
  checked++;
  if (!fs.existsSync(file)) fail(`Missing resource: ${path.relative(root, file)}. Download the complete repository including hidden directories.`);
}
for (const source of walk(path.join(root, 'src-tauri/src')).filter(p => p.endsWith('.rs'))) {
  const text = fs.readFileSync(source, 'utf8');
  for (const match of text.matchAll(/include_(?:bytes|str)!\(\s*"([^"]+)"/g)) resource(path.resolve(path.dirname(source), match[1]));
  for (const match of text.matchAll(/include_(?:bytes|str)!\(concat!\(\s*env!\("CARGO_MANIFEST_DIR"\),\s*"([^"]+)"/g)) {
    resource(path.resolve(root, 'src-tauri', '.' + match[1]));
  }
}
for (const file of Object.keys(config.bundle.resources || {})) resource(path.resolve(root, 'src-tauri', file));
for (const file of config.bundle.icon || []) resource(path.resolve(root, 'src-tauri', file));
resource(path.resolve(root, 'src-tauri', config.build.frontendDist, 'index.html'));
if (checked) ok(`Checked ${checked} embedded/resource paths`);
for (const name of ['HV-Launcher.bat', 'HV-PlugNPlay.bat', 'kvc.exe', 'drvloader.exe']) {
  if (!fs.existsSync(path.join(root, 'src-tauri/resources/hypervisor', name))) console.log(`[INFO] Optional hypervisor resource omitted: ${name}. Build is supported without it.`);
}
if (!fs.existsSync(path.join(root, 'node_modules/@tauri-apps/cli/package.json'))) fail('npm dependencies missing. Run npm ci in the repository root.');
else ok('Local Tauri CLI installed');
if (!process.env.CGS_RYUU_API_KEY) console.warn('[INFO] CGS_RYUU_API_KEY is not set. Public builds still work with HubCap and local import; Ryuu download needs your key.');
if (failures) { console.error(`\n${failures} check(s) failed. See BUILD.md.`); process.exitCode = 1; }
else console.log('\nReady. Run npm run build:portable or npm run build:setup.');
