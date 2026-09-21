# Sanctum Phase 1 実装状況

## 実装済み

| Phase 1 要件 | 実装 |
|---|---|
| Project / Vault | staging 内で全構造を作ってから atomic rename。manifest/DB identity 照合、exclusive lock |
| Hypothesis Block | kind/status/tags/body/research notes、optimistic concurrency、soft delete/Trash |
| Markdown + LaTeX | Editor/Split/Preview、GFM、inline/display KaTeX、code/table/quote/task/footnote token/citation token |
| Block Graph | drag/pan/zoom/search/status filter/edge filter、保存済み座標 |
| Semantic Edge | 9種類、方向、note、soft delete |
| File / PDF Attachment | relation 付き複数添付、drag & drop、ファイル名検索、画像preview、OS viewer、soft delete、CAS dedup、SHA-256 |
| Version History | full immutable snapshot、reason/hash/diff、新 version として restore |
| Hypothesis Branch | 親 block/version を固定した lineage。merge は未実装 |
| Autosave | 300ms recovery draft + 1500ms immutable version commit。無変更保存なし、競合は上書きせず停止 |
| SQLite WAL | bundled SQLite、WAL、FULL synchronous、FK、busy timeout、application ID |
| Snapshot | Manual/10-minute/Daily/Weekly policy、online backup、zstd readback、clone restore |
| File Integrity | CAS object 全件 hash 検査、Snapshot/Backup manifest と DB object 一覧の照合 |
| Backup | 自己完結 archive、Argon2id、chunked XChaCha20-Poly1305、復号後 readback 検証 |
| Automatic Backup | 24時間間隔、外部folder、Windows Credential Manager、既存の暗号化・readback検証pipelineを再利用 |
| Portable Export | Markdown、添付実体、BibTeX/JSON、relations、variables、全file SHA-256 manifest。新規folderへatomic publish |
| Restore | 既存 path 拒否、staging 検証後 atomic publish、DB/履歴/journal/object を復元 |
| Search | title/body/notes/LaTeX/tags/variables/citations/file metadata の FTS5 |
| Citation Import | DOI/Crossref、複数entry BibTeX、手入力。既存citation keyは更新し、同一blockへの重複linkを作らない |
| Research Integrity | DB/FK/journal/version/object/snapshot/backup hash、unsupported、rejected dependency、dangling edge、variable conflict、broken citation、orphan、unbacked change |
| Security | local-first、DOI取込時だけCrossref、strict CSP、限定 capability、backup key zeroization |
| ChatGPT / Codex connection | 個人用marketplaceに加えSecure MCP Tunnelで普通のChatGPTにも接続。localhost限定MCP、active Vaultのみ、検索・履歴/関係/UTF-8 text添付の読取・作成・CAS添付・競合安全な編集・整合性確認。delete/restore非公開 |
| Secure MCP Tunnel | 公式Windows clientをSanctum管理下へcopyし、outbound-onlyで起動・停止・自動再接続。Runtime API keyはWindows Credential Manager、設定JSONには非secretのTunnel IDとclient pathだけを保存 |

## 検証結果

- `cargo test -p sanctum-core -- --test-threads=1`: **26 tests**（0.4.0 CIで再検証）
- `cargo clippy -p sanctum-core --all-targets -- -D warnings`: **passed**
- frontend Vitest: **14 passed**
- Sanctum MCP / personal-plugin unit tests: protocol metadata、core経由の履歴保存、stale update拒否、marketplace非破壊merge、invalid JSON拒否、deep-link encoding
- Windows durability regression: existing files are reopened read/write before `FlushFileBuffers`; Vault creation, attachments, snapshots, backups, and restores share the tested helper
- Windows production bundle regression: release buildは`custom-protocol`を必須化し、Tauriがdevelopment modeを報告した場合はbuild scriptが停止。0.1.2実行ファイルへのfrontend埋め込みも検査済み
- Autosave regression: DBメタデータを除いた正規化snapshotだけを比較し、無変更の再保存を停止。1編集につき1回だけ正式保存するfrontend testを追加
- TypeScript project build: **passed**
- Vite production build: **passed**。最大 JavaScript chunk は Markdown 系 約433 kB（gzip 約131 kB）
- Windows x64 executable: **build passed**。PE32+ / Windows GUI subsystem、外部 GNU runtime DLL 依存なし
- NSIS installer: **generated and archive-tested**。`sanctum-desktop.exe` と同一の `WebView2Loader.dll` を同梱、LZMA archive test passed
- Tauri adapter の Linux native check: この実行環境に `pkg-config` / GTK / WebKitGTK development package がないため system dependency build で停止。保存 core、TypeScript、frontend bundle の失敗ではない。CI には公式 Linux prerequisites を入れた desktop check を用意した
- Windows実機での install / launch / Vault round-trip は、このLinux環境では未実行。Windows runnerで再現可能な installer workflow を追加した

## 意図的に Phase 1 外

- cloud sync、共同編集、自律的なAI一括変更
- branch merge UI
- PDF 本文抽出・annotation locator UI・内蔵PDF renderer
- live Vault の at-rest encryption
- mobile client
- CAS object の物理 garbage collection

## 残余リスク

1. 300ms以内の最終 keystroke は process/電源断で失いうる。
2. local Snapshot は同じ Vault の CAS を共有するため、端末全損には外部 Backup が必要。
3. live Vault は平文。OS full-disk encryption が必要。
4. password を失うと encrypted Backup は復号できない。
5. storage firmware が flush 成功を偽る障害、全コピー同時破壊はアプリだけでは防げない。
6. Phase 1 installer は未署名のため、Windows SmartScreen が確認を表示する可能性がある。正式配布では信頼された code-signing certificate が必要。
7. クロス生成したinstallerは構造・内容・hashまで検査済みだが、実Windows上のinstall / launch / uninstall試験は別途必要。
8. ChatGPT接続は同一ユーザーのlocalhost境界。任意コードを実行できる同一WindowsユーザーはVaultファイル自体にもアクセスできるため、OSアカウントとfull-disk encryptionを信頼境界に含める。
9. Secure MCP Tunnelを通じてChatGPTが取得した研究情報はOpenAI側の処理対象になる。Developer modeのwrite toolにはmodel誤操作とprompt injectionの残余リスクがある。

これらを隠して「絶対安全」と表示しない。Research Integrity と Recovery Center は、最後の外部 backup より新しい研究変更も警告する。
