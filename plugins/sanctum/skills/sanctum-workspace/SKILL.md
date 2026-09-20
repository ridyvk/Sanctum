---
name: sanctum-workspace
description: Sanctumで開いている研究Vaultの検索、読取り、仮説ブロックの作成・編集、添付追加、整合性確認を行う。
---

# Sanctum Workspace

Sanctumの内容を扱う依頼では、最初に`sanctum_status`で接続中のVaultを確認する。Vaultが開いていなければ、Sanctumデスクトップで対象Vaultを開くよう短く案内する。

## 読取り

- 概要把握には`sanctum_list_blocks`、語句やテーマの探索には`sanctum_search`を使う。
- 本文、履歴、添付、引用が必要なときだけ`sanctum_get_block`を使う。
- テキスト添付の内容が必要なときは、添付IDを確認してから`sanctum_read_text_attachment`を使う。PDFや画像の本文を読めるとは扱わない。
- 関係性の分析には`sanctum_get_graph`を使う。
- 長い本文を不必要に全文取得せず、依頼に必要な範囲へ絞る。

## 変更

- 新規作成では、ユーザーの内容を保ち、勝手な主張や出典を追加しない。
- 既存ブロックの編集前に必ず`sanctum_get_block`で最新版を読み、返された`rowVersion`を`expectedRowVersion`へ渡す。
- `changeReason`には変更内容を短く具体的に記録する。
- 競合が返ったら自動で上書きしない。最新版を読み直し、差分をユーザーへ説明してから次の操作を決める。
- 添付追加は、ユーザーが対象ファイルと追加先を明示した場合だけ行う。

削除、履歴復元、Backup復元はこのプラグインでは行わない。必要な場合はSanctumデスクトップの画面を使うよう案内する。
