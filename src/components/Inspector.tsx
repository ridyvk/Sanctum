import { useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { readFile, writeFile } from "@tauri-apps/plugin-fs";
import { diffWordsWithSpace } from "diff";
import { api, isAndroidRuntime } from "../api";
import { lookupDoi, parseBibTeX, type CitationDraft } from "../citations";
import { saveMobileFile } from "../mobileFiles";
import {
  attachmentRelationLabel,
  blockKindLabel,
  blockStatusLabel,
  edgeTypeLabel,
  formatDateTime,
} from "../labels";
import type {
  Attachment,
  AttachmentRelation,
  BlockCitationRecord,
  BlockVersion,
  CitationRecord,
  EdgeType,
  GraphData,
  HypothesisBlock,
  VariableRecord,
} from "../types";

type Tab = "relations" | "versions" | "files" | "variables" | "citations" | "metadata";

interface Props {
  block: HypothesisBlock | null;
  graph: GraphData;
  onBlockChanged: (block: HypothesisBlock) => void;
  onDeleted: (blockId: string) => void;
  onGraphChanged: () => Promise<void>;
  onError: (message: string) => void;
}

const tabs: { id: Exclude<Tab, "metadata">; label: string }[] = [
  { id: "relations", label: "関係" },
  { id: "versions", label: "履歴" },
  { id: "files", label: "ファイル" },
  { id: "variables", label: "変数" },
  { id: "citations", label: "文献" },
];

const edgeTypes: EdgeType[] = ["Supports", "Contradicts", "Depends on", "Derived from", "Assumes", "Extends", "Tests", "Alternative to", "Related to"];
const fileRelations: AttachmentRelation[] = ["Supports", "Contradicts", "Background", "Method", "Dataset", "Reference", "Other"];

export default function Inspector({ block, graph, onBlockChanged, onDeleted, onGraphChanged, onError }: Props) {
  const [tab, setTab] = useState<Tab>("relations");
  const [versions, setVersions] = useState<BlockVersion[]>([]);
  const [files, setFiles] = useState<Attachment[]>([]);
  const [variables, setVariables] = useState<VariableRecord[]>([]);
  const [citations, setCitations] = useState<BlockCitationRecord[]>([]);
  const [loading, setLoading] = useState(false);

  const reload = async () => {
    if (!block) return;
    setLoading(true);
    try {
      const [nextVersions, nextFiles, nextVariables, nextCitations] = await Promise.all([
        api.versions(block.id), api.attachments(block.id), api.variables(), api.citations(block.id),
      ]);
      setVersions(nextVersions);
      setFiles(nextFiles);
      setVariables(nextVariables);
      setCitations(nextCitations);
    } catch (cause) {
      onError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => { void reload(); }, [block?.id, block?.currentVersionId]);

  if (!block) {
    return <aside className="inspector empty-inspector"><p>ブロックを選択</p></aside>;
  }

  return (
    <aside className="inspector">
      <div className="inspector-tabs" role="tablist">
        {tabs.map((item) => <button key={item.id} className={tab === item.id ? "active" : ""} onClick={() => setTab(item.id)}>{item.label}</button>)}
        <button className={tab === "metadata" ? "active" : ""} onClick={() => setTab("metadata")}>詳細</button>
      </div>
      <header className="inspector-header"><h3>{tab === "metadata" ? "詳細" : tabs.find((item) => item.id === tab)?.label}</h3>{loading && <span>読込中</span>}</header>
      <div className="inspector-content">
        {tab === "relations" && <RelationsPanel block={block} graph={graph} onChanged={onGraphChanged} onError={onError} />}
        {tab === "versions" && <VersionsPanel block={block} versions={versions} onRestored={(restored) => { onBlockChanged(restored); void reload(); }} onError={onError} />}
        {tab === "files" && <FilesPanel block={block} files={files} onChanged={() => void reload()} onError={onError} />}
        {tab === "variables" && <VariablesPanel block={block} variables={variables} onChanged={() => void reload()} onError={onError} />}
        {tab === "citations" && <CitationsPanel block={block} citations={citations} onChanged={() => void reload()} onError={onError} />}
        {tab === "metadata" && <MetadataPanel block={block} onBlockChanged={onBlockChanged} onDeleted={onDeleted} onError={onError} />}
      </div>
    </aside>
  );
}

function RelationsPanel({ block, graph, onChanged, onError }: { block: HypothesisBlock; graph: GraphData; onChanged: () => Promise<void>; onError: (message: string) => void }) {
  const [target, setTarget] = useState("");
  const [type, setType] = useState<EdgeType>("Related to");
  const relations = graph.edges.filter((edge) => edge.sourceBlockId === block.id || edge.targetBlockId === block.id);
  const names = new Map(graph.blocks.map((item) => [item.id, item.title]));
  const add = async () => {
    if (!target) return;
    try {
      await api.createEdge(block.id, target, type);
      setTarget("");
      await onChanged();
    } catch (cause) { onError(cause instanceof Error ? cause.message : String(cause)); }
  };
  return <div className="panel-stack">
    <div className="compact-form"><select value={type} onChange={(event) => setType(event.target.value as EdgeType)}>{edgeTypes.map((item) => <option key={item} value={item}>{edgeTypeLabel[item]}</option>)}</select><select value={target} onChange={(event) => setTarget(event.target.value)}><option value="">接続先を選択</option>{graph.blocks.filter((item) => item.id !== block.id).map((item) => <option key={item.id} value={item.id}>{item.title}</option>)}</select><button className="button primary compact" disabled={!target} onClick={() => void add()}>追加</button></div>
    <div className="inspector-list">{relations.map((edge) => {
      const outgoing = edge.sourceBlockId === block.id;
      const other = outgoing ? edge.targetBlockId : edge.sourceBlockId;
      return <div className="relation-row" key={edge.id}><div><strong>{edgeTypeLabel[edge.edgeType]}</strong><span>{outgoing ? "→" : "←"} {names.get(other) ?? other.slice(0, 8)}</span></div><button className="text-button danger" onClick={() => void api.softDeleteEdge(edge.id).then(onChanged).catch((cause) => onError(String(cause)))}>削除</button></div>;
    })}{!relations.length && <Empty text="関係はまだない" />}</div>
  </div>;
}

function VersionsPanel({ block, versions, onRestored, onError }: { block: HypothesisBlock; versions: BlockVersion[]; onRestored: (block: HypothesisBlock) => void; onError: (message: string) => void }) {
  const [expanded, setExpanded] = useState<string | null>(null);
  const restore = async (version: BlockVersion) => {
    const reason = window.prompt(`${version.versionLabel}を新しい版として復元する理由`);
    if (!reason?.trim()) return;
    try { onRestored(await api.restoreVersion(block.id, version.id, reason)); }
    catch (cause) { onError(cause instanceof Error ? cause.message : String(cause)); }
  };
  return <div className="version-list">{versions.map((version, index) => {
    const previous = versions[index + 1];
    const changes = previous ? diffWordsWithSpace(previous.snapshot.bodyMarkdown, version.snapshot.bodyMarkdown) : [];
    return <article className="version-card" key={version.id}><button className="version-main" onClick={() => setExpanded((value) => value === version.id ? null : version.id)}><span className="version-label">{version.versionLabel}</span><div><strong>{version.changeReason}</strong><time>{formatDateTime(version.createdAt)}</time></div>{version.id === block.currentVersionId && <i>現在</i>}</button>{expanded === version.id && <div className="version-detail"><code>{version.contentSha256.slice(0, 16)}…</code>{previous ? <div className="word-diff">{changes.map((part, partIndex) => <span className={part.added ? "added" : part.removed ? "removed" : ""} key={partIndex}>{part.value}</span>)}</div> : <p>最初の版</p>}<button className="button secondary compact" disabled={version.id === block.currentVersionId} onClick={() => void restore(version)}>この版を復元</button></div>}</article>;
  })}</div>;
}

function FilesPanel({ block, files, onChanged, onError }: { block: HypothesisBlock; files: Attachment[]; onChanged: () => void; onError: (message: string) => void }) {
  const android = isAndroidRuntime();
  const [relation, setRelation] = useState<AttachmentRelation>("Reference");
  const relationRef = useRef(relation);
  const [filter, setFilter] = useState("");
  const [dragging, setDragging] = useState(false);
  const [busy, setBusy] = useState(false);
  const [preview, setPreview] = useState<{ name: string; url: string } | null>(null);
  useEffect(() => { relationRef.current = relation; }, [relation]);

  const attachPaths = async (paths: string[]) => {
    if (!paths.length) return;
    setBusy(true);
    try {
      for (const [index, path] of paths.entries()) {
        if (!android) { await api.attachFile(block.id, path, relationRef.current); continue; }
        const name = decodeURIComponent(path.split("/").at(-1) ?? "").split("/").at(-1)!
          .replace(/[\\/:*?"<>|\x00-\x1f]/g, "-").trim().slice(0, 140) || `添付-${index + 1}`;
        const bytes = await readFile(path);
        if (bytes.byteLength > 64 * 1024 * 1024) throw new Error("64 MBを超える添付ファイルはこの版では追加できない");
        const transfer = await api.prepareMobileImport(name);
        try {
          await writeFile(transfer.path, bytes);
          await api.attachMobileImport(transfer.id, name, block.id, relationRef.current);
        } finally { await api.discardMobileImport(transfer.id, name); }
      }
      onChanged();
    } catch (cause) {
      onError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    try {
      getCurrentWebview().onDragDropEvent((event) => {
        if (event.payload.type === "enter" || event.payload.type === "over") setDragging(true);
        if (event.payload.type === "leave") setDragging(false);
        if (event.payload.type === "drop") {
          setDragging(false);
          void attachPaths(event.payload.paths);
        }
      }).then((stop) => { if (disposed) stop(); else unlisten = stop; }).catch(() => undefined);
    } catch { /* Browser tests have no native webview. */ }
    return () => { disposed = true; unlisten?.(); };
    // The active block intentionally owns the native drop listener.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [block.id]);

  const attach = async () => {
    const selected = await open({ multiple: true, directory: false, title: "添付するファイルを選択" });
    if (!selected) return;
    await attachPaths(Array.isArray(selected) ? selected : [selected]);
  };
  const showPreview = async (file: Attachment) => {
    try {
      const result = await api.attachmentPreview(file.id);
      setPreview({ name: file.displayName, url: `data:${result.mediaType};base64,${result.dataBase64}` });
    } catch (cause) { onError(cause instanceof Error ? cause.message : String(cause)); }
  };
  const remove = async (file: Attachment) => {
    if (!window.confirm(`「${file.displayName}」を外す？`)) return;
    try { await api.deleteAttachment(file.id); onChanged(); }
    catch (cause) { onError(cause instanceof Error ? cause.message : String(cause)); }
  };
  const visible = files.filter((file) => file.displayName.toLocaleLowerCase().includes(filter.trim().toLocaleLowerCase()));
  return <div className="panel-stack">
    <div className="inline-form"><select value={relation} onChange={(event) => setRelation(event.target.value as AttachmentRelation)}>{fileRelations.map((item) => <option key={item} value={item}>{attachmentRelationLabel[item]}</option>)}</select><button className="button primary compact" disabled={busy} onClick={() => void attach()}>{busy ? "追加中" : "選択"}</button></div>
    <button className={`file-drop-zone ${dragging ? "active" : ""}`} disabled={busy} onClick={() => void attach()}>{android ? "ファイルを追加" : dragging ? "ここへ追加" : "ファイルをドロップ"}</button>
    {files.length > 5 && <input className="file-filter" value={filter} onChange={(event) => setFilter(event.target.value)} placeholder="ファイル名で検索" />}
    <div className="inspector-list">{visible.map((file) => <div className="file-row file-row-actions" key={file.id}><div><strong title={file.displayName}>{file.displayName}</strong><span>{attachmentRelationLabel[file.relationType]} · {formatBytes(file.byteSize)}</span></div><div className="file-actions">{file.mediaType?.startsWith("image/") && <button className="text-button" onClick={() => void showPreview(file)}>表示</button>}<button className="text-button" onClick={() => {
      if (!android) { void api.openAttachment(file.id).catch((cause) => onError(String(cause))); return; }
      void api.exportMobileAttachment(file.id).then(async (exported) => {
        try { await saveMobileFile(exported.path, exported.fileName); }
        finally { await api.discardMobileExport(exported.path); }
      }).catch((cause) => onError(String(cause)));
    }}>{android ? "保存" : "開く"}</button><button className="text-button danger" onClick={() => void remove(file)}>外す</button></div></div>)}{!visible.length && <Empty text={files.length ? "一致するファイルはない" : "添付ファイルはない"} />}</div>
    {preview && <div className="attachment-preview" role="dialog" aria-label={preview.name}><header><strong>{preview.name}</strong><button className="text-button" onClick={() => setPreview(null)}>閉じる</button></header><img src={preview.url} alt={preview.name} /></div>}
  </div>;
}

function VariablesPanel({ block, variables, onChanged, onError }: { block: HypothesisBlock; variables: VariableRecord[]; onChanged: () => void; onError: (message: string) => void }) {
  const [symbol, setSymbol] = useState("");
  const [definition, setDefinition] = useState("");
  const [formula, setFormula] = useState("");
  const relevant = variables.filter((variable) => variable.definitions.some((item) => item.blockId === block.id));
  const add = async () => {
    if (!symbol.trim() || !definition.trim()) return;
    try { await api.registerVariable(symbol, definition, block.id, formula); setSymbol(""); setDefinition(""); setFormula(""); onChanged(); }
    catch (cause) { onError(cause instanceof Error ? cause.message : String(cause)); }
  };
  return <div className="panel-stack"><div className="compact-form variable-form"><input value={symbol} onChange={(event) => setSymbol(event.target.value)} placeholder="μ" aria-label="変数記号" /><input value={definition} onChange={(event) => setDefinition(event.target.value)} placeholder="定義" /><input value={formula} onChange={(event) => setFormula(event.target.value)} placeholder="μ(a,t)" /><button className="button primary compact" disabled={!symbol.trim() || !definition.trim()} onClick={() => void add()}>登録</button></div><div className="inspector-list">{relevant.map((variable) => <div className={`variable-row ${variable.hasConflict ? "conflict" : ""}`} key={variable.id}><div><strong>{variable.symbol}</strong>{variable.definitions.filter((item) => item.blockId === block.id).map((item) => <span key={item.id}>{item.definition}{item.formula && ` · ${item.formula}`}</span>)}{variable.hasConflict && <em>別ブロックの定義と競合</em>}</div></div>)}{!relevant.length && <Empty text="変数は未登録" />}</div></div>;
}

function CitationsPanel({ block, citations, onChanged, onError }: { block: HypothesisBlock; citations: BlockCitationRecord[]; onChanged: () => void; onError: (message: string) => void }) {
  const [mode, setMode] = useState<"doi" | "bibtex" | "manual">("doi");
  const [key, setKey] = useState("");
  const [title, setTitle] = useState("");
  const [doi, setDoi] = useState("");
  const [bibtex, setBibtex] = useState("");
  const [busy, setBusy] = useState(false);
  const addDrafts = async (drafts: CitationDraft[]) => {
    if (!drafts.length) throw new Error("読み込める文献がない");
    setBusy(true);
    try {
      for (const draft of drafts) await api.addCitation(block.id, draft);
      onChanged();
    } finally { setBusy(false); }
  };
  const importDoi = async () => {
    try { await addDrafts([await lookupDoi(doi)]); setDoi(""); }
    catch (cause) { onError(cause instanceof Error ? cause.message : String(cause)); }
  };
  const importBibtex = async () => {
    try { const drafts = parseBibTeX(bibtex); await addDrafts(drafts); setBibtex(""); }
    catch (cause) { onError(cause instanceof Error ? cause.message : String(cause)); }
  };
  const addManual = async () => {
    if (!key.trim() || !title.trim()) return;
    try {
      await addDrafts([{ citationKey: key.trim(), title: title.trim(), authors: "", year: null, doi: doi.trim() || null, url: null, rawCslJson: {} }]);
      setKey(""); setTitle(""); setDoi("");
    } catch (cause) { onError(cause instanceof Error ? cause.message : String(cause)); }
  };
  return <div className="panel-stack">
    <div className="segmented citation-modes"><button className={mode === "doi" ? "active" : ""} onClick={() => setMode("doi")}>DOI</button><button className={mode === "bibtex" ? "active" : ""} onClick={() => setMode("bibtex")}>BibTeX</button><button className={mode === "manual" ? "active" : ""} onClick={() => setMode("manual")}>手入力</button></div>
    {mode === "doi" && <div className="compact-form"><input value={doi} onChange={(event) => setDoi(event.target.value)} placeholder="10.xxxx/xxxxx" /><button className="button primary compact" disabled={busy || !doi.trim()} onClick={() => void importDoi()}>{busy ? "取得中" : "DOIから追加"}</button></div>}
    {mode === "bibtex" && <div className="compact-form"><textarea className="bibtex-input" value={bibtex} onChange={(event) => setBibtex(event.target.value)} placeholder="@article{...}" /><button className="button primary compact" disabled={busy || !bibtex.trim()} onClick={() => void importBibtex()}>{busy ? "読込中" : "BibTeXを読込"}</button></div>}
    {mode === "manual" && <div className="compact-form"><input value={key} onChange={(event) => setKey(event.target.value)} placeholder="引用キー" /><input value={title} onChange={(event) => setTitle(event.target.value)} placeholder="文献名" /><input value={doi} onChange={(event) => setDoi(event.target.value)} placeholder="DOI（任意）" /><button className="button primary compact" disabled={busy || !key.trim() || !title.trim()} onClick={() => void addManual()}>登録</button></div>}
    <div className="inspector-list">{citations.map((link) => <div className="citation-row" key={link.linkId}><div><strong>[{link.citation.citationKey}] {link.citation.title}</strong><span>{[link.citation.authors, link.citation.year, link.citation.doi].filter(Boolean).join(" · ")}</span>{link.quoteText && <q>{link.quoteText}</q>}</div></div>)}{!citations.length && <Empty text="文献は未登録" />}</div>
  </div>;
}

function MetadataPanel({ block, onBlockChanged, onDeleted, onError }: { block: HypothesisBlock; onBlockChanged: (block: HypothesisBlock) => void; onDeleted: (id: string) => void; onError: (message: string) => void }) {
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [confirmation, setConfirmation] = useState("");
  const branch = async () => {
    const title = window.prompt("分岐する仮説のタイトル", `${block.title} — 分岐`);
    if (!title?.trim()) return;
    const reason = window.prompt("分岐する理由");
    if (!reason?.trim()) return;
    try { onBlockChanged(await api.branchBlock(block.id, block.rowVersion, title, reason)); }
    catch (cause) { onError(cause instanceof Error ? cause.message : String(cause)); }
  };
  const remove = async () => {
    if (confirmation !== block.title) return;
    try { await api.softDeleteBlock(block.id, block.rowVersion); onDeleted(block.id); }
    catch (cause) { onError(cause instanceof Error ? cause.message : String(cause)); }
  };
  const fields = [
    ["ブロックID", block.id], ["種類", blockKindLabel[block.kind]], ["状態", blockStatusLabel[block.status]], ["行バージョン", String(block.rowVersion)],
    ["作成", formatDateTime(block.createdAt)], ["更新", formatDateTime(block.updatedAt)],
    ["親ブロック", block.parentBlockId ?? "—"],
  ];
  return <div className="panel-stack"><dl className="metadata-list">{fields.map(([term, value]) => <div key={term}><dt>{term}</dt><dd>{value}</dd></div>)}</dl><button className="button secondary full" onClick={() => void branch()}>現在の版から分岐</button><button className="button danger-outline full" onClick={() => setDeleteOpen(true)}>ゴミ箱へ移動</button>{deleteOpen && <div className="danger-zone"><strong>ゴミ箱へ移動する？</strong><p>履歴は残り、あとで復元できる</p><label><b>{block.title}</b>と入力<input value={confirmation} onChange={(event) => setConfirmation(event.target.value)} /></label><div className="inline-actions"><button className="button ghost compact" onClick={() => { setDeleteOpen(false); setConfirmation(""); }}>キャンセル</button><button className="button danger compact" disabled={confirmation !== block.title} onClick={() => void remove()}>移動</button></div></div>}</div>;
}

function Empty({ text }: { text: string }) { return <p className="empty-row">{text}</p>; }

function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
}
