import { useEffect, useMemo, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import rehypeKatex from "rehype-katex";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import { api } from "../api";
import { blockFingerprint, toBlockSnapshot } from "../blockSnapshot";
import { blockKindLabel, blockStatusLabel, formatDateTime } from "../labels";
import type {
  BlockKind,
  BlockSnapshot,
  BlockStatus,
  HypothesisBlock,
  RecoveryDraft,
  SaveBlockInput,
} from "../types";

type SaveState = "saved" | "dirty" | "saving" | "conflict" | "error" | "recovered";
type ViewMode = "editor" | "split" | "preview";
type DocumentTab = "hypothesis" | "notes";

interface EditorForm {
  title: string;
  bodyMarkdown: string;
  researchNotesMarkdown: string;
  kind: BlockKind;
  status: BlockStatus;
  tags: string;
  changeReason: string;
}

interface Props {
  block: HypothesisBlock;
  recovery: RecoveryDraft | null;
  onSaved: (block: HypothesisBlock) => void;
  onRecoveryResolved: () => void;
  onError: (message: string) => void;
}

const statuses: BlockStatus[] = [
  "Idea",
  "Developing",
  "Testing",
  "Supported",
  "Weakly Supported",
  "Rejected",
  "Archived",
];

const kinds: BlockKind[] = ["Hypothesis", "Assumption", "Method", "Evidence"];

function formFrom(snapshot: BlockSnapshot, reason: string): EditorForm {
  return {
    title: snapshot.title,
    bodyMarkdown: snapshot.bodyMarkdown,
    researchNotesMarkdown: snapshot.researchNotesMarkdown,
    kind: snapshot.kind,
    status: snapshot.status,
    tags: snapshot.tags.join(", "),
    changeReason: reason,
  };
}

function formSnapshot(id: string, form: EditorForm): BlockSnapshot {
  return {
    id,
    title: form.title.trim(),
    bodyMarkdown: form.bodyMarkdown,
    researchNotesMarkdown: form.researchNotesMarkdown,
    kind: form.kind,
    status: form.status,
    tags: form.tags.split(",").map((tag) => tag.trim()).filter(Boolean),
  };
}

export default function BlockEditor({ block, recovery, onSaved, onRecoveryResolved, onError }: Props) {
  const initialSnapshot = recovery?.snapshot ?? block;
  const [form, setForm] = useState<EditorForm>(() =>
    formFrom(initialSnapshot, recovery ? "復旧した編集内容" : "自動保存"),
  );
  const [saveState, setSaveState] = useState<SaveState>(recovery ? "recovered" : "saved");
  const [viewMode, setViewMode] = useState<ViewMode>("split");
  const [documentTab, setDocumentTab] = useState<DocumentTab>("hypothesis");
  const [recoveryPending, setRecoveryPending] = useState(Boolean(recovery));
  const formRef = useRef(form);
  const blockRef = useRef(block);
  const committedRef = useRef<BlockSnapshot>(toBlockSnapshot(block));
  const generationRef = useRef(0);
  const savingRef = useRef(false);
  const editorRef = useRef<HTMLTextAreaElement | null>(null);

  useEffect(() => {
    formRef.current = form;
  }, [form]);

  const currentSnapshot = useMemo(() => formSnapshot(block.id, form), [block.id, form]);
  const dirty = blockFingerprint(currentSnapshot) !== blockFingerprint(committedRef.current);

  const updateForm = (patch: Partial<EditorForm>) => {
    generationRef.current += 1;
    setForm((current) => ({ ...current, ...patch }));
  };

  const makeInput = (candidate: EditorForm): SaveBlockInput => ({
    ...formSnapshot(block.id, candidate),
    blockId: block.id,
    expectedRowVersion: blockRef.current.rowVersion,
    changeReason: candidate.changeReason.trim() || "自動保存",
  });

  const commit = async (candidate: EditorForm, generation: number, forceRecovery = false) => {
    if (savingRef.current || (recoveryPending && !forceRecovery)) return;
    if (blockFingerprint(formSnapshot(block.id, candidate)) === blockFingerprint(committedRef.current)) {
      setSaveState("saved");
      return;
    }
    savingRef.current = true;
    const savingIndicator = window.setTimeout(() => setSaveState("saving"), 350);
    let succeeded = false;
    try {
      const saved = await api.saveBlock(makeInput(candidate));
      blockRef.current = saved;
      committedRef.current = toBlockSnapshot(saved);
      onSaved(saved);
      setSaveState(generationRef.current === generation ? "saved" : "dirty");
      succeeded = true;
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause);
      setSaveState(message.toLowerCase().includes("concurrent edit") ? "conflict" : "error");
      onError(message);
    } finally {
      window.clearTimeout(savingIndicator);
      savingRef.current = false;
      if (succeeded && blockFingerprint(formSnapshot(block.id, formRef.current)) !== blockFingerprint(committedRef.current)) {
        window.setTimeout(() => void commit(formRef.current, generationRef.current), 250);
      }
    }
  };

  useEffect(() => {
    if (!dirty || recoveryPending || saveState === "conflict") return;
    const generation = generationRef.current;
    const candidate = form;
    setSaveState("dirty");
    const draftTimer = window.setTimeout(() => {
      void api.persistRecoveryDraft(makeInput(candidate)).catch((cause) => {
        setSaveState("error");
        onError(`復旧用の下書きを保存できなかった: ${cause instanceof Error ? cause.message : String(cause)}`);
      });
    }, 100);
    const saveTimer = window.setTimeout(() => void commit(candidate, generation), 700);
    return () => {
      window.clearTimeout(draftTimer);
      window.clearTimeout(saveTimer);
    };
    // committedRef and blockRef intentionally stay mutable so in-flight saves cannot overwrite newer text.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [form, recoveryPending]);

  const acceptRecovery = async () => {
    const generation = generationRef.current;
    void commit(formRef.current, generation, true);
    setRecoveryPending(false);
    onRecoveryResolved();
  };

  const discardRecovery = async () => {
    if (!recovery) return;
    try {
      await api.discardRecoveryDraft(block.id, recovery.contentSha256);
      const reset = formFrom(block, "自動保存");
      formRef.current = reset;
      setForm(reset);
      committedRef.current = toBlockSnapshot(block);
      setRecoveryPending(false);
      setSaveState("saved");
      onRecoveryResolved();
    } catch (cause) {
      onError(cause instanceof Error ? cause.message : String(cause));
    }
  };

  const insertMarkdown = (before: string, after = "") => {
    const editor = editorRef.current;
    if (!editor) return;
    const source = documentTab === "hypothesis" ? form.bodyMarkdown : form.researchNotesMarkdown;
    const start = editor.selectionStart;
    const end = editor.selectionEnd;
    const next = `${source.slice(0, start)}${before}${source.slice(start, end)}${after}${source.slice(end)}`;
    updateForm(documentTab === "hypothesis" ? { bodyMarkdown: next } : { researchNotesMarkdown: next });
    requestAnimationFrame(() => {
      editor.focus();
      editor.setSelectionRange(start + before.length, end + before.length);
    });
  };

  const markdown = documentTab === "hypothesis" ? form.bodyMarkdown : form.researchNotesMarkdown;

  return (
    <section className="editor-pane" aria-label="ブロック編集">
      {recoveryPending && (
        <div className="recovery-banner" role="alert">
          <div><span><strong>未保存の編集が見つかった</strong> · {formatDateTime(recovery!.updatedAt)}</span></div>
          <div className="inline-actions"><button className="button secondary" onClick={() => void discardRecovery()}>破棄</button><button className="button primary" onClick={() => void acceptRecovery()}>復元して保存</button></div>
        </div>
      )}

      <header className="editor-header">
        <div className="block-identity">
          <span className="block-id">{block.id.slice(0, 8).toUpperCase()}</span>
          <select className="quiet-select" value={form.kind} onChange={(event) => updateForm({ kind: event.target.value as BlockKind })} aria-label="種類">
            {kinds.map((kind) => <option key={kind} value={kind}>{blockKindLabel[kind]}</option>)}
          </select>
        </div>
        <div className="editor-header-actions">
          <SaveIndicator state={saveState} />
          <div className="segmented" aria-label="表示切替">
            <button className={viewMode === "editor" ? "active" : ""} onClick={() => setViewMode("editor")}>編集</button>
            <button className={viewMode === "split" ? "active" : ""} onClick={() => setViewMode("split")}>分割</button>
            <button className={viewMode === "preview" ? "active" : ""} onClick={() => setViewMode("preview")}>表示</button>
          </div>
        </div>
      </header>

      <div className="editor-meta">
        <input className="title-input" value={form.title} onChange={(event) => updateForm({ title: event.target.value })} aria-label="タイトル" />
        <div className="meta-row">
          <select className={`status-select status-${form.status.toLowerCase().replaceAll(" ", "-")}`} value={form.status} onChange={(event) => updateForm({ status: event.target.value as BlockStatus })}>
            {statuses.map((status) => <option key={status} value={status}>{blockStatusLabel[status]}</option>)}
          </select>
          <input className="tags-input" value={form.tags} onChange={(event) => updateForm({ tags: event.target.value })} placeholder="タグ（カンマ区切り）" aria-label="タグ" />
          <input className="reason-input" value={form.changeReason} onChange={(event) => updateForm({ changeReason: event.target.value })} placeholder="変更理由" aria-label="変更理由" />
        </div>
      </div>

      <div className="document-tabs">
        <button className={documentTab === "hypothesis" ? "active" : ""} onClick={() => setDocumentTab("hypothesis")}>本文</button>
        <button className={documentTab === "notes" ? "active" : ""} onClick={() => setDocumentTab("notes")}>研究ノート</button>
      </div>

      <div className="markdown-toolbar" aria-label="Markdownツール">
        <button onClick={() => insertMarkdown("**", "**")} title="太字"><strong>B</strong></button>
        <button onClick={() => insertMarkdown("_", "_")} title="斜体"><em>I</em></button>
        <button onClick={() => insertMarkdown("$", "$")} title="行内数式">数式</button>
        <button onClick={() => insertMarkdown("\n$$\n", "\n$$\n")} title="別行数式">∑</button>
        <button onClick={() => insertMarkdown("\n```python\n", "\n```\n")} title="コードブロック">{`</>`}</button>
        <button onClick={() => insertMarkdown("\n> ")} title="引用">引用</button>
        <button onClick={() => insertMarkdown("\n- [ ] ")} title="タスク">☐</button>
        <button onClick={() => insertMarkdown("[^1]", "\n\n[^1]: ")} title="脚注">¹</button>
        <button onClick={() => insertMarkdown("[@", "]")} title="文献引用">@</button>
        <span className="toolbar-spacer" />
        <button title="元に戻す" onClick={() => document.execCommand("undo")}>戻す</button>
        <button title="やり直す" onClick={() => document.execCommand("redo")}>進む</button>
      </div>

      <div className={`editor-workspace view-${viewMode}`}>
        {viewMode !== "preview" && (
          <textarea
            ref={editorRef}
            className="markdown-input"
            spellCheck
            value={markdown}
            onChange={(event) => updateForm(documentTab === "hypothesis" ? { bodyMarkdown: event.target.value } : { researchNotesMarkdown: event.target.value })}
            aria-label={`${documentTab} Markdown editor`}
          />
        )}
        {viewMode !== "editor" && (
          <article className="markdown-preview">
            {markdown.trim() ? (
              <ReactMarkdown
                remarkPlugins={[remarkGfm, remarkMath]}
                rehypePlugins={[rehypeKatex]}
                components={{ a: ({ children, href }) => <a href={href} onClick={(event) => event.preventDefault()} title="プレビューでは外部リンクを開かない">{children}</a> }}
              >{markdown}</ReactMarkdown>
            ) : (
              <p className="preview-placeholder">プレビュー</p>
            )}
          </article>
        )}
      </div>
    </section>
  );
}

function SaveIndicator({ state }: { state: SaveState }) {
  const content: Record<SaveState, string> = {
    saved: "保存済み",
    dirty: "編集中",
    saving: "保存中",
    conflict: "編集が競合",
    error: "保存失敗",
    recovered: "復旧データあり",
  };
  return <span className={`save-state save-${state}`}>{content[state]}</span>;
}
