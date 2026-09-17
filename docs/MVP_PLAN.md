# Phase 1 実装計画と受入条件

| 順序 | Slice | 受入条件 |
|---:|---|---|
| 1 | Vault/DB | atomic create、exclusive lock、WAL/FULL/FK、missing DB は再生成しない |
| 2 | Block/History | create/save/restore/branch、stale save rejection、不変履歴 |
| 3 | Recovery draft | crash 後に候補を提示し、明示保存または破棄できる |
| 4 | Graph | 9 semantic edge、drag position、filter/search、soft delete |
| 5 | Files | multi attachment、relation type、CAS dedup、hash mismatch 検出 |
| 6 | Research metadata | variable conflict、citation、FTS search |
| 7 | Integrity | unsupported、rejected dependency、orphan、broken citation/file、DB/history/backup 状態 |
| 8 | Snapshot | manual/10m/daily/weekly、readback 検証、clone restore |
| 9 | Backup | self-contained encrypted archive、readback 検証、clone restore |
| 10 | Desktop UI | Home、3-pane workspace、Markdown/KaTeX、Graph、Inspector、Trash/Recovery |
| 11 | Verification | Rust safety tests、frontend tests、production build、manual recovery runbook |

## Definition of done

- モック API を本番 UI の成功経路に使わない。
- browser-only 起動時は明瞭な demo 表示とし、永続保存済みと表示しない。
- 失敗を握りつぶさず、保存状態を `Saving / Saved / Conflict / Recovery available / Error` で表示する。
- destructive operation は対象名の確認を要求する。
- export は Markdown/JSON/LaTeX/ZIP の拡張点を data model に確保する。Phase 1 完了後の slice として追跡する。
- 全コードと schema、設計、安全性レビュー、復旧 runbook を同梱する。
