import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HypothesisBlock, VaultSummary } from "../types";
import Workspace from "./Workspace";

const mocks = vi.hoisted(() => ({
  listBlocks: vi.fn(),
  graph: vi.fn(),
  summary: vi.fn(),
  getBlock: vi.fn(),
  recoveryDraft: vi.fn(),
  createDueSnapshots: vi.fn(),
  runDueAutomaticBackup: vi.fn(),
  softDeleteBlock: vi.fn(),
}));

vi.mock("../api", () => ({ api: mocks }));
vi.mock("./BlockEditor", () => ({
  default: ({ block }: { block: HypothesisBlock }) => <section aria-label="ブロック編集">{block.title}</section>,
}));
vi.mock("./Inspector", () => ({ default: () => <aside data-testid="inspector">Inspector</aside> }));

const block: HypothesisBlock = {
  id: "block-0001",
  kind: "Hypothesis",
  title: "最初の仮説",
  bodyMarkdown: "本文",
  researchNotesMarkdown: "研究ノート",
  status: "Idea",
  tags: [],
  currentVersionId: "version-1",
  rowVersion: 1,
  parentBlockId: null,
  createdAt: "2026-09-17T00:00:00Z",
  updatedAt: "2026-09-17T00:00:00Z",
  deletedAt: null,
};

const vault: VaultSummary = {
  vaultId: "vault-1",
  name: "研究Vault",
  path: "C:\\Research",
  createdAt: "2026-09-17T00:00:00Z",
  revision: 1,
};

describe("Workspace panels and block menu", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.listBlocks.mockResolvedValue([block]);
    mocks.graph.mockResolvedValue({ blocks: [block], edges: [], positions: [] });
    mocks.summary.mockResolvedValue(vault);
    mocks.getBlock.mockResolvedValue(block);
    mocks.recoveryDraft.mockResolvedValue(null);
    mocks.createDueSnapshots.mockResolvedValue([]);
    mocks.runDueAutomaticBackup.mockResolvedValue(null);
    mocks.softDeleteBlock.mockResolvedValue(undefined);
  });

  afterEach(cleanup);

  it("collapses either side panel so the main stage receives the freed width", async () => {
    const { container } = render(<Workspace vault={vault} onClose={vi.fn()} />);
    await screen.findByRole("button", { name: /最初の仮説/ });
    const grid = container.querySelector(".workspace-grid");

    fireEvent.click(screen.getByRole("button", { name: "左パネルを隠す" }));
    expect(grid).toHaveClass("navigator-hidden");
    expect(screen.getByRole("button", { name: "左パネルを表示" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "右パネルを隠す" }));
    expect(grid).toHaveClass("inspector-hidden");
    expect(screen.getByRole("button", { name: "右パネルを表示" })).toBeInTheDocument();
  });

  it("offers a recoverable delete action from each block context menu", async () => {
    mocks.listBlocks.mockResolvedValueOnce([block]).mockResolvedValueOnce([]);
    render(<Workspace vault={vault} onClose={vi.fn()} />);
    const blockButton = await screen.findByRole("button", { name: /最初の仮説/ });

    fireEvent.contextMenu(blockButton, { clientX: 120, clientY: 160 });
    fireEvent.click(screen.getByRole("menuitem", { name: "ゴミ箱へ移動" }));

    expect(screen.getByRole("dialog", { name: "ゴミ箱へ移動する？" })).toBeInTheDocument();
    expect(mocks.softDeleteBlock).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "移動する" }));

    await waitFor(() => expect(mocks.softDeleteBlock).toHaveBeenCalledWith(block.id, block.rowVersion));
    expect(await screen.findByRole("status")).toHaveTextContent("履歴は残っている");
  });
});
