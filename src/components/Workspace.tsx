import { lazy, Suspense, useCallback, useEffect, useMemo, useState } from "react";
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
  const [loading, setLoading] = useState(true);
  const [creating, setCreating] = useState(false);
  const [query, setQuery] = useState("");
  const [searchHits, setSearchHits] = useState<SearchHit[]>([]);
  const [statusFilter, setStatusFilter] = useState<string>("all");
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
        void api.createDueSnapshots().then(() => api.summary()).then(setCurrentVault).catch((cause) => showError(`自動Snapshotに失敗した: ${String(cause)}`));
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
    const timer = window.setInterval(() => {
      void api.createDueSnapshots().then(() => api.summary()).then(setCurrentVault).catch((cause) => showError(`自動Snapshotに失敗した: ${String(cause)}`));
    }, 60_000);
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

  const selectBlock = async (id: string, switchToEditor = true) => {
    try {
      const [block, draft] = await Promise.all([api.getBlock(id), api.recoveryDraft(id)]);
      setSelected(block);
      setRecovery(draft && blockFingerprint(draft.snapshot) !== blockFingerprint(block) ? draft : null);
      if (switchToEditor) setView("editor");
      setQuery("");
      setSearchHits([]);
    } catch (cause) { showError(cause instanceof Error ? cause.message : String(cause)); }
  };

  const createBlock = async () => {
    setCreating(true);
    try {
      const block = await api.createBlock({
        title: "無題の仮説",
        bodyMarkdown: "## 研究の問い\n\n検証できる主張を書く。\n\n## モデル\n\n$$\nY = f(X)\n$$\n",
        researchNotesMarkdown: "",
        kind: "Hypothesis",
        status: "Idea",
        tags: [],
        changeReason: "仮説を作成",
      });
      await Promise.all([refreshBlocks(), refreshGraph()]);
      setSelected(block); setRecovery(null); setView("editor");
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
    setView("editor");
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

  const filteredBlocks = useMemo(() => blocks.filter((block) => statusFilter === "all" || block.status === statusFilter), [blocks, statusFilter]);

  return (
    <main className="workspace">
      <header className="workspace-topbar">
        <div className="workspace-brand"><button className="text-button" onClick={() => void onClose()}>戻る</button><div><strong>{currentVault.name}</strong><span>リビジョン {currentVault.revision}</span></div></div>
        <div className="global-search"><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="タイトル・本文・LaTeX・変数・文献を検索" />{query && <button onClick={() => setQuery("")}>消す</button>}{searchHits.length > 0 && <div className="search-results">{searchHits.map((hit) => <button key={hit.blockId} onClick={() => void selectBlock(hit.blockId)}><strong>{hit.title}</strong><span>{hit.excerpt}</span></button>)}</div>}</div>
        <div className="topbar-safety"><span>ローカル保存</span></div>
      </header>

      <div className="workspace-grid">
        <aside className="navigator">
          <div className="navigator-tabs">
            <button className={view === "editor" ? "active" : ""} onClick={() => setView("editor")}>ブロック</button>
            <button className={view === "graph" ? "active" : ""} onClick={() => setView("graph")}>グラフ</button>
            <button className={view === "integrity" ? "active" : ""} onClick={() => setView("integrity")}>検査</button>
            <button className={view === "recovery" ? "active" : ""} onClick={() => setView("recovery")}>復旧</button>
          </div>

          <div className="navigator-heading"><div><strong>ブロック</strong><span>{blocks.length}</span></div><button className="text-button" disabled={creating} onClick={() => void createBlock()}>{creating ? "作成中" : "追加"}</button></div>
          <label className="filter-select"><select value={statusFilter} onChange={(event) => setStatusFilter(event.target.value)}><option value="all">すべての状態</option>{(["Idea", "Developing", "Testing", "Supported", "Weakly Supported", "Rejected", "Archived"] as const).map((status) => <option key={status} value={status}>{blockStatusLabel[status]}</option>)}</select></label>

          <nav className="block-list" aria-label="仮説ブロック">
            {filteredBlocks.map((block) => <button key={block.id} className={selected?.id === block.id ? "active" : ""} onClick={() => void selectBlock(block.id)}><span className={`status-dot status-${block.status.toLowerCase().replaceAll(" ", "-")}`} /><div><strong>{block.title}</strong><span>{blockKindLabel[block.kind]} · {block.id.slice(0, 8).toUpperCase()}{block.parentBlockId ? " · 分岐" : ""}</span></div></button>)}
            {!loading && !filteredBlocks.length && <div className="navigator-empty"><p>表示するブロックがない</p><button onClick={() => void createBlock()}>仮説を作成</button></div>}
          </nav>
          <footer className="navigator-footer">自動保存</footer>
        </aside>

        <section className="main-stage">
          {loading ? <div className="center-message">Vaultを開いている</div> : (
            <Suspense fallback={<div className="center-message">読み込み中</div>}>
              {view === "editor" && (selected ? <BlockEditor key={selected.id} block={selected} recovery={recovery} onSaved={handleSaved} onRecoveryResolved={() => setRecovery(null)} onError={showError} /> : <WelcomeEmpty onCreate={() => void createBlock()} />)}
              {view === "graph" && <ResearchGraph data={graph} selectedBlockId={selected?.id ?? null} onSelectBlock={(id) => void selectBlock(id, false)} onRefresh={refreshGraph} onError={showError} />}
              {view === "integrity" && <IntegrityView onSelectBlock={(id) => void selectBlock(id)} onError={showError} />}
              {view === "recovery" && <RecoveryCenter onBlocksChanged={async () => { await Promise.all([refreshBlocks(), refreshGraph()]); }} onError={showError} onNotice={showNotice} />}
            </Suspense>
          )}
        </section>

        <Inspector block={selected} graph={graph} onBlockChanged={(block) => void handleBlockChanged(block)} onDeleted={(id) => void handleDeleted(id)} onGraphChanged={async () => { await refreshGraph(); }} onError={showError} />
      </div>

      {message && <div className={`toast ${message.tone}`} role={message.tone === "error" ? "alert" : "status"}><span>{message.text}</span><button className="text-button" onClick={() => setMessage(null)}>閉じる</button></div>}
    </main>
  );
}

function WelcomeEmpty({ onCreate }: { onCreate: () => void }) {
  return <div className="workspace-empty"><h2>最初の仮説を作成</h2><button className="button primary" onClick={onCreate}>仮説を作成</button></div>;
}
