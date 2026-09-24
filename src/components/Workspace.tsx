import { lazy, Suspense, useCallback, useEffect, useMemo, useState } from "react";
import type { MouseEvent as ReactMouseEvent } from "react";
import { api } from "../api";
import { blockFingerprint } from "../blockSnapshot";
import { blockKindLabel, blockStatusLabel } from "../labels";
import type { GraphData, HypothesisBlock, RecoveryDraft, SearchHit, VaultSummary } from "../types";
import BlockEditor from "./BlockEditor";
import Inspector from "./Inspector";

const ResearchGraph = lazy(() => import("./ResearchGraph"));
const IntegrityView = lazy(() => import("./IntegrityView"));
const RecoveryCenter = lazy(() => import("./RecoveryCenter"));

type View = "editor" | "graph" | "integrity" | "recovery";
type MobilePane = "list" | "stage" | "inspector";
type BlockContextMenu = { block: HypothesisBlock; x: number; y: number };
const commonStatuses = ["Idea", "Testing", "Supported", "Rejected"] as const;

interface Props {
  vault: VaultSummary;
  onClose: () => Promise<void>;
}

const emptyGraph: GraphData = { blocks: [], edges: [], positions: [] };

export default function Workspace({ vault, onClose }: Props) {
  const [currentVault, setCurrentVault] = useState(vault);
  const [blocks, setBlocks] = useState<HypothesisBlock[]>([]);
  const [selected, setSelected] = useState<HypothesisBlock | null>(null);
  const [recovery, setRecovery] = useState<RecoveryDraft | null>(null);
  const [graph, setGraph] = useState<GraphData>(emptyGraph);
  const [view, setView] = useState<View>("editor");
  const [mobilePane, setMobilePane] = useState<MobilePane>("list");
  const [loading, setLoading] = useState(true);
  const [creating, setCreating] = useState(false);
  const [query, setQuery] = useState("");
  const [searchHits, setSearchHits] = useState<SearchHit[]>([]);
  const [statusFilter, setStatusFilter] = useState<string>("all");
  const [advancedNavigation, setAdvancedNavigation] = useState(false);
  const [navigatorOpen, setNavigatorOpen] = useState(true);
  const [inspectorOpen, setInspectorOpen] = useState(true);
  const [contextMenu, setContextMenu] = useState<BlockContextMenu | null>(null);
  const [deleteCandidate, setDeleteCandidate] = useState<HypothesisBlock | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [message, setMessage] = useState<{ tone: "error" | "notice"; text: string } | null>(null);

  const showError = (text: string) => setMessage({ tone: "error", text });
  const showNotice = (text: string) => setMessage({ tone: "notice", text });

  const refreshGraph = useCallback(async () => {
    const [nextGraph, summary] = await Promise.all([api.graph(), api.summary()]);
    setGraph(nextGraph);
    setCurrentVault(summary);
  }, []);

  const refreshBlocks = useCallback(async () => {
    const next = await api.listBlocks();
    setBlocks(next);
    return next;
  }, []);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      setLoading(true);
      try {
        const [nextBlocks, nextGraph] = await Promise.all([api.listBlocks(), api.graph()]);
        if (cancelled) return;
        setBlocks(nextBlocks);
        setGraph(nextGraph);
        if (nextBlocks[0]) await selectBlock(nextBlocks[0].id, false);
      } catch (cause) {
        showError(cause instanceof Error ? cause.message : String(cause));
      } finally {
        if (!cancelled) setLoading(false);
      }
    };
    void load();
    return () => { cancelled = true; };
  }, []);

  useEffect(() => {
    const runMaintenance = () => {
      void api.createDueSnapshots().then((created) => {
        if (created.length) void api.summary().then(setCurrentVault);
      }).catch((cause) => showError(`自動Snapshotに失敗した: ${String(cause)}`));
      void api.runDueAutomaticBackup().then((created) => {
        if (created) void api.summary().then(setCurrentVault);
      }).catch((cause) => showError(`自動Backupに失敗した: ${String(cause)}`));
    };
    runMaintenance();
    const timer = window.setInterval(runMaintenance, 15 * 60_000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    const normalized = query.trim();
    if (!normalized) { setSearchHits([]); return; }
    const timer = window.setTimeout(() => {
      void api.search(normalized).then(setSearchHits).catch((cause) => showError(String(cause)));
    }, 180);
    return () => window.clearTimeout(timer);
  }, [query]);

  useEffect(() => {
    if (!contextMenu) return;
    const close = () => setContextMenu(null);
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") close();
    };
    window.addEventListener("click", close);
    window.addEventListener("resize", close);
    window.addEventListener("scroll", close, true);
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("resize", close);
      window.removeEventListener("scroll", close, true);
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, [contextMenu]);

  useEffect(() => {
    if (!deleteCandidate) return;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !deleting) setDeleteCandidate(null);
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [deleteCandidate, deleting]);

  const selectBlock = async (id: string, switchToEditor = true) => {
    try {
      const [block, draft] = await Promise.all([api.getBlock(id), api.recoveryDraft(id)]);
      setSelected(block);
      setRecovery(draft && blockFingerprint(draft.snapshot) !== blockFingerprint(block) ? draft : null);
      if (switchToEditor) { setView("editor"); setMobilePane("stage"); }
      setQuery("");
      setSearchHits([]);
    } catch (cause) { showError(cause instanceof Error ? cause.message : String(cause)); }
  };

  const createBlock = async () => {
    setCreating(true);
    try {
      const block = await api.createBlock({
        title: "無題の仮説",
        bodyMarkdown: "",
        researchNotesMarkdown: "",
        kind: "Hypothesis",
        status: "Idea",
        tags: [],
        changeReason: "作成",
      });
      await Promise.all([refreshBlocks(), refreshGraph()]);
      setSelected(block); setRecovery(null); setView("editor"); setMobilePane("stage");
    } catch (cause) { showError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setCreating(false); }
  };

  const handleSaved = (saved: HypothesisBlock) => {
    setSelected(saved);
    setBlocks((current) => current.map((block) => block.id === saved.id ? saved : block));
    setGraph((current) => ({ ...current, blocks: current.blocks.map((block) => block.id === saved.id ? saved : block) }));
    void api.summary().then(setCurrentVault).catch((cause) => showError(String(cause)));
  };

  const handleBlockChanged = async (changed: HypothesisBlock) => {
    await Promise.all([refreshBlocks(), refreshGraph()]);
    setSelected(changed);
    setRecovery(null);
    setView("editor"); setMobilePane("stage");
  };

  const handleDeleted = async (blockId: string) => {
    const next = await refreshBlocks();
    await refreshGraph();
    if (selected?.id === blockId) {
      setSelected(next[0] ?? null);
      setRecovery(null);
    }
    showNotice("ブロックをゴミ箱へ移動した。履歴は残っている");
  };

  const openBlockContextMenu = (event: ReactMouseEvent<HTMLButtonElement>, block: HypothesisBlock) => {
    event.preventDefault();
    const menuWidth = 180;
    const menuHeight = 88;
    setContextMenu({
      block,
      x: Math.max(8, Math.min(event.clientX, window.innerWidth - menuWidth - 8)),
      y: Math.max(8, Math.min(event.clientY, window.innerHeight - menuHeight - 8)),
    });
  };

  const deleteBlock = async () => {
    if (!deleteCandidate || deleting) return;
    setDeleting(true);
    try {
      await api.softDeleteBlock(deleteCandidate.id, deleteCandidate.rowVersion);
      await handleDeleted(deleteCandidate.id);
      setDeleteCandidate(null);
    } catch (cause) {
      showError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setDeleting(false);
    }
  };

  const filteredBlocks = useMemo(() => blocks.filter((block) => {
    if (statusFilter === "all") return true;
    if (statusFilter === "other") return !commonStatuses.includes(block.status as typeof commonStatuses[number]);
    return block.status === statusFilter;
  }), [blocks, statusFilter]);

  return (
    <main className="workspace">
      <header className="workspace-topbar">
        <div className="workspace-brand"><button className="text-button" onClick={() => void onClose()}>戻る</button><div><strong>{currentVault.name}</strong><span>リビジョン {currentVault.revision}</span></div><button className="panel-toggle" onClick={() => setNavigatorOpen((open) => !open)} aria-label={navigatorOpen ? "左パネルを隠す" : "左パネルを表示"} title={navigatorOpen ? "左パネルを隠す" : "左パネルを表示"}><span aria-hidden="true">{navigatorOpen ? "◀" : "▶"}</span></button></div>
        <div className="global-search"><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="本文・変数・文献・ファイルを検索" />{query && <button onClick={() => setQuery("")}>消す</button>}{searchHits.length > 0 && <div className="search-results">{searchHits.map((hit) => <button key={hit.blockId} onClick={() => void selectBlock(hit.blockId)}><strong>{hit.title}</strong><span>{hit.excerpt}</span></button>)}</div>}</div>
        <div className="topbar-safety"><span>ローカル保存</span><button className="panel-toggle" onClick={() => setInspectorOpen((open) => !open)} aria-label={inspectorOpen ? "右パネルを隠す" : "右パネルを表示"} title={inspectorOpen ? "右パネルを隠す" : "右パネルを表示"}><span aria-hidden="true">{inspectorOpen ? "▶" : "◀"}</span></button></div>
      </header>

      <div className={`workspace-grid${navigatorOpen ? "" : " navigator-hidden"}${inspectorOpen ? "" : " inspector-hidden"}`} data-mobile-pane={mobilePane} data-view={view}>
        <aside className="navigator">
          <div className="navigator-tabs">
            <button className={view === "editor" ? "active" : ""} onClick={() => setView("editor")}>ブロック</button>
            <button className={view === "graph" ? "active" : ""} onClick={() => setView("graph")}>グラフ</button>
            <button className={view === "recovery" ? "active" : ""} onClick={() => setView("recovery")}>データ</button>
            <button className={advancedNavigation ? "active" : ""} onClick={() => { setAdvancedNavigation((value) => !value); if (advancedNavigation && view === "integrity") setView("editor"); }}>詳細</button>
          </div>
          {advancedNavigation && <button className={`advanced-nav-item ${view === "integrity" ? "active" : ""}`} onClick={() => setView("integrity")}>整合性検査</button>}

          <div className="navigator-heading"><div><strong>ブロック</strong><span>{blocks.length}</span></div><button className="text-button" disabled={creating} onClick={() => void createBlock()}>{creating ? "作成中" : "追加"}</button></div>
          <label className="filter-select"><select value={statusFilter} onChange={(event) => setStatusFilter(event.target.value)}><option value="all">すべて</option>{commonStatuses.map((status) => <option key={status} value={status}>{blockStatusLabel[status]}</option>)}<option value="other">その他</option></select></label>

          <nav className="block-list" aria-label="仮説ブロック">
            {filteredBlocks.map((block) => <button key={block.id} className={selected?.id === block.id ? "active" : ""} onClick={() => void selectBlock(block.id)} onContextMenu={(event) => openBlockContextMenu(event, block)}><span className={`status-dot status-${block.status.toLowerCase().replaceAll(" ", "-")}`} /><div><strong>{block.title}</strong><span>{blockKindLabel[block.kind]} · {block.id.slice(0, 8).toUpperCase()}{block.parentBlockId ? " · 分岐" : ""}</span></div></button>)}
            {!loading && !filteredBlocks.length && <div className="navigator-empty"><p>表示するブロックがない</p><button onClick={() => void createBlock()}>仮説を作成</button></div>}
          </nav>
        </aside>

        <section className="main-stage">
          {loading ? <div className="center-message">Vaultを開いている</div> : (
            <Suspense fallback={<div className="center-message">読み込み中</div>}>
              {view === "editor" && (selected ? <BlockEditor key={selected.id} block={selected} recovery={recovery} onSaved={handleSaved} onRecoveryResolved={() => setRecovery(null)} onError={showError} /> : <WelcomeEmpty onCreate={() => void createBlock()} />)}
              {view === "graph" && <ResearchGraph data={graph} selectedBlockId={selected?.id ?? null} onSelectBlock={(id) => void selectBlock(id, false)} onRefresh={refreshGraph} onError={showError} />}
              {view === "integrity" && <IntegrityView onSelectBlock={(id) => void selectBlock(id)} onError={showError} />}
              {view === "recovery" && <RecoveryCenter onBlocksChanged={async () => { await Promise.all([refreshBlocks(), refreshGraph()]); }} onError={showError} onNotice={showNotice} onIntegrity={() => setView("integrity")} />}
            </Suspense>
          )}
        </section>

        <Inspector block={selected} graph={graph} onBlockChanged={(block) => void handleBlockChanged(block)} onDeleted={(id) => void handleDeleted(id)} onGraphChanged={async () => { await refreshGraph(); }} onError={showError} />
      </div>

      <nav className="mobile-bottom-nav" aria-label="スマホ用ナビゲーション">
        <button className={view === "editor" && mobilePane === "list" ? "active" : ""} aria-current={view === "editor" && mobilePane === "list" ? "page" : undefined} onClick={() => { setView("editor"); setMobilePane("list"); }}>一覧</button>
        <button className={view === "editor" && mobilePane === "stage" ? "active" : ""} aria-current={view === "editor" && mobilePane === "stage" ? "page" : undefined} onClick={() => { setView("editor"); setMobilePane("stage"); }}>本文</button>
        <button className={view === "editor" && mobilePane === "inspector" ? "active" : ""} aria-current={view === "editor" && mobilePane === "inspector" ? "page" : undefined} onClick={() => { setView("editor"); setMobilePane("inspector"); }}>詳細</button>
        <button className={view === "graph" ? "active" : ""} aria-current={view === "graph" ? "page" : undefined} onClick={() => setView("graph")}>グラフ</button>
        <button className={view === "recovery" || view === "integrity" ? "active" : ""} aria-current={view === "recovery" || view === "integrity" ? "page" : undefined} onClick={() => setView("recovery")}>データ</button>
      </nav>

      {contextMenu && (
        <div className="block-context-menu" role="menu" style={{ left: contextMenu.x, top: contextMenu.y }} onClick={(event) => event.stopPropagation()}>
          <button role="menuitem" onClick={() => { void selectBlock(contextMenu.block.id); setContextMenu(null); }}>開く</button>
          <button className="danger" role="menuitem" onClick={() => { setDeleteCandidate(contextMenu.block); setContextMenu(null); }}>ゴミ箱へ移動</button>
        </div>
      )}

      {deleteCandidate && (
        <div className="modal-backdrop" onMouseDown={() => { if (!deleting) setDeleteCandidate(null); }}>
          <section className="modal block-delete-modal" role="dialog" aria-modal="true" aria-labelledby="delete-block-title" onMouseDown={(event) => event.stopPropagation()}>
            <h2 id="delete-block-title">ゴミ箱へ移動する？</h2>
            <p className="muted"><strong>{deleteCandidate.title}</strong>を一覧から外す。履歴は残り、データ画面から復元できる</p>
            <div className="modal-actions"><button className="button ghost" disabled={deleting} autoFocus onClick={() => setDeleteCandidate(null)}>キャンセル</button><button className="button danger" disabled={deleting} onClick={() => void deleteBlock()}>{deleting ? "移動中" : "移動する"}</button></div>
          </section>
        </div>
      )}

      {message && <div className={`toast ${message.tone}`} role={message.tone === "error" ? "alert" : "status"}><span>{message.text}</span><button className="text-button" onClick={() => setMessage(null)}>閉じる</button></div>}
    </main>
  );
}

function WelcomeEmpty({ onCreate }: { onCreate: () => void }) {
  return <div className="workspace-empty"><h2>最初の仮説を作成</h2><button className="button primary" onClick={onCreate}>仮説を作成</button></div>;
}
