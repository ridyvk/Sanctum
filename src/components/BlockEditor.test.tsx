import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { api } from "../api";
import type { HypothesisBlock, RecoveryDraft } from "../types";
import BlockEditor from "./BlockEditor";

const block: HypothesisBlock = {
  id: "block-0001",
  kind: "Hypothesis",
  title: "最初の仮説",
  bodyMarkdown: "本文",
  researchNotesMarkdown: "",
  status: "Idea",
  tags: [],
  currentVersionId: "version-1",
  rowVersion: 1,
  parentBlockId: null,
  createdAt: "2026-09-17T00:00:00Z",
  updatedAt: "2026-09-17T00:00:00Z",
  deletedAt: null,
};

describe("BlockEditor autosave", () => {
  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("does not save an unchanged block", async () => {
    vi.useFakeTimers();
    const saveBlock = vi.spyOn(api, "saveBlock").mockResolvedValue(block);
    render(<BlockEditor block={block} recovery={null} onSaved={() => undefined} onRecoveryResolved={() => undefined} onError={() => undefined} />);

    await act(async () => { await vi.advanceTimersByTimeAsync(2_000); });

    expect(saveBlock).not.toHaveBeenCalled();
    expect(screen.getByText("保存済み")).toBeInTheDocument();
  });

  it("saves one version for one edit and then becomes idle", async () => {
    vi.useFakeTimers();
    const saved = { ...block, title: "更新した仮説", currentVersionId: "version-2", rowVersion: 2 };
    const draft: RecoveryDraft = {
      blockId: block.id,
      baseRowVersion: 1,
      snapshot: { ...block, title: "更新した仮説" },
      contentSha256: "a".repeat(64),
      updatedAt: "2026-09-17T00:00:01Z",
    };
    vi.spyOn(api, "persistRecoveryDraft").mockResolvedValue(draft);
    const saveBlock = vi.spyOn(api, "saveBlock").mockResolvedValue(saved);
    render(<BlockEditor block={block} recovery={null} onSaved={() => undefined} onRecoveryResolved={() => undefined} onError={() => undefined} />);

    fireEvent.change(screen.getByRole("textbox", { name: "タイトル" }), { target: { value: "更新した仮説" } });
    await act(async () => { await vi.advanceTimersByTimeAsync(1_000); });
    await act(async () => { await vi.advanceTimersByTimeAsync(2_000); });

    expect(saveBlock).toHaveBeenCalledTimes(1);
    expect(screen.getByText("保存済み")).toBeInTheDocument();
  });

  it("expands the preview for both the body and research notes", () => {
    const withNotes = { ...block, researchNotesMarkdown: "# 研究ノート本文" };
    const { container } = render(<BlockEditor block={withNotes} recovery={null} onSaved={() => undefined} onRecoveryResolved={() => undefined} onError={() => undefined} />);

    fireEvent.click(screen.getByRole("button", { name: "表示" }));
    fireEvent.click(screen.getByRole("button", { name: "大画面" }));

    expect(container.querySelector(".editor-pane")).toHaveClass("preview-expanded");
    expect(screen.getByRole("button", { name: "大画面を閉じる" })).toBeInTheDocument();
    expect(container.querySelector(".markdown-preview")).toHaveTextContent("本文");

    fireEvent.click(screen.getByRole("button", { name: "研究ノート" }));
    expect(container.querySelector(".markdown-preview")).toHaveTextContent("研究ノート本文");

    fireEvent.keyDown(window, { key: "Escape" });
    expect(container.querySelector(".editor-pane")).not.toHaveClass("preview-expanded");
  });
});
