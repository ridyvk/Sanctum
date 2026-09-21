# Sanctum Phase 1 — データ安全性の自己レビュー

## 結論

設計の重大リスクは、単なる autosave 不具合ではなく、(1) 成功表示と durable commit のずれ、(2) 履歴自体の書き換え、(3) 未検証 backup、(4) 復元時の上書き、(5) DB と添付実体の部分成功に集中する。Phase 1 は各箇所に fail-closed の境界を置く。

ただし「絶対に失わない」は物理的には保証できない。全コピーの同時破壊、記憶装置や OS の虚偽応答、backup password の紛失、利用者が外部 backup を作らない状況までは防げない。UI はこの限界を明示する。

## リスク登録簿

| ID | リスク | 影響 | 対策 | 残余リスク |
|---|---|---|---|---|
| R1 | WebView/OS が編集中に停止 | 直近入力喪失 | 300ms recovery draft + 1500ms immutable autosave | 最後の約300ms、storage 自体の故障 |
| R2 | DB commit 前に成功表示 | 利用者が保存済みと誤認 | transaction commit 後だけ成功応答 | OS/drive が flush を偽る場合 |
| R3 | last-write-wins | 別 window の変更消失 | `row_version` optimistic concurrency | 手動解決は必要 |
| R4 | WAL/DB の不適切な file copy | 復元不能 snapshot | SQLite Online Backup API | SQLite/OS 下層障害 |
| R5 | version 行の改変・欠落 | 研究履歴の信頼喪失 | DB trigger、snapshot hash、投影一致検査 | 管理者による全証跡の巧妙な改変 |
| R6 | 添付 copy と DB relation の部分成功 | broken link / orphan | staging→hash/fsync→rename→DB transaction | crash 時の harmless orphan |
| R7 | 同一 Vault 内 snapshot だけに依存 | 端末故障で全消失 | UI で external backup 未実施を警告 | external backup を作らない選択 |
| R8 | 暗号化しただけで backup 成功扱い | 壊れた backup の蓄積 | publish 前に復号・展開・全検証 | 将来の媒体劣化。定期再検査が必要 |
| R9 | restore が現行 Vault を上書き | 唯一の良好コピー喪失 | clone-only、既存 destination 拒否 | 利用者が後で手動削除する場合 |
| R10 | archive path traversal / link | Vault 外上書き | allowlist、正規 path、link/重複拒否 | archive parser の未知の欠陥 |
| R11 | migration 途中停止 | schema 半更新 | transaction + checksum + pre-migration snapshot | disk full が snapshot 前に発生 |
| R12 | soft delete が依存 graph を隠す | 誤った結論 | dangling/rejected dependency integrity check | 意味判断は研究者に残る |
| R13 | variable 同記号・異定義 | 数式解釈の混同 | project registry + normalized definition conflict | 同義表現の自動判定は限定的 |
| R14 | journal file 書込前 crash | DB と journal のずれ | transactional outbox、次回 open 時 drain | directory/file system 全体破損 |
| R15 | live DB を network share に配置 | lock/WAL semantics 崩壊 | Phase 1 で非対応を明示・警告 | 強制配置された場合は保証外 |
| R16 | live data が平文 | 端末盗難時の漏洩 | OS full-disk encryption 推奨、backup は暗号化 | Phase 1 live Vault は暗号化なし |
| R17 | Snapshot の zstd bit rot | restore 時まで気づかない | 作成直後 readback、定期 integrity check | 検査間隔中の劣化 |
| R18 | disk full | transaction/backup 失敗 | 空き容量の事前確認、atomic temp、失敗を明示 | 事前値と実消費の競合 |
| R19 | publish直前に同名pathが作られる | 既存Vault/backup上書き | OSのatomic no-replace rename。競合は失敗 | 未対応OSではpublish自体を拒否 |
| R20 | snapshot directory公開後、DB記録前に停止 | 有効snapshotがUIから見えない | open時に未記録manifestを検証してappend-only recordを回復 | 壊れた未記録snapshotは無視しlive Vaultを優先 |
| R21 | AI接続がSQLiteを直接更新 | 履歴・journal・競合検知の迂回 | MCPはactive `Arc<Vault>`の公開APIだけを呼び、直接DB pathを公開しない | core API自体の欠陥 |
| R22 | AIが古い内容で上書き・破壊操作 | 新しい研究変更の消失 | updateは`expectedRowVersion`必須。delete/restore toolは非公開、stale updateはfail-closed | 利用者が競合後に誤った統合を明示する場合 |
| R23 | MCPを外部やWeb pageから呼ばれる | 研究情報漏洩・不正変更 | `127.0.0.1`だけにbind、browser `Origin`/CORS要求拒否、JSON専用、ChatGPT pluginはpersonal marketplaceのみ | 同一OSユーザーで任意コードを実行できる攻撃者はVault file自体も読める |
| R24 | Secure MCP TunnelのRuntime API keyが漏れる | 第三者がTunnelを使用 | keyはWindows Credential Managerだけに保存し、process environmentで渡す。設定JSON・argv・frontend・GitHubへ出さない | 同一Windows user権限、Credential Manager侵害、OpenAI account侵害 |
| R25 | remote ChatGPTが誤操作またはprompt injectionで書き込む | 研究内容の不正確な変更 | developer modeを明示、active Vaultだけ、全writeはcore API・immutable version・row version競合検知を通す。delete/restore toolは非公開 | 利用者が承認した誤変更。ChatGPTへ取得された情報はOpenAI側の処理対象になる |

## 自己レビューで修正した設計上の穴

1. **autosave だけでは crash window が長い** — 通常 version 保存とは別に短い debounce の `block_drafts` を追加した。
2. **soft delete が復元不能だと実質破壊的** — Trash 一覧と undelete transaction を MVP に追加した。
3. **圧縮成功だけでは Snapshot の健全性を証明しない** — zstd を読み戻し、byte size/hash/DB/履歴を検証してから公開する。
4. **JSON hash だけあっても検査しなければ意味がない** — open、integrity、snapshot 作成で全 version hash と current projection を照合する。
5. **アプリ API が不変でも SQLite を直接触れば履歴を壊せる** — update/delete 禁止 trigger を追加し、外部改変は integrity で fatal にする。
6. **暗号鍵が error path に残りうる** — 派生鍵を zeroizing buffer に限定する。
7. **backup restore の展開量攻撃** — encrypted size と manifest size に上限・空き容量 guard を置く。
8. **存在確認とrenameの間に競合できる** — Linux `RENAME_NOREPLACE`、macOS `RENAME_EXCL`、Windows no-replace moveでpublishする。
9. **知らないSQLiteをmigrationしてしまう** — database headerのapplication IDをmigrationより先に検査する。

## Crash point ごとの期待結果

| Crash point | 再起動後 |
|---|---|
| recovery draft transaction 前 | 最後に commit 済み version。最大約300msの入力は失いうる |
| draft commit 後、正式 save 前 | recovery prompt から本文を救出可能 |
| block transaction 中 | transaction 全体 rollback、直前 version が current |
| block commit 後、journal 書込前 | outbox が残り open 時に journal を生成 |
| object staging 中 | DB relation なし。staging 残骸を隔離可能 |
| object rename 後、relation commit 前 | CAS orphan。データ破壊ではなく余分な安全コピー |
| snapshot staging 中 | 公開一覧に出ない。live Vault は不変 |
| backup 暗号化途中 | `.partial` のみ。成功 record なし |
| restore staging 中 | destination は存在しない。source と backup は不変 |

## 復旧可能性の判定

「backup がある」はファイル名の存在では判定しない。次の全条件が成立した時だけ verified とする。

- header と AEAD tag が全 chunk で正しい
- archive path が allowlist 内で、link/重複がない
- Vault manifest と DB identity/schema/revision が一致
- SQLite `quick_check = ok` かつ foreign key violation がゼロ
- immutable version hash と current projection が一致
- manifest が列挙する object がすべて存在し SHA-256 が一致

## 運用上の推奨

- 作業端末とは別の媒体へ暗号化 backup を作る。
- backup password は Vault と別の password manager に保管する。
- 月次で restore clone を実行し、開けることまで確認する。
- live Vault はローカル SSD と OS full-disk encryption 上に置く。
- Research Integrity の fatal finding がある状態では新 Snapshot/Backup を作らず、良好な過去コピーを保全する。
