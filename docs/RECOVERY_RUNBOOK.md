# Sanctum 復旧 Runbook

## 最初にしないこと

- 壊れた可能性がある Vault を削除・上書きしない。
- `.sqlite-wal` / `.sqlite-shm` を手作業で切り離さない。
- 唯一の backup に直接 restore しない。
- CAS object を重複に見えても手動削除しない。

## 1. 編集 crash から戻す

1. 同じ Vault を通常どおり開く。
2. `Recovery draft available` が表示された Block を確認する。
3. 内容を比較し、`Save recovered draft` で新 version として commit、または `Discard` する。
4. Research Integrity を実行する。

## 2. Soft-delete を戻す

1. Workspace の `Trash` を開く。
2. Block ID とタイトルを確認する。
3. `Restore` を実行する。過去 version は削除時も常に残っている。

## 3. Snapshot を検証して clone 復元する

1. Snapshots で対象時刻と revision を選ぶ。
2. `Verify` を実行する。
3. 現行 Vault と異なる、空の destination を指定する。
4. `Restore clone` を実行する。
5. clone を開き、Integrity と必要な Block/version/file を確認する。
6. 現行 Vault の扱いを決めるのは確認後にする。

## 4. Encrypted Backup から戻す

1. backup file を読み取り専用の安全な場所へ複製する。
2. 新しい destination と password を指定する。
3. Sanctum は復号・archive 検査・DB/履歴/object 検証を行う。
4. 検証が一つでも失敗すれば destination は公開されない。
5. 成功した clone を開き、Integrity を再実行する。

## 5. Integrity が fatal の場合

1. 現行 Vault を閉じる。
2. ディレクトリ全体を別媒体へ byte-for-byte 保全する。
3. 最後に verified だった Snapshot/Backup を clone restore する。
4. clone と保全コピーを比較し、欠けた直近変更は手作業で新 version として取り込む。
5. 原因が不明なまま壊れた Vault へ書き込みを再開しない。

## 定期訓練

月に一度、最新 external backup を一時 clone に restore し、DB・添付・代表的な version を開けることを確認する。「作成できた」ではなく「復元できた」を backup の成功基準にする。
