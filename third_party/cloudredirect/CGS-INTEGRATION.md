# CrocLua local cloud engine

Upstream: https://github.com/crghofficial/cloudredirect

Pinned source: `4e81e492e4678c1d907fbeaef2625846f6afaad8` (upstream 2.6.6 source).
License: MIT, preserved in LICENSE. This is a modified build, `2.6.6-cgs-local.1`.

CGS changes:

- Native namespace registration and RPC handling require an active numeric root
  Lua in `config/lua` or `config/stplug-in`, a self `addappid`, and at least one
  referenced depot manifest. Commented auto-update pins are accepted, even
  after Steam evicts downloaded manifest files from its cache.
- Depot IDs, disabled Lua, Lua without manifest references and games without Lua pass through
  to Steam. Removing a Lua is checked again when an RPC arrives.
- Deferred seed receives the filtered namespace list.
- Provider configuration is isolated at `<Steam>/config/cgs-local-cloud.json`.
  Only the folder provider is accepted. Stats, achievements, playtime, Lua
  synchronization and parental features are off in CGS-generated configuration.
- Local working data is isolated at `<Steam>/cgs-local-cloud`.
  The selected folder holds the local provider's save data; the Steam directory
  retains its working cache and synchronization metadata.
- Automatic binary updates and global legacy save cleanup are disabled.

Build from an x64 MSVC developer environment:

```powershell
./tools/Build-LocalCloud.ps1
```

The helper builds only the C++ DLL with the static MSVC runtime. No upstream
companion app, batch scripts, credentials or external cloud account is used.
The resulting DLL is embedded in the Rust application. Standard application
builds can use the committed DLL without installing C++ tools.

Configuration is applied with Steam stopped, backed up before changes, and
Steam is restarted normally. Disabling restores the previous `[cloud]` section
without deleting saves. Folder changes copy existing provider data and reject
conflicting destination files. Originals remain in the old location.

This uses undocumented Steam interfaces. A successful configuration operation
does not establish that a particular Steam build/game has synchronized; verify
a save/load and a Steam restart using a test save before relying on it.
