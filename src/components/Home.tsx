import { useEffect, useMemo, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { readFile, writeFile } from "@tauri-apps/plugin-fs";
import { api, isAndroidRuntime, isDesktopRuntime } from "../api";
import { formatDate } from "../labels";
import { loadRecentVaults, rememberVault, type RecentVault } from "../recentVaults";
import type { ChatGptPluginStatus, ChatGptTunnelStatus, VaultSummary } from "../types";
import AppUpdater from "./AppUpdater";

interface Props {
  onOpened: (summary: VaultSummary) => void;
}

type Modal = "new" | "restore" | "chatgpt" | null;
type ChatGptMode = "chatgpt" | "codex";

export default function Home({ onOpened }: Props) {
  const android = isAndroidRuntime();
  const desktop = isDesktopRuntime() && !android;
  const [recent, setRecent] = useState<RecentVault[]>(() => loadRecentVaults());
  const [modal, setModal] = useState<Modal>(null);
  const [name, setName] = useState("");
  const [parentPath, setParentPath] = useState("");
  const [archivePath, setArchivePath] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [updating, setUpdating] = useState(false);
  const [pluginBusy, setPluginBusy] = useState(false);
  const [pluginStatus, setPluginStatus] = useState<ChatGptPluginStatus | null>(null);
  const [chatGptMode, setChatGptMode] = useState<ChatGptMode>("chatgpt");
  const [tunnelStatus, setTunnelStatus] = useState<ChatGptTunnelStatus | null>(null);
  const [tunnelId, setTunnelId] = useState("");
  const [runtimeApiKey, setRuntimeApiKey] = useState("");
  const [tunnelClientPath, setTunnelClientPath] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [mobileLoading, setMobileLoading] = useState(android);

  useEffect(() => {
    if (!android) return;
    let cancelled = false;
    void api.listMobileVaults().then((vaults) => {
      if (cancelled) return;
      const previous = loadRecentVaults();
      setRecent(vaults.map((vault) => ({
        vaultId: vault.vaultId,
        name: vault.name,
        path: vault.path,
        lastOpenedAt: previous.find((entry) => entry.path === vault.path)?.lastOpenedAt ?? vault.createdAt,
      })));
    }).catch((cause) => { if (!cancelled) setError(String(cause)); })
      .finally(() => { if (!cancelled) setMobileLoading(false); });
    return () => { cancelled = true; };
  }, [android]);

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

  const chooseTunnelClient = async () => {
    const selected = await open({
      multiple: false,
      directory: false,
      title: "OpenAI tunnel-client.exeを選択",
      filters: [{ name: "Windows executable", extensions: ["exe"] }],
    });
    if (typeof selected === "string") setTunnelClientPath(selected);
  };

  const createVault = async () => {
    if (!safeFolderName || (!android && !parentPath)) return;
    setBusy(true);
    setError(null);
    try {
      const summary = android
        ? await api.createMobileVault(name.trim())
        : await api.createVault(`${parentPath}/${safeFolderName}.sanctum`, name.trim());
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
      const selected = path ?? (android ? null : await open({ directory: true, multiple: false, title: ".sanctum Vaultを開く" }));
      if (typeof selected !== "string") return;
      const summary = android ? await api.openMobileVault(selected) : await api.openVault(selected);
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
    if (android) {
      const fileName = "incoming.sanctum-backup";
      setBusy(true);
      setError(null);
      try {
        const contents = await readFile(archivePath);
        if (contents.byteLength > 64 * 1024 * 1024) throw new Error("64 MBを超えるBackupはこの版では読み込めない");
        const transfer = await api.prepareMobileImport(fileName);
        try {
          await writeFile(transfer.path, contents);
          const summary = await api.restoreMobileBackup(transfer.id, fileName, password);
          setRecent(rememberVault(summary));
          onOpened(summary);
        } finally {
          await api.discardMobileImport(transfer.id, fileName);
        }
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : String(cause));
      } finally {
        setBusy(false);
      }
      return;
    }
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

  const showChatGpt = async () => {
    setModal("chatgpt");
    setChatGptMode("chatgpt");
    setPluginBusy(true);
    setError(null);
    try {
      const [tunnel, plugin] = await Promise.all([
        api.chatGptTunnelStatus(),
        api.chatGptPluginStatus(),
      ]);
      setTunnelStatus(tunnel);
      setTunnelId(tunnel.tunnelId ?? "");
      setTunnelClientPath(tunnel.clientPath ?? "");
      setPluginStatus(plugin);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setPluginBusy(false);
    }
  };

  const configureTunnel = async () => {
    if (!tunnelId.trim() || !tunnelClientPath || (!runtimeApiKey && !tunnelStatus?.hasCredential)) return;
    setPluginBusy(true);
    setError(null);
    try {
      const status = await api.configureChatGptTunnel(
        tunnelId.trim(),
        runtimeApiKey,
        tunnelClientPath,
      );
      setTunnelStatus(status);
      setTunnelId(status.tunnelId ?? tunnelId.trim());
      setTunnelClientPath(status.clientPath ?? tunnelClientPath);
      setRuntimeApiKey("");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setPluginBusy(false);
    }
  };

  const setTunnelRunning = async (running: boolean) => {
    setPluginBusy(true);
    setError(null);
    try {
      setTunnelStatus(running ? await api.startChatGptTunnel() : await api.stopChatGptTunnel());
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setPluginBusy(false);
    }
  };

  const refreshTunnel = async () => {
    setPluginBusy(true);
    setError(null);
    try {
      setTunnelStatus(await api.chatGptTunnelStatus());
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setPluginBusy(false);
    }
  };

  const openTunnelSettings = async () => {
    setError(null);
    try {
      await api.openChatGptTunnelSettings();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  };

  const openChatGptConnectors = async () => {
    setError(null);
    try {
      await api.openChatGptConnectors();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  };

  const openTunnelAdmin = async () => {
    setError(null);
    try {
      await api.openChatGptTunnelAdmin();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  };

  const installChatGpt = async () => {
    setPluginBusy(true);
    setError(null);
    try {
      setPluginStatus(await api.installChatGptPlugin());
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setPluginBusy(false);
    }
  };

  const openChatGpt = async () => {
    setPluginBusy(true);
    setError(null);
    try {
      await api.openChatGptPlugin();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setPluginBusy(false);
    }
  };

  useEffect(() => {
    if (modal !== "chatgpt" || !tunnelStatus?.running || tunnelStatus.ready) return;
    const interval = window.setInterval(() => {
      void api.chatGptTunnelStatus().then(setTunnelStatus).catch(() => undefined);
    }, 1_500);
    return () => window.clearInterval(interval);
  }, [modal, tunnelStatus?.ready, tunnelStatus?.running]);

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
            <button className="button secondary" disabled={!isDesktopRuntime() || busy || updating} onClick={() => android ? (setError(null), setModal("restore")) : void openPath()}>
              {android ? "Backupを読み込む" : "Vaultを開く"}
            </button>
            <button className="button primary" disabled={!isDesktopRuntime() || busy || updating} onClick={() => { setError(null); setModal("new"); }}>
              新規作成
            </button>
          </div>
        </div>

        {error && <div className="error-banner" role="alert">{error}</div>}

        {mobileLoading ? <div className="center-message">研究を読み込み中</div> : recent.length ? (
          <div className="project-grid">
            {recent.map((project) => (
              <button className="project-card" key={`${project.vaultId}-${project.path}`} onClick={() => void openPath(project.path)} disabled={!isDesktopRuntime() || busy || updating}>
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
          <div className="home-footer-links">
            {desktop && <button className="restore-link" disabled={busy || updating} onClick={() => void showChatGpt()}>
              ChatGPT接続
            </button>}
            {!android && <button className="restore-link" disabled={!desktop || busy || updating} onClick={() => { setError(null); setModal("restore"); }}>
              暗号化Backupから復元
            </button>}
          </div>
          <AppUpdater desktop={desktop} onInstallStateChange={setUpdating} />
        </div>
      </section>

      {modal && (
        <div className="modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) setModal(null); }}>
          <section className={`modal${modal === "chatgpt" ? " connection-modal" : ""}`} role="dialog" aria-modal="true" aria-labelledby="modal-title">
            <button className="text-button modal-close" onClick={() => setModal(null)}>閉じる</button>
            {modal === "new" ? (
              <>
                <h2 id="modal-title">新しいSanctum</h2>
                <label>研究テーマ<input autoFocus value={name} onChange={(event) => setName(event.target.value)} placeholder="無期限寿命の経済学" /></label>
                {android ? <p className="muted">このスマホ内に保存する</p> : <label>保存先<div className="path-picker"><input readOnly value={parentPath} placeholder="ローカルフォルダを選択" /><button className="button secondary" onClick={() => void chooseParent()}>選択</button></div></label>}
                <div className="modal-actions"><button className="button ghost" onClick={() => setModal(null)}>キャンセル</button><button className="button primary" disabled={!safeFolderName || (!android && !parentPath) || busy} onClick={() => void createVault()}>{busy ? "作成中" : "作成"}</button></div>
              </>
            ) : modal === "restore" ? (
              <>
                <h2 id="modal-title">Backupから復元</h2>
                <p className="muted">既存のVaultは上書きせず、新しいVaultとして復元する</p>
                <label>暗号化Backup<div className="path-picker"><input readOnly value={archivePath} placeholder=".sanctum-backupを選択" /><button className="button secondary" onClick={() => void chooseArchive()}>選択</button></div></label>
                <label>パスワード<input type="password" value={password} onChange={(event) => setPassword(event.target.value)} autoComplete="off" placeholder="12文字以上" /></label>
                <div className="modal-actions"><button className="button ghost" onClick={() => setModal(null)}>キャンセル</button><button className="button primary" disabled={!archivePath || password.length < 12 || busy} onClick={() => void restoreBackup()}>{busy ? "検証中" : android ? "復元する" : "復元先を選択"}</button></div>
              </>
            ) : (
              <>
                <h2 id="modal-title">ChatGPT接続</h2>
                <div className="connection-tabs segmented" role="tablist" aria-label="接続先">
                  <button className={chatGptMode === "chatgpt" ? "active" : ""} role="tab" aria-selected={chatGptMode === "chatgpt"} onClick={() => setChatGptMode("chatgpt")}>普通のChatGPT</button>
                  <button className={chatGptMode === "codex" ? "active" : ""} role="tab" aria-selected={chatGptMode === "codex"} onClick={() => setChatGptMode("codex")}>Codex / Work</button>
                </div>
                {error && <div className="error-banner" role="alert">{error}</div>}
                {chatGptMode === "chatgpt" ? (
                  <div className="connection-panel" role="tabpanel">
                    <p className="muted">普通のChatGPTから、Sanctumで開いているVaultを操作する</p>
                    <div className="connection-status" data-ready={tunnelStatus?.ready ? "true" : "false"}>
                      <strong>
                        {tunnelStatus?.ready
                          ? "Tunnel準備完了"
                          : tunnelStatus?.running
                            ? "Tunnel接続中"
                            : tunnelStatus?.configured
                              ? "Tunnel停止中"
                              : "未設定"}
                      </strong>
                      {tunnelStatus?.tunnelId && <code>{tunnelStatus.tunnelId}</code>}
                      {tunnelStatus?.lastError && <span>{tunnelStatus.lastError}</span>}
                    </div>

                    <div className="connection-setup-row">
                      <span>1. Tunnelを作成し、Runtime API keyとWindows版tunnel-clientを取得</span>
                      <button className="button secondary compact" onClick={() => void openTunnelSettings()}>OpenAI設定</button>
                    </div>

                    <label>
                      Tunnel ID
                      <input value={tunnelId} onChange={(event) => setTunnelId(event.target.value)} placeholder="tunnel_..." autoComplete="off" spellCheck={false} />
                    </label>
                    <label>
                      Runtime API key
                      <input type="password" value={runtimeApiKey} onChange={(event) => setRuntimeApiKey(event.target.value)} placeholder={tunnelStatus?.hasCredential ? "保存済み。変更時だけ入力" : "OpenAIで発行したkey"} autoComplete="off" spellCheck={false} />
                    </label>
                    <label>
                      tunnel-client.exe
                      <div className="path-picker">
                        <input readOnly value={tunnelClientPath} placeholder="ダウンロードしたファイルを選択" />
                        <button className="button secondary" onClick={() => void chooseTunnelClient()}>選択</button>
                      </div>
                    </label>
                    <p className="credential-note">API keyはWindows資格情報マネージャーへ保存する</p>

                    <div className="modal-actions connection-actions">
                      <button className="button ghost" disabled={pluginBusy} onClick={() => void refreshTunnel()}>状態を更新</button>
                      {tunnelStatus?.configured && (
                        <button className="button secondary" disabled={pluginBusy} onClick={() => void setTunnelRunning(!tunnelStatus.running)}>
                          {tunnelStatus.running ? "停止" : "起動"}
                        </button>
                      )}
                      <button className="button primary" disabled={pluginBusy || !tunnelId.trim() || !tunnelClientPath || (!runtimeApiKey && !tunnelStatus?.hasCredential)} onClick={() => void configureTunnel()}>
                        {pluginBusy ? "処理中" : tunnelStatus?.configured ? "設定を更新" : "保存して起動"}
                      </button>
                    </div>

                    {tunnelStatus?.running && (
                      <div className="plugin-ready">
                        <strong>2. ChatGPTへ追加</strong>
                        <p>ChatGPTの設定でDeveloper modeを有効にし、接続方法でTunnelを選ぶ</p>
                        <div className="connection-ready-actions">
                          {tunnelStatus.adminUiUrl && <button className="button secondary compact" onClick={() => void openTunnelAdmin()}>Tunnel状態</button>}
                          <button className="button primary compact" onClick={() => void openChatGptConnectors()}>ChatGPTで追加</button>
                        </div>
                      </div>
                    )}
                  </div>
                ) : (
                  <div className="connection-panel" role="tabpanel">
                    <p className="muted">このPCだけで使う個人用プラグインとして登録する</p>
                    {pluginStatus?.installed ? (
                      <div className="plugin-ready">
                        <strong>登録済み</strong>
                        <p>SanctumでVaultを開いている間、検索・読取り・作成・編集・添付追加ができる</p>
                      </div>
                    ) : (
                      <p className="muted">登録後、ChatGPT Workを再起動してPersonalからSanctumをインストールする</p>
                    )}
                    <div className="modal-actions">
                      {pluginStatus?.installed ? (
                        <button className="button primary" disabled={pluginBusy} onClick={() => void openChatGpt()}>{pluginBusy ? "開いている" : "Codexで開く"}</button>
                      ) : (
                        <button className="button primary" disabled={pluginBusy} onClick={() => void installChatGpt()}>{pluginBusy ? "登録中" : "登録する"}</button>
                      )}
                    </div>
                  </div>
                )}
              </>
            )}
          </section>
        </div>
      )}
    </main>
  );
}
