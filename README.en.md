<div align="center">

**[Русский](README.md) | English**

# 🐊 CrocodileGena Studio

### **The all-in-one hub for your Steam library, DLC, online fixes, and Windows environment**

[![Version](https://img.shields.io/badge/Version-5.5.5-63f5c2?style=for-the-badge&logo=tauri&logoColor=black)](https://github.com/)
[![Platform](https://img.shields.io/badge/Platform-Windows_10_|_11_(x64)-0078D6?style=for-the-badge&logo=windows&logoColor=white)](https://microsoft.com/windows)
[![Stack](https://img.shields.io/badge/Stack-Rust_Tauri_v2_+_Modern_JS-black?style=for-the-badge&logo=rust&logoColor=orange)](https://tauri.app/)
[![Safety](https://img.shields.io/badge/Safety-Byte--for--byte_Backups-2ea44f?style=for-the-badge&logo=shield)](#safety)
[![Telemetry](https://img.shields.io/badge/Telemetry-0%25_Fully_Offline-a987ff?style=for-the-badge)](#about)

<p align="center">
  <b>Smart game scanning</b> • <b>One-click DLC unlocking</b> • <b>Universal Online Fix</b> • <b>Save backups</b> • <b>CrocLua & Manifests</b> • <b>Anti-cheat & Security</b>
</p>

[✨ About](#about) • [🧩 Modules](#modules) • [🛡️ Safety](#safety) • [⚙️ Customization](#customization) • [🚀 Installation & Building](#building) • [❓ FAQ](#faq)

---

</div>

## 🖼️ Interface

![Crocodile Gena Studio 5.5.5 home screen](docs/images/home.png)

<p align="center"><sub>The version 5.5.5 home screen. Customize the theme, accent, sidebar, and scale. The screenshot shows the Russian interface; English is also available in Settings.</sub></p>

---

<a name="about"></a>

## 📖 About

**CrocodileGena Studio (Desktop Edition)** is a next-generation desktop application designed to automate workflows for Steam games and networking components.

It combines file scanning, network emulator injection, save management, automatic game addition through Lua manifests, low-level Windows security management (VBS/HVCI), and anti-cheat driver auditing.

The application uses **Rust (Tauri v2) + a native Dark Glassmorphism UI**. File operations, the Windows registry, PE headers, and networking are handled directly by compiled code, without intermediary Python servers or console utilities.

---

<a name="modules"></a>

## 🧩 All Modules at a Glance

The application provides **7 workspaces**, accessible from the sidebar:

| No. | Module | Icon | Purpose |
| :---: | :--- | :---: | :--- |
| **01** | **Universal Online Fix** | 🛠️ | Steam network emulator, Spacewar lobbies, Epic Online Services (EOS) fixes |
| **02** | **Save Manager** | 💾 | Deep save discovery across 15+ locations, ZIP/RAR backups, restoration |
| **03** | **DLC Unlocker** | 🔑 | Selective DLC unlocking through **SmokeAPI** or **CreamAPI** |
| **04** | **CrocLua & Manifests** | 📜 | Add games to Steam, search through Ryuu/HubCap, Millennium plugin |
| **05** | **System Security** | 🛡️ | Launch profiles, cleanup for 12 anti-cheat systems, VBS/DSE/Test Mode management |
| **06** | **Crack Vault** | 📦 | Cloud release catalog with a built-in six-part downloader |
| **07** | **Utilities** | 🧰 | External tools (CirnoDownloader, Sigma, Zapret2) and guides |

---

### 🛠️ Module 01: Universal Online Fix

A unified workflow for adapting single-player and cooperative games for online play through Steam and Epic Games servers.

#### Key features

* **Smart game directory analysis**:
  * Drag and drop a folder or select it through the system file dialog.
  * Automatically discover the game executable (`.exe`), architecture (`x86` / `x64`), and `steam_api.dll` / `steam_api64.dll` libraries.
  * Detect game engines, including Unreal Engine 4/5, Unity, CryEngine, and others.
* **Intelligent identification**:
  * Extract an AppID automatically or search manually through the official Steam Store API, with high-resolution cover images.
* **Flexible lobby configuration (Fake AppID)**:
  * `480` — Spacewar *(the most popular choice)*;
  * `1484020` — Cube Racer;
  * `250820` — SteamVR;
  * `3989960` — Custom AppID.
* **Built-in DLC unlocker**:
  * Automatically collect the official DLC list from Steam Store.
  * Add a `[DLC]` section directly to the fix configuration files (`OnlineFix.ini` / `OnlineFix64.ini`).
* **Epic Online Services (EOS) module**:
  * Check whether `EOSSDK-Win64-Shipping.dll` is present and inspect its status.
  * Install the adapted EpicFix to address multiplayer room creation issues.
  * Instantly restore the original EOSSDK.
* **Interactive operation log (Live Log)**:
  * Follow each step: creating backups, replacing libraries, and writing INI files.

---

### 💾 Module 02: Save Manager

An automated hub for backing up and synchronizing game progress. It helps prevent save loss when reinstalling Windows, switching repacks, or installing fixes.

#### Supported locations (15+ categories)

1. **Windows directories**:
   * `%USERPROFILE%\Saved Games`
   * `%USERPROFILE%\Documents\My Games` and game folders directly inside Documents
   * `%LOCALAPPDATA%`, including Unreal Engine layouts such as `*\Saved\SaveGames`
   * `%APPDATA%` (Roaming)
   * `%LOCALAPPDATA%Low`, including Unity layouts such as `LocalLow\<Company>\<Game>`
2. **Official Steam client**:
   * `<Steam>\userdata\<AccountID>\<AppID>`
3. **Steam emulators and common release groups**:
   * `Goldberg SteamEmu Saves` / `GSE Saves`
   * `SmartSteamEmu` / `SteamEmu`
   * `CODEX` / `EMPRESS` / `RUNE` / `FLT` / `TENOKE` / `SKIDROW`
   * `OnlineFix` / `0xdeadc0de`

#### Features

* **Smart clutter filtering**: automatically ignore temporary caches and files such as `cache`, `gpucache`, `shadercache`, `crashpad`, `logs`, `.tmp`, `.dmp`, and `lockfile`.
* **Detailed statistics**: view the number of folders and files discovered and their total size.
* **Archive creation**:
  * Built-in **ZIP** compression without external dependencies.
  * **RAR** archive creation when WinRAR is installed.
  * Each archive includes a `cgs-save-manifest.json` manifest recording the exact original paths.
* **Safe restoration**:
  * Restore archives to their original system folders, with path traversal protection and size checks.

---

### 🔑 Module 03: DLC Unlocker

A full-featured DLC manager that provides access to an installed game's DLC catalog without manually editing configuration files.

~~~text
                 Scan Steam Libraries
              libraryfolders.vdf + ACF
                          │
                  Combined DLC Catalog
              Steam Store API + AppInfo.vdf
                          │
               ┌──────────┴──────────┐
               ▼                     ▼
           CreamAPI               SmokeAPI
      Replace steam_api.dll    Proxy next to game EXE
      Back up steam_api_o.dll version / winhttp / winmm
      ExtraProtection option  Steam libraries unchanged
~~~

#### Key features

* **Deep Steam library integration**:
  * Scan libraries across drives by parsing `libraryfolders.vdf` and `appmanifest_*.acf`.
  * Automatically exclude incomplete downloads, system tools, and utilities.
  * Add a custom library or select a game folder manually.
* **Combined DLC catalog**:
  * Combine the official Steam Store API with the local binary `appcache/appinfo.vdf` cache (formats v27, v28, and v29).
  * Display hidden DLC, DLC removed from the store, and region-restricted DLC.
  * Search by DLC name or ID.
  * Use “Unlock all” or manually select specific DLC with checkboxes.
* **Two independent installation methods**:
  * **SmokeAPI (4.1.3)**:
    * Proxy system libraries next to the game EXE (`version.dll`, `winhttp.dll`, or `winmm.dll`).
    * **Steam files are not modified!**
    * Analyze the target executable's PE architecture (`x64` / `x86`) and select the appropriate build.
    * Maintain a transactional `.cgs-smaki.json` installation record for clean removal.
  * **CreamAPI**:
    * Classic integration by replacing `steam_api.dll` / `steam_api64.dll`.
    * Automatically back up originals as `steam_api_o.dll` / `steam_api64_o.dll`.
    * **Extra Protection** option: bypass the game's internal Steam API signature and checksum checks.

---

### 📜 Module 04: CrocLua & Manifests

A tool for adding licensed games directly to the official Steam client through Lua scripts and depot manifests.

#### Key features

* **Automatic Steam detection**:
  * Read installation paths from the Windows registry (`HKCU\Software\Valve\Steam`).
  * Restart Steam with one click to apply added games.
* **Manifest service integration**:
  * **Ryuu Generator** (`generator.ryuu.lol`):
    * Search the database and download manifests directly.
    * Built-in Cloudflare bypass using a real Chrome User-Agent.
  * **HubCap** (`hubcapmanifest.com`):
    * Use your own API key.
    * Track daily quotas and remaining downloads in the interface (0/25 counter).
* **Automatic file placement**:
  * Place `.lua` files in `config/stplug-in/` and `.manifest` files in the root `depotcache/` directory.
* **Manual import**:
  * Drag and drop `.lua`, `.manifest`, or `.zip` files.
* **Added game management**:
  * View active games, temporarily disable them, or remove them.
  * Detailed game cards: release date, cover image, and a comparison of unlocked DLC against DLC missing from the Lua file.
  * **Automatic manifest updates** option.
* **Millennium plugin (“Crocodile Manifest”)**:
  * Bundled plugin for Millennium (v0.4.9).
  * One-click installation adds a **“Get manifest”** button directly to game pages in the official Steam Store!

---

### 🛡️ Module 05: System Security, Anti-Cheat & VBS

An engineering workspace for auditing the Windows environment, managing security services, and safely launching game configurations.

#### 1. Launch profile manager

* Organize games and assign specialized launch profiles (KVC / DRV).
* Automatically check administrator permissions (UAC) and environment readiness before launching.
* Follow each step through the interactive event log.

#### 2. Anti-Cheat Auditor

Deep scanning and removal of leftover components from **12 popular anti-cheat systems**:

1. **Easy Anti-Cheat (EAC)** — Epic Games (`EasyAntiCheat`, `EasyAntiCheat_EOS`);
2. **BattlEye** — BattlEye Innovations (`BEService`);
3. **Riot Vanguard** — Riot Games (`vgk.sys`, `vgc`);
4. **FACEIT Anti-Cheat** — FACEIT AC;
5. **EA AntiCheat** — Electronic Arts (`EAAntiCheat`);
6. **PunkBuster** — Even Balance (`PnkBstrA`, `PnkBstrB`);
7. **XIGNCODE3** — Wellbia (`xhunter1.sys`);
8. **nProtect GameGuard** — INCA Internet (`npggsvc`);
9. **Anti-Cheat Expert (ACE)** — Tencent (`ace-base.sys`, `ace-game.sys`);
10. **TrueProtect / MoMo** — NetEase;
11. **Denuvo Anti-Cheat** — Irdeto (`DenuvoAntiCheat`);
12. **EQU8 Anti-Cheat** — EQU8.

> [!NOTE]
> Before any anti-cheat cleanup operation, the application creates a local backup of the services and paths being removed.

#### 3. VBS & Windows Security

* Inspect **Virtualization-based Security (VBS)** and **Hypervisor-protected Code Integrity (HVCI)**.
* Check Device Guard, Credential Guard, System Guard, and HyperGuard settings in the `HKLM` registry.
* Monitor **Driver Signature Enforcement (DSE)**.
* Manage **Windows Test Mode (Testsigning)** through BCD configuration:
  * Enable Test Mode to launch unsigned libraries.
  * Show a reminder about disabling Secure Boot in BIOS/UEFI.
* Safely disable the visor (“Disable visor”), saving previous registry values and boot settings to `cgs-vbs-snapshot.json` for instant rollback (“Enable visor”).

---

### 📦 Module 06: Crack Vault

A built-in client for downloading verified fixes and cracks from the Pirate Station community.

* **Direct synchronization**: automatically load the catalog from the cloud repository.
* **Pinned files**: quickly access popular libraries and universal emulators.
* **Detailed information**: game build number, archive passwords, file size, and release date.
* **High-speed six-part downloader (Parallel Multi-Part Engine)**:
  * Download in six parallel parts, with speed calculation and progress display.
  * Resume downloads and cancel safely.
  * Open the downloaded file's folder in Windows Explorer.

#### 📂 Extract ZIP / RAR Directly into the Game Folder

After downloading an archive, click **“Extract into game”** — no manual file transfer and no WinRAR installation required for this operation.

* **DenuvOwO and ARTIFACT support**: find the appropriate folder inside the archive. If several match, choose the one you need.
* **Contents go into the game root**: copy the selected folder's contents, not its outer container. Preserve the internal directory structure.
* **Automatic password**: use the password from the download card and copy it to the clipboard.
* **One window and one “Extract” button**: choose the game folder, preview the contents, and start extraction without a separate “Prepare” screen.
* **Short contents list**: show each top-level folder once, without listing every nested path. JPG/JPEG files are hidden only from the list — they are still extracted.
* **Backups of replaced files**: save originals to `.cgs-backups` inside the game folder. The downloaded archive stays in Downloads.
* Archive contents are **not executed automatically**.

More about the extractor: **[ARCHIVE-INSTALL.md](ARCHIVE-INSTALL.md)** *(Russian)*.

---

### 🧰 Module 07: Utilities (Toolbox)

An integrated hub of companion software for downloading and configuring games:

* **CirnoDownloader** — a high-speed downloader for Steam manifests and content.
* **Sigma** — an alternative downloader with a graphical interface.
* **Zapret2** — a networking utility for bypassing DPI throttling and game server blocks.
* **Interactive guide**: step-by-step instructions for configuring Zapret2 presets (including presets with ALT) to keep downloads stable.

---

<a name="customization"></a>

## 🎨 Settings & Personalization

Use **Settings** to tailor the application to your preferences:

* 🌈 **Accent colors**:
  * 🟢 **Mint** — neon mint, the default;
  * 🟣 **Violet** — deep cyber violet;
  * 🔵 **Blue** — classic neon blue;
  * 🔴 **Rose** — bright neon pink.
* 🌐 **Language switching**: instantly switch between **Russian** and **English** without restarting.
* 🔑 **CrocLua service selection**: switch between Ryuu with a key supplied at build time and HubCap with your own key and quota monitoring.
* 🔄 **Update center**:
  * Check for new application versions through the official GitHub repository.
  * Download an updated **Portable EXE** or **NSIS Setup** directly.
* 📢 **Official community links**: quickly access the Pirate Station community and the developer's support channel on Telegram.

### ✨ Ready-Made Interface Styles

**11 themes**, including a light theme:

| Theme | Character |
| :--- | :--- |
| **Graphite** | The default dark interface |
| **Glass** | Glass-inspired styling |
| **Minimal** | Restrained minimalism |
| **Neon** | Neon accents |
| **Midnight** | A dark blue palette |
| **Obsidian** | Deep dark tones |
| **Forest** | A green palette |
| **Plum** | Purple tones |
| **Warm** | A warm palette |
| **Arctic** | Cool tones |
| **White** | A light interface |

### 🧩 Make the Workspace Your Own

* **Sidebar**: full, compact, or icons only. Change the module order.
* **Module visibility**: hide unused sections from the menu and Home, then restore them in Settings.
* **Scale from 90–125%** and three element shapes: sharp, soft, and rounded.
* **Five typography choices**: system font, Bahnschrift, Verdana, Tahoma, and Consolas. Adjust font weight and enable higher contrast.
* **Animations and glow**: toggle them independently.
* **Gena, the live mascot**: reacts to your actions and can appear in the sidebar or a floating panel. Disable the mascot or its reactions if you prefer.

Settings are saved locally and applied without restarting the application.

---

<a name="safety"></a>

## 🛡️ Safety & Rollback

User data safety is CrocodileGena Studio's top priority. The application follows strict protection standards:

~~~text
[User] → [Action] → [Byte-for-byte backup] → [Hash manifest] → [Apply fix]
                                                               │
[Restore] ← [Remove fix] ← [Verify hashes] ← [Restore original] ←─┘
~~~

1. **Byte-for-byte backups (`.backup_orig`)**: before replacing an original Steam library (`steam_api.dll`, `steam_api64.dll`, `EOSSDK-Win64-Shipping.dll`), save an exact copy and a JSON manifest with checksums.
2. **Clean removal**: “Remove / restore” deletes the installed fix files and returns the originals to their locations.
3. **Registry isolation**: save changes to VBS and Device Guard settings in a local `cgs-vbs-snapshot.json` snapshot.
4. **SmokeAPI Safe Mode**: remove `version.dll` / `winhttp.dll` / `winmm.dll` proxies only when their hashes match the unlocker. Unrelated system libraries are never overwritten or damaged.
5. **Save protection**: strictly validate paths during save backup and extraction to prevent path traversal.

---

<a name="building"></a>

## 🚀 Installation & Building

### Ready-Made Releases

Download a build from **Releases**:

* `CrocodileGenaStudio-Setup.exe` — Windows installer;
* `CrocodileGenaStudio-Portable.exe` — portable version without installation.

---

### Building from Source

Detailed instructions for building, setting up your environment, and troubleshooting are available in:

👉 **[BUILD.md](BUILD.md)** *(Russian)*

~~~powershell
# Quick start:
npm ci
npm run doctor
npm run dev

# Build the release installer:
npm run build:setup
~~~

---

<a name="faq"></a>

## ❓ Frequently Asked Questions (FAQ)

<details>
<summary><b>Should I use SmokeAPI or CreamAPI for DLC unlocking?</b></summary>
<br>

* **SmokeAPI (recommended by default)**: installs next to the game EXE (`version.dll`, `winhttp.dll`, or `winmm.dll`). Original Steam files are not modified. The unlocker often continues working after game updates without reinstallation.
* **CreamAPI**: replaces `steam_api.dll` or `steam_api64.dll`. Suitable for older games or games whose security mechanisms block external proxy DLLs next to the EXE.
</details>

<details>
<summary><b>Why does my game not launch after installing Online Fix?</b></summary>
<br>

1. Make sure the official **Steam** client is running in the background.
2. If the game uses Epic Games networking, open **Epic Online Services** in Module 01 and click **“Install EpicFix”**.
3. Check your antivirus (Windows Defender): it may mistakenly quarantine the emulator. Add the game folder to exclusions.
</details>

<details>
<summary><b>How do Test Mode (Testsigning) and VBS work?</b></summary>
<br>

Some specialized drivers require disabling signature enforcement (DSE) or core isolation (HVCI/VBS). CrocodileGena Studio lets you disable these mechanisms with one click. To activate Test Mode, first disable **Secure Boot** in your motherboard's UEFI/BIOS settings.
</details>

<details>
<summary><b>Where are rollback backups stored?</b></summary>
<br>

The application creates a `.backup_orig` directory in the game folder. It stores the original file and a hash manifest. During rollback, the application verifies checksums and guarantees restoration to the original state, then safely removes `.backup_orig`.
</details>

---

## Source Code License

The author's own code is available under **GPL-3.0-only** — see [LICENSE](LICENSE).
Third-party DLLs, executables, and dependencies have separate terms described in [THIRD_PARTY.md](THIRD_PARTY.md) *(Russian)*.
For building from source, start with [BUILD.md](BUILD.md); the `BUILD-WINDOWS.cmd` helper is also available.

## ⚖️ Disclaimer

This software is created exclusively for educational, research, and evaluation purposes: studying Windows application architecture, reverse engineering dynamic libraries, and exploring virtualization mechanisms. The developers are not responsible for unlawful use or violations of third-party service agreements.

---

<div align="center">

**CrocodileGena Studio** • *Built with care for its users* • 2026

[![Telegram Channel](https://img.shields.io/badge/Telegram-Pirate_Station-2CA5E0?style=for-the-badge&logo=telegram&logoColor=white)](https://t.me/piratestationn)
[![Telegram Support](https://img.shields.io/badge/Telegram-Developer_Support-2CA5E0?style=for-the-badge&logo=telegram&logoColor=white)](https://t.me/crocodilehub2)

</div>
