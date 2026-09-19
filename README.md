# Sanctum

Sanctum is a local-first Research OS for preserving hypotheses, evidence, equations, literature, files, graph relations, and the history of how research changed.

Phase 1 is intentionally centered on data integrity: SQLite WAL transactions, immutable block versions, crash-recovery drafts, semantic graph edges, content-addressed attachments, verified snapshots, and read-back-verified encrypted backups.

## Install on Windows

For 64-bit Windows 10/11, download the latest `Sanctum-Setup-*-x64.exe` from GitHub Releases and double-click it. The NSIS installer uses per-user installation, so administrator privileges are not required.

This Phase 1 installer is not code-signed. Windows SmartScreen may therefore require **More info → Run anyway**. Verify the published SHA-256 before running it. The installed application bundles `WebView2Loader.dll`; Microsoft Edge WebView2 itself is normally present on supported Windows versions and the installer can bootstrap it when missing.

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
