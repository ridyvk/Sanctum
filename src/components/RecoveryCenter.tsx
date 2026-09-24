import { useEffect, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { api, isAndroidRuntime } from "../api";
import { blockKindLabel, formatDateTime, snapshotKindLabel } from "../labels";
import { saveMobileFile } from "../mobileFiles";
import type { AutomaticBackupStatus, BackupRecord, HypothesisBlock, SnapshotRecord } from "../types";

interface Props {
  onBlocksChanged: () => Promise<void>;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
  onIntegrity?: () => void;
}

export default function RecoveryCenter(props: Props) {
  return isAndroidRuntime() ? <MobileRecoveryCenter {...props} /> : <DesktopRecoveryCenter {...props} />;
}

function DesktopRecoveryCenter({ onBlocksChanged, onError, onNotice, onIntegrity }: Props) {
  const [snapshots, setSnapshots] = useState<SnapshotRecord[]>([]);
  const [backups, setBackups] = useState<BackupRecord[]>([]);
  const [trash, setTrash] = useState<HypothesisBlock[]>([]);
  const [automatic, setAutomatic] = useState<AutomaticBackupStatus | null>(null);
  const [password, setPassword] = useState("");
  const [automaticPassword, setAutomaticPassword] = useState("");
  const [automaticDirectory, setAutomaticDirectory] = useState("");
  const [busy, setBusy] = useState<string | null>(null);

  const reload = async () => {
    try {
      const [nextSnapshots, nextBackups, nextTrash, nextAutomatic] = await Promise.all([
        api.snapshots(), api.backups(), api.listDeletedBlocks(), api.automaticBackupStatus(),
      ]);
      setSnapshots(nextSnapshots);
      setBackups(nextBackups);
      setTrash(nextTrash);
      setAutomatic(nextAutomatic);
      setAutomaticDirectory((current) => current || nextAutomatic.config.destinationDirectory);
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
    try { const result = await api.createBackup(destination, password); setPassword(""); onNotice(`Backupを作成・検証した: ${result.fileName}`); await reload(); }
    catch (cause) { onError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setBusy(null); }
  };

  const chooseAutomaticDirectory = async () => {
    const selected = await open({ multiple: false, directory: true, title: "自動Backupの保存先" });
    if (typeof selected === "string") setAutomaticDirectory(selected);
  };

  const configureAutomatic = async () => {
    if (!automaticDirectory) { onError("自動Backupの保存先を選んで"); return; }
    if (automaticPassword.length < 12) { onError("自動Backupのパスワードは12文字以上にして"); return; }
    setBusy("automatic");
    try {
      setAutomatic(await api.configureAutomaticBackup(automaticDirectory, automaticPassword));
      setAutomaticPassword("");
      onNotice("自動Backupを有効にした");
      const created = await api.runDueAutomaticBackup();
      if (created) onNotice(`最初の自動Backupを作成した: ${created.fileName}`);
      await reload();
    } catch (cause) { onError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setBusy(null); }
  };

  const disableAutomatic = async () => {
    setBusy("automatic");
    try { setAutomatic(await api.disableAutomaticBackup()); onNotice("自動Backupを停止した"); }
    catch (cause) { onError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setBusy(null); }
  };

  const exportPortable = async () => {
    const destination = await open({ multiple: false, directory: true, title: "エクスポート先の親フォルダ" });
    if (typeof destination !== "string") return;
    setBusy("export");
    try {
      const result = await api.exportPortable(destination);
      onNotice(`完全エクスポートを作成した: ${result.destinationPath}`);
    } catch (cause) { onError(cause instanceof Error ? cause.message : String(cause)); }
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
      <header className="page-header"><h2>データ</h2><div className="inline-actions">{onIntegrity && <button className="button secondary mobile-only" onClick={onIntegrity}>整合性検査</button>}<button className="button secondary" onClick={() => void reload()}>更新</button></div></header>

      <section className="export-card">
        <div><h3>完全エクスポート</h3><span>Markdown・添付・BibTeX・関係・変数・ハッシュ</span></div>
        <button className="button primary" disabled={Boolean(busy)} onClick={() => void exportPortable()}>{busy === "export" ? "書出中" : "書き出す"}</button>
      </section>

      <div className="recovery-grid">
        <section className="recovery-card featured">
          <header><h3>自動Backup</h3></header>
          {automatic?.config.enabled ? <div className="automatic-backup-active"><strong>有効</strong><span title={automatic.config.destinationDirectory}>{automatic.config.destinationDirectory}</span><span>{automatic.config.lastSuccessAt ? `最終 ${formatDateTime(automatic.config.lastSuccessAt)}` : "初回作成待ち"}</span><button className="button secondary full" disabled={Boolean(busy)} onClick={() => void disableAutomatic()}>停止</button></div> : <div className="automatic-backup-form"><button className="directory-field" onClick={() => void chooseAutomaticDirectory()}>{automaticDirectory || "保存先を選ぶ"}</button><label className="password-label">パスワード<input type="password" autoComplete="new-password" value={automaticPassword} onChange={(event) => setAutomaticPassword(event.target.value)} placeholder="12文字以上" /></label><button className="button primary full" disabled={Boolean(busy) || !automaticDirectory || automaticPassword.length < 12} onClick={() => void configureAutomatic()}>{busy === "automatic" ? "設定中" : "有効にする"}</button></div>}
        </section>

        <section className="recovery-card">
          <header><h3>手動Backup</h3></header>
          <label className="password-label">パスワード<input type="password" autoComplete="new-password" value={password} onChange={(event) => setPassword(event.target.value)} placeholder="12文字以上" /></label>
          <div className="dual-actions"><button className="button primary" disabled={Boolean(busy) || password.length < 12} onClick={() => void createBackup()}>{busy === "backup" ? "作成中" : "作成"}</button><button className="button secondary" disabled={Boolean(busy) || password.length < 12} onClick={() => void verifyExternal()}>{busy === "verify" ? "検証中" : "検証"}</button></div>
          <div className="recovery-list">{backups.slice(0, 5).map((backup) => <article key={backup.id}><div><strong>{backup.fileName}</strong><span>{formatDateTime(backup.createdAt)} · {formatBytes(backup.byteSize)}</span></div></article>)}{!backups.length && <p className="empty-row">Backupはまだない</p>}</div>
        </section>
      </div>

      <details className="advanced-recovery">
        <summary>復旧とゴミ箱</summary>
        <div className="recovery-grid advanced-grid">
          <section className="recovery-card">
            <header><h3>Snapshot</h3></header>
            <button className="button secondary full" disabled={Boolean(busy)} onClick={() => void createSnapshot()}>{busy === "snapshot" ? "作成中" : "作成"}</button>
            <div className="recovery-list">{snapshots.map((snapshot) => <article key={snapshot.id}><div><strong>{snapshotKindLabel[snapshot.kind]}</strong><span>{formatDateTime(snapshot.createdAt)} · リビジョン {snapshot.revision}</span></div><button className="button ghost compact" disabled={Boolean(busy)} onClick={() => void restoreSnapshot(snapshot)}>{busy === snapshot.id ? "復元中" : "復元"}</button></article>)}{!snapshots.length && <p className="empty-row">Snapshotはまだない</p>}</div>
          </section>
          <section className="recovery-card">
            <header><h3>ゴミ箱</h3><span>{trash.length}</span></header>
            <div className="trash-list">{trash.map((block) => <article key={block.id}><div><strong>{block.title}</strong><span>{blockKindLabel[block.kind]} · {block.deletedAt ? formatDateTime(block.deletedAt) : "—"}</span></div><button className="button secondary compact" disabled={Boolean(busy)} onClick={() => void restoreTrash(block)}>{busy === block.id ? "復元中" : "復元"}</button></article>)}{!trash.length && <p className="empty-row">空</p>}</div>
          </section>
        </div>
        <p className="recovery-caveat">Snapshotは同じ端末内。端末故障に備えるには外部Backupを使う</p>
      </details>
    </section>
  );
}

function MobileRecoveryCenter({ onBlocksChanged, onError, onNotice, onIntegrity }: Props) {
  const [snapshots, setSnapshots] = useState<SnapshotRecord[]>([]);
  const [trash, setTrash] = useState<HypothesisBlock[]>([]);
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState<string | null>(null);

  const reload = async () => {
    try {
      const [nextSnapshots, nextTrash] = await Promise.all([api.snapshots(), api.listDeletedBlocks()]);
      setSnapshots(nextSnapshots);
      setTrash(nextTrash);
    } catch (cause) { onError(String(cause)); }
  };
  useEffect(() => { void reload(); }, []);

  const backup = async () => {
    setBusy("backup");
    try {
      const record = await api.createMobileBackup(password);
      try {
        const saved = await saveMobileFile(record.destinationPath, record.fileName);
        if (saved) { setPassword(""); onNotice("暗号化Backupを書き出して検証した"); }
      } finally { await api.discardMobileExport(record.destinationPath); }
      await reload();
    } catch (cause) { onError(String(cause)); }
    finally { setBusy(null); }
  };

  const snapshot = async () => {
    setBusy("snapshot");
    try { await api.createSnapshot("Manual"); await reload(); onNotice("Snapshotを作成した"); }
    catch (cause) { onError(String(cause)); }
    finally { setBusy(null); }
  };

  const restoreSnapshot = async (item: SnapshotRecord) => {
    setBusy(item.id);
    try {
      const restored = await api.restoreMobileSnapshot(item.id);
      onNotice(`「${restored.name}」を新しいVaultとして復元した。ホームから開ける`);
    } catch (cause) { onError(String(cause)); }
    finally { setBusy(null); }
  };

  const restoreTrash = async (block: HypothesisBlock) => {
    setBusy(block.id);
    try {
      await api.restoreDeletedBlock(block.id, block.rowVersion);
      await Promise.all([reload(), onBlocksChanged()]);
      onNotice(`「${block.title}」を復元した`);
    } catch (cause) { onError(String(cause)); }
    finally { setBusy(null); }
  };

  return <section className="recovery-page mobile-recovery">
    <header className="page-header"><h2>データ</h2><div className="inline-actions">{onIntegrity && <button className="button secondary" onClick={onIntegrity}>整合性検査</button>}<button className="button secondary" onClick={() => void reload()}>更新</button></div></header>
    <section className="recovery-card featured">
      <header><h3>暗号化Backup</h3></header>
      <p className="muted">端末の外へ保存する。PC版でも読み込める</p>
      <label className="password-label">パスワード<input type="password" autoComplete="new-password" value={password} onChange={(event) => setPassword(event.target.value)} placeholder="12文字以上" /></label>
      <button className="button primary full" disabled={Boolean(busy) || password.length < 12} onClick={() => void backup()}>{busy === "backup" ? "書き出し中" : "Backupを書き出す"}</button>
    </section>
    <div className="recovery-grid">
      <section className="recovery-card">
        <header><h3>Snapshot</h3></header>
        <button className="button secondary full" disabled={Boolean(busy)} onClick={() => void snapshot()}>{busy === "snapshot" ? "作成中" : "作成"}</button>
        <div className="recovery-list">{snapshots.map((item) => <article key={item.id}><div><strong>{snapshotKindLabel[item.kind]}</strong><span>{formatDateTime(item.createdAt)} · リビジョン {item.revision}</span></div><button className="button secondary compact" disabled={Boolean(busy)} onClick={() => void restoreSnapshot(item)}>復元</button></article>)}{!snapshots.length && <p className="empty-row">Snapshotはまだない</p>}</div>
      </section>
      <section className="recovery-card">
        <header><h3>ゴミ箱</h3><span>{trash.length}</span></header>
        <div className="trash-list">{trash.map((block) => <article key={block.id}><div><strong>{block.title}</strong><span>{blockKindLabel[block.kind]}</span></div><button className="button secondary compact" disabled={Boolean(busy)} onClick={() => void restoreTrash(block)}>復元</button></article>)}{!trash.length && <p className="empty-row">空</p>}</div>
      </section>
    </div>
    <p className="recovery-caveat">端末を削除・初期化すると、書き出していないVaultは失われる。Snapshotは同じ端末内に保存される</p>
  </section>;
}

function formatBytes(bytes: number) {
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
}
