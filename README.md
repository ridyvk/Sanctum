# Sanctum

Sanctum is a local-first Research OS for preserving hypotheses, evidence, equations, literature, files, graph relations, and the history of how research changed.

Phase 1 is intentionally centered on data integrity: SQLite WAL transactions, immutable block versions, crash-recovery drafts, semantic graph edges, content-addressed attachments, verified snapshots, and read-back-verified encrypted backups.

## Install on Windows

For 64-bit Windows 10/11, download the latest `Sanctum-Setup-*-x64.exe` from GitHub Releases and double-click it. The NSIS installer uses per-user installation, so administrator privileges are not required.

This Phase 1 installer is not code-signed. Windows SmartScreen may therefore require **More info → Run anyway**. Verify the published SHA-256 before running it. The installed application bundles `WebView2Loader.dll`; Microsoft Edge WebView2 itself is normally present on supported Windows versions and the installer can bootstrap it when missing.

## Android preview

The Android build has a dedicated single-column interface with a bottom navigation bar for blocks, writing, details, graph, and data. It stores Vaults in the app's private local directory; the Windows interface and Windows updater are unchanged. Vaults on different devices do not synchronize automatically.

The `Android preview` GitHub Actions workflow builds a debug-signed ARM64 APK as a workflow artifact on pull requests and manual runs. This is a preview build, not the Windows release installer. To build locally with Android Studio, the Android SDK/NDK, Java 17, and the Rust Android target installed:

```bash
npm ci
npm run tauri android init -- --ci
npm run tauri icon -- src-tauri/icons/icon.png
npm run tauri android build -- --apk --debug --target aarch64 --ci
```

To install the preview from GitHub Actions, download the `sanctum-android-arm64-debug` artifact, extract its ZIP, and open the `.apk` inside on the phone. Android may ask you to allow installation from the specific app that opened the APK (for example, your browser or file manager); grant that permission only if you trust the APK and then return to the installer. This is a debug-signed preview outside Google Play, so system warnings may still appear. If Android says the APK is harmful or installation fails, note the exact message instead of forcing the install. Do not uninstall an existing preview without first exporting an encrypted backup: its app-private Vaults are removed on uninstall, and a different debug signing key may prevent an in-place update.

To carry a Windows Vault to Android, make an encrypted `.sanctum-backup` on Windows, transfer that file to the phone, and select **Backupを読み込む** on the Android Home screen. To take a copy off the phone, use **データ → Backupを書き出す** and choose a document destination. Backups are verified before export and read back after writing. The Android file transfer UI currently limits individual imports and exports to 64 MB. Android's app-private Vault is removed if the app is uninstalled; keep an external encrypted backup before doing so.

## Read first

- [Architecture and data model (Japanese)](docs/DESIGN_JA.md)
- [Data-safety self review (Japanese)](docs/SAFETY_REVIEW.md)
- [MVP plan](docs/MVP_PLAN.md)
- [Recovery runbook](docs/RECOVERY_RUNBOOK.md)

## Development

```bash
npm install
npm test
npm run build
cargo test -p sanctum-core
npm run tauri dev
```

The native Windows installer is built and signed for the in-app updater on a Windows runner by the `publish-windows-update` workflow. Release artifacts must pass the Rust safety tests, frontend tests, production frontend build, and native Tauri build before they are published.

The browser-only Vite target is for UI development and explicitly reports that durable Vault operations require the Tauri runtime. It must never claim that research data was saved.

## 0.5.2 workspace controls

Version 0.5.2 lets either workspace side panel collapse independently so the editor uses the freed width. Blocks now expose a right-click menu with a recoverable move-to-trash action, and both the main text and research notes can expand into a distraction-free large preview. The Vault format and storage core are unchanged.

## 0.5.1 ordinary ChatGPT connection

Version 0.5.1 connects the same localhost-only MCP server to ordinary ChatGPT web conversations through OpenAI Secure MCP Tunnel. The Home screen guides a one-time setup: create a private tunnel, select the official Windows `tunnel-client`, enter the tunnel ID and runtime API key, then add the tunnel from ChatGPT developer mode. Sanctum starts and stops `tunnel-client` with the application and reconnects automatically on later launches.

The runtime API key is stored in Windows Credential Manager. It is passed to `tunnel-client` only through its process environment and is never written to the Sanctum config, command arguments, GitHub, or the frontend. The tunnel is outbound-only; the MCP listener remains bound to `127.0.0.1:43991` and no inbound firewall port is opened. This is a private developer-mode connection, not a public plugin submission. See the official [Secure MCP Tunnel guide](https://developers.openai.com/api/docs/guides/secure-mcp-tunnels) and [ChatGPT developer mode guide](https://developers.openai.com/api/docs/guides/developer-mode).

## 0.5.0 private ChatGPT connection

Version 0.5.0 adds a local MCP endpoint and a personal Sanctum plugin for ChatGPT desktop/Codex. While Sanctum is running, the plugin can inspect the active Vault, list and search blocks, read block history and references, inspect the research graph, read bounded UTF-8 text attachments, create or concurrency-safely update blocks, attach explicitly named local files, and run integrity checks. Every write still passes through `sanctum-core`, including immutable versions, journal events, content-addressed storage, and optimistic concurrency. Destructive delete and restore tools are intentionally not exposed.

The Home screen's `ChatGPT接続` action installs the plugin into the current user's personal marketplace. It is not submitted to the public Plugins Directory, and the MCP server binds only to `127.0.0.1:43991`; live Vault content is not uploaded to a separate Sanctum service.

## 0.4.5 opaque application icon

Version 0.4.5 keeps the supplied Sanctum mark and its 0.4.4 placement unchanged while replacing transparency with a solid white square background. The opaque artwork is bundled into the Windows executable, installer, Start menu shortcut, desktop shortcut, and taskbar icon; no image is added inside the application UI.

## 0.4.4 transparent application icon

Version 0.4.4 replaces the white icon tile with the supplied two-orbit Sanctum mark on a genuinely transparent background. The transparent artwork is bundled into the Windows executable, installer, Start menu shortcut, desktop shortcut, and taskbar icon; no image is added inside the application UI.

## 0.4.3 Windows icon refresh

Version 0.4.3 refreshes existing Start menu and desktop shortcuts after an in-app update and notifies Windows that shell icons changed. This makes the 0.4.2 monochrome application icon visible without deleting user data or reinstalling from scratch.

## 0.4.2 application icon

Version 0.4.2 replaces the application and installer icon with the supplied monochrome Sanctum symbol. The original geometry is centered on a white rounded tile so it remains legible against both dark and light Windows surfaces. No decorative image was added inside the application UI.

## 0.4.1 text contrast

Version 0.4.1 keeps the existing type sizes and lightweight Japanese typography while increasing secondary and faint text contrast in both dark and light themes.

## 0.4.0 data portability and daily backup

Version 0.4.0 adds a portable export folder containing Markdown, original attachments, BibTeX/JSON citations, graph relations, variables, and a SHA-256 manifest. Optional daily encrypted backups reuse the verified backup pipeline; the password is stored only in Windows Credential Manager. Attachments support multi-select and native drag-and-drop, image preview, default-application opening, deletion, and filename search. Citations can be imported from DOI/Crossref or multi-entry BibTeX. The everyday UI now keeps integrity, branch, metadata, legacy statuses, and recovery details behind explicit detail controls without changing existing Vault data.

## 0.3.0 signed in-app updates

Version 0.3.0 is the one-time updater bootstrap release. After installing it, Sanctum checks the signed GitHub Release feed only while the Home screen is open. An available update can be downloaded and installed from the version control at the bottom of Home. Updating is intentionally unavailable while a Vault is open, so it cannot restart the process during an edit or save.

## 0.3.1 monochrome reading UI

Version 0.3.1 removes on-screen image icons in favor of short text labels, reduces the Home wordmark, uses a compact Mincho stack for Japanese, and makes the light theme strictly monochrome with black as its accent.

## 0.3.2 Windows UI typography

Version 0.3.2 replaces the Mincho stack with the Windows VS Code-style UI stack: Segoe UI for Latin text and Yu Gothic UI for Japanese, with natural letter spacing. Code editors remain monospaced.

Updater packages and `latest.json` are signed in CI with a repository secret. The app contains only the public verification key. See [updater release setup](docs/UPDATER_RELEASE.md).

## 0.2.0 editor and UI fix

Version 0.2.0 normalizes block snapshots before change detection, preventing an unchanged block from creating versions in a loop. Routine fast autosaves no longer flash a spinner. The interface now uses a black-and-white dark theme, concise Japanese UI labels, and explicitly styled native select options. The GitHub Actions workflow builds a checked Windows installer for every main-branch update and publishes installer assets for version tags.

## 0.1.2 Windows production-bundle fix

Version 0.1.2 connects the Tauri CLI production feature to `tauri/custom-protocol`, so an installed release loads the frontend embedded in the executable instead of the Vite development URL at `localhost:1420`. A release-build guard now fails the build if Tauri still reports development mode, preventing the same broken installer from being published again.

## 0.1.1 Windows durability fix

Version 0.1.1 reopens already-created files with write access before calling the operating-system durability flush. Windows requires that access for `FlushFileBuffers`; 0.1.0 used a read-only handle and could report `Access is denied (os error 5)` after otherwise successful Vault creation. The same correction covers attachment publishing, snapshots, backups, and restores, with a regression test around the shared helper.

## Security boundary

External backups are encrypted client-side. The live Vault is **not encrypted at rest in Phase 1**; use operating-system full-disk encryption. Never place a live WAL-mode Vault on a network filesystem whose locking semantics are unknown.
