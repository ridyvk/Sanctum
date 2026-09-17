# Sanctum 自動更新の公開手順

## 初回だけ必要な設定

GitHubの`ridyvk/Sanctum`を開き、`Settings` → `Secrets and variables` → `Actions` → `New repository secret`へ進む。

- Name: `TAURI_SIGNING_PRIVATE_KEY`
- Secret: 保管してある`Sanctum-Updater-Signing.key`の全文

秘密鍵はGitHub ActionsのSecret以外へ貼らず、リポジトリへcommitしない。アプリとリポジトリには検証用の公開鍵だけを置く。秘密鍵を失うと既存アプリへ同じ更新経路で配布できなくなるため、安全な場所にもバックアップする。

## リリース

1. `main`でテストを通し、`package.json`、`Cargo.toml`、`src-tauri/tauri.conf.json`のversionを同じ値へ上げる。
2. 検証済みの`main`を`release`ブランチへ反映する。
3. GitHub Actionsの`publish-windows-update`がWindows上で再テストし、NSISインストーラー、署名、`latest.json`をGitHub Releaseへ公開する。
4. 公開後、旧版Sanctumのホーム画面で更新を確認し、ダウンロード・署名検証・インストール・再起動を実機確認する。

公開鍵を変更すると既存版は新しい更新を検証できない。鍵のローテーションは、旧公開鍵で署名した移行版を先に配布してから行う。
