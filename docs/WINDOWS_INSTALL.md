# Sanctum Windows インストール

## 対象

- 64-bit Windows 10 / 11
- ファイル: `Sanctum-Setup-0.2.0-x64.exe`

## インストール

1. インストーラーをダブルクリックする。
2. 未署名版で SmartScreen が表示された場合だけ、「詳細情報」から「実行」を選ぶ。
3. インストール完了後、スタートメニューまたはデスクトップの Sanctum を開く。

インストール先は現在のWindowsアカウント配下。管理者権限は不要。

## 最初のVault

テスト用の空フォルダを指定してSanctumを作成し、仮説ブロックを一つ保存する。その後、Sanctumを終了・再起動し、同じVaultを開いて保存内容が残っていることを確認する。本物の研究データを入れる前に、Snapshot作成、暗号化Backup、別フォルダへのRestoreも一度試す。

## 完全性確認

PowerShellで次を実行し、配布時に示されたSHA-256と一致することを確認する。

```powershell
Get-FileHash .\Sanctum-Setup-0.2.0-x64.exe -Algorithm SHA256
```

## 現在の注意点

- Phase 1 installerはcode-signing certificateで未署名。
- Vault本体はPhase 1ではat-rest暗号化されない。BitLockerなどOSのディスク暗号化を使う。
- live VaultをOneDrive等の同期フォルダやネットワークドライブへ直接置かない。外部Backupの保存先として使う。

## 0.1.1から更新する場合

0.1.1は本番画面を同梱していても、起動時に開発用URL `localhost:1420` を参照する梱包不良がある。ファイアウォールやプロキシは変更せず、Sanctumを終了して旧版をアンインストールした後、最新版をインストールする。
