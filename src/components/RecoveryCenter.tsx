import { useEffect, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import { blockKindLabel, formatDateTime, snapshotKindLabel } from "../labels";
import type { BackupRecord, HypothesisBlock, SnapshotRecord } from "../types";

interface Props {
  onBlocksChanged: () => Promise<void>;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
}

export default function RecoveryCenter({ onBlocksChanged, onError, onNotice }: Props) {
  const [snapshots, setSnapshots] = useState<SnapshotRecord[]>([]);
  const [backups, setBackups] = useState<BackupRecord[]>([]);
  const [trash, setTrash] = useState<HypothesisBlock[]>([]);
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState<string | null>(null);

  const reload = async () => {
    try {
      const [nextSnapshots, nextBackups, nextTrash] = await Promise.all([api.snapshots(), api.backups(), api.listDeletedBlocks()]);
      setSnapshots(nextSnapshots); setBackups(nextBackups); setTrash(nextTrash);
    } catch (cause) { onError(cause instanceof Error ? cause.message : String(cause)); }
  };

  useEffect(() => { void reload(); }, []);

  const createSnapshot = async () => {
    setBusy("snapshot");
    try { const result = await api.createSnapshot("Manual"); onNotice(`リビジョン${result.revision}のSnapshotを作成した`); await reload(); }
    catch (cause) { onError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setBusy(null); }
  };

  const createBackup = async () => {
    if (password.length < 12) { onError("Backupのパスワードは12文字以上にして"); return; }
    const destination = await save({ title: "暗号化Backupの保存先", defaultPath: `Sanctum-${new Date().toISOString().slice(0, 10)}.sanctum-backup`, filters: [{ name: "Sanctum backup", extensions: ["sanctum-backup"] }] });
    if (!destination) return;
    setBusy("backup");
    try { const result = await api.createBackup(destination, password); setPassword(""); onNotice(`暗号化Backupを作成・検証した: ${result.fileName}`); await reload(); }
    catch (cause) { onError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setBusy(null); }
  };

  const restoreSnapshot = async (snapshot: SnapshotRecord) => {
    const destination = await save({ title: "復元先の新しいVault名", defaultPath: `Recovered-r${snapshot.revision}.sanctum` });
    if (!destination) return;
    setBusy(snapshot.id);
    try { await api.restoreSnapshot(snapshot.id, destination); onNotice(`新しいVaultとして復元した: ${destination}`); }
    catch (cause) { onError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setBusy(null); }
  };

  const verifyExternal = async () => {
    if (password.length < 12) { onError("先にBackupのパスワードを入力して"); return; }
    const archive = await open({ multiple: false, directory: false, title: "検証する暗号化Backupを選択", filters: [{ name: "Sanctum backup", extensions: ["sanctum-backup"] }] });
    if (typeof archive !== "string") return;
    setBusy("verify");
    try { await api.verifyBackup(archive, password); onNotice("Backupの検証に成功した"); }
    catch (cause) { onError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setBusy(null); }
  };

  const restoreTrash = async (block: HypothesisBlock) => {
    setBusy(block.id);
    try { await api.restoreDeletedBlock(block.id, block.rowVersion); await Promise.all([reload(), onBlocksChanged()]); onNotice(`「${block.title}」を復元した`); }
    catch (cause) { onError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setBusy(null); }
  };

  return (
    <section className="recovery-page">
      <header className="page-header"><div><h2>復旧</h2></div><button className="button secondary" onClick={() => void reload()}>更新</button></header>

      <div className="recovery-principle"><div><strong>同期とBackupは別</strong><p>端末故障に備えるには、暗号化Backupを別の場所へ保存する</p></div></div>

      <div className="recovery-grid">
        <section className="recovery-card">
          <header><div><h3>Snapshot</h3></div></header>
          <button className="button secondary full" disabled={Boolean(busy)} onClick={() => void createSnapshot()}>{busy === "snapshot" ? "作成中" : "Snapshotを作成"}</button>
          <div className="recovery-list">{snapshots.map((snapshot) => <article key={snapshot.id}><div><strong>{snapshotKindLabel[snapshot.kind]}</strong><span>{formatDateTime(snapshot.createdAt)} · リビジョン {snapshot.revision}</span><code>{snapshot.databaseSha256.slice(0, 14)}…</code></div><button className="button ghost compact" disabled={Boolean(busy)} onClick={() => void restoreSnapshot(snapshot)}>{busy === snapshot.id ? "復元中" : "復元"}</button></article>)}{!snapshots.length && <p className="empty-row">Snapshotはまだない</p>}</div>
          <p className="recovery-caveat">同じ端末内。端末故障には外部Backupが必要</p>
        </section>

        <section className="recovery-card featured">
          <header><div><h3>暗号化Backup</h3></div></header>
          <label className="password-label">パスワード<input type="password" autoComplete="new-password" value={password} onChange={(event) => setPassword(event.target.value)} placeholder="12文字以上" /></label>
          <div className="dual-actions"><button className="button primary" disabled={Boolean(busy) || password.length < 12} onClick={() => void createBackup()}>{busy === "backup" ? "作成中" : "作成"}</button><button className="button secondary" disabled={Boolean(busy) || password.length < 12} onClick={() => void verifyExternal()}>{busy === "verify" ? "検証中" : "検証"}</button></div>
          <div className="recovery-list">{backups.map((backup) => <article key={backup.id}><div><strong>{backup.fileName}</strong><span>{formatDateTime(backup.createdAt)} · {formatBytes(backup.byteSize)}</span><code>{backup.archiveSha256.slice(0, 14)}…</code></div></article>)}{!backups.length && <p className="empty-row">Backupはまだない</p>}</div>
        </section>
      </div>

      <section className="trash-section">
        <header><div><div><h3>ゴミ箱</h3></div></div><span>{trash.length}</span></header>
        <div className="trash-list">{trash.map((block) => <article key={block.id}><div><strong>{block.title}</strong><span>{blockKindLabel[block.kind]} · {block.deletedAt ? formatDateTime(block.deletedAt) : "—"}</span><code>{block.id}</code></div><button className="button secondary compact" disabled={Boolean(busy)} onClick={() => void restoreTrash(block)}>{busy === block.id ? "復元中" : "復元"}</button></article>)}{!trash.length && <p className="empty-row">空</p>}</div>
      </section>
    </section>
  );
}

function formatBytes(bytes: number) {
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
}
