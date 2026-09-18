import { useMemo, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { api, isDesktopRuntime } from "../api";
import { formatDate } from "../labels";
import { loadRecentVaults, rememberVault, type RecentVault } from "../recentVaults";
import type { VaultSummary } from "../types";
import AppUpdater from "./AppUpdater";

interface Props {
  onOpened: (summary: VaultSummary) => void;
}

type Modal = "new" | "restore" | null;

export default function Home({ onOpened }: Props) {
  const desktop = isDesktopRuntime();
  const [recent, setRecent] = useState<RecentVault[]>(() => loadRecentVaults());
  const [modal, setModal] = useState<Modal>(null);
  const [name, setName] = useState("");
  const [parentPath, setParentPath] = useState("");
  const [archivePath, setArchivePath] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [updating, setUpdating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const safeFolderName = useMemo(
    () => name.trim().replace(/[\\/:*?"<>|]/g, "-").replace(/\s+/g, " "),
    [name],
  );

  const chooseParent = async () => {
    const selected = await open({ directory: true, multiple: false, title: "Vaultの保存先を選択" });
    if (typeof selected === "string") setParentPath(selected);
  };

  const chooseArchive = async () => {
    const selected = await open({
      multiple: false,
      directory: false,
      title: "暗号化Backupを選択",
      filters: [{ name: "Sanctum backup", extensions: ["sanctum-backup"] }],
    });
    if (typeof selected === "string") setArchivePath(selected);
  };

  const createVault = async () => {
    if (!safeFolderName || !parentPath) return;
    setBusy(true);
    setError(null);
    try {
      const summary = await api.createVault(`${parentPath}/${safeFolderName}.sanctum`, name.trim());
      setRecent(rememberVault(summary));
      onOpened(summary);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  const openPath = async (path?: string) => {
    setBusy(true);
    setError(null);
    try {
      const selected = path ?? (await open({ directory: true, multiple: false, title: ".sanctum Vaultを開く" }));
      if (typeof selected !== "string") return;
      const summary = await api.openVault(selected);
      setRecent(rememberVault(summary));
      onOpened(summary);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  const restoreBackup = async () => {
    if (!archivePath || password.length < 12) return;
    const destination = await save({
      title: "復元先の新しいVault名を指定",
      defaultPath: `${name.trim() || "Recovered"}.sanctum`,
    });
    if (!destination) return;
    setBusy(true);
    setError(null);
    try {
      const restoredPath = await api.restoreBackup(archivePath, password, destination);
      const summary = await api.openVault(restoredPath);
      setRecent(rememberVault(summary));
      onOpened(summary);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <main className="home">
      <header className="home-hero">
        <h1>SANCTUM</h1>
      </header>

      {!desktop && (
        <div className="runtime-notice" role="status">
          <div>
            <strong>ブラウザ表示</strong>
            <span>保存機能はデスクトップ版でのみ利用できる</span>
          </div>
        </div>
      )}

      <section className="projects-section" aria-labelledby="projects-heading">
        <div className="section-heading-row">
          <div>
            <h2 id="projects-heading">研究</h2>
          </div>
          <div className="home-actions">
            <button className="button secondary" disabled={!desktop || busy || updating} onClick={() => void openPath()}>
              Vaultを開く
            </button>
            <button className="button primary" disabled={!desktop || busy || updating} onClick={() => { setError(null); setModal("new"); }}>
              新規作成
            </button>
          </div>
        </div>

        {error && <div className="error-banner" role="alert">{error}</div>}

        {recent.length ? (
          <div className="project-grid">
            {recent.map((project) => (
              <button className="project-card" key={`${project.vaultId}-${project.path}`} onClick={() => void openPath(project.path)} disabled={!desktop || busy || updating}>
                <h3>{project.name}</h3>
                <p>{project.path}</p>
                <time>{formatDate(project.lastOpenedAt)}</time>
              </button>
            ))}
          </div>
        ) : (
          <div className="empty-projects">
            <h3>研究はまだない</h3>
          </div>
        )}

        <div className="home-footer">
          <button className="restore-link" disabled={!desktop || busy || updating} onClick={() => { setError(null); setModal("restore"); }}>
            暗号化Backupから復元
          </button>
          <AppUpdater desktop={desktop} onInstallStateChange={setUpdating} />
        </div>
      </section>

      {modal && (
        <div className="modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) setModal(null); }}>
          <section className="modal" role="dialog" aria-modal="true" aria-labelledby="modal-title">
            <button className="text-button modal-close" onClick={() => setModal(null)}>閉じる</button>
            {modal === "new" ? (
              <>
                <h2 id="modal-title">新しいSanctum</h2>
                <label>研究テーマ<input autoFocus value={name} onChange={(event) => setName(event.target.value)} placeholder="無期限寿命の経済学" /></label>
                <label>保存先<div className="path-picker"><input readOnly value={parentPath} placeholder="ローカルフォルダを選択" /><button className="button secondary" onClick={() => void chooseParent()}>選択</button></div></label>
                <div className="modal-actions"><button className="button ghost" onClick={() => setModal(null)}>キャンセル</button><button className="button primary" disabled={!safeFolderName || !parentPath || busy} onClick={() => void createVault()}>{busy ? "作成中" : "作成"}</button></div>
              </>
            ) : (
              <>
                <h2 id="modal-title">Backupから復元</h2>
                <p className="muted">既存のVaultは上書きせず、新しいVaultとして復元する</p>
                <label>暗号化Backup<div className="path-picker"><input readOnly value={archivePath} placeholder=".sanctum-backupを選択" /><button className="button secondary" onClick={() => void chooseArchive()}>選択</button></div></label>
                <label>パスワード<input type="password" value={password} onChange={(event) => setPassword(event.target.value)} autoComplete="off" placeholder="12文字以上" /></label>
                <div className="modal-actions"><button className="button ghost" onClick={() => setModal(null)}>キャンセル</button><button className="button primary" disabled={!archivePath || password.length < 12 || busy} onClick={() => void restoreBackup()}>{busy ? "検証中" : "復元先を選択"}</button></div>
              </>
            )}
          </section>
        </div>
      )}
    </main>
  );
}
