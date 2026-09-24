import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import Home from "./Home";

const mocks = vi.hoisted(() => ({
  list: vi.fn(), create: vi.fn(), open: vi.fn(), prepare: vi.fn(), restore: vi.fn(), discard: vi.fn(),
  dialogOpen: vi.fn(), readFile: vi.fn(), writeFile: vi.fn(),
}));

vi.mock("../api", () => ({
  isDesktopRuntime: () => true,
  isAndroidRuntime: () => true,
  api: {
    listMobileVaults: mocks.list,
    createMobileVault: mocks.create,
    openMobileVault: mocks.open,
    prepareMobileImport: mocks.prepare,
    restoreMobileBackup: mocks.restore,
    discardMobileImport: mocks.discard,
  },
}));
vi.mock("./AppUpdater", () => ({ default: () => null }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: mocks.dialogOpen, save: vi.fn() }));
vi.mock("@tauri-apps/plugin-fs", () => ({ readFile: mocks.readFile, writeFile: mocks.writeFile }));

const vault = {
  vaultId: "vault-1", name: "寿命の研究", path: "/private/vaults/0000.sanctum",
  createdAt: "2026-09-24T00:00:00Z", revision: 1,
};

describe("Android home and Vault transfer", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    mocks.list.mockResolvedValue([vault]);
    mocks.open.mockResolvedValue(vault);
    mocks.create.mockResolvedValue(vault);
    mocks.prepare.mockResolvedValue({ id: "transfer-1", path: "/private/imports/incoming.sanctum-backup" });
    mocks.restore.mockResolvedValue(vault);
    mocks.discard.mockResolvedValue(undefined);
    mocks.dialogOpen.mockResolvedValue("content://documents/backup");
    mocks.readFile.mockResolvedValue(new Uint8Array([1, 2, 3]));
    mocks.writeFile.mockResolvedValue(undefined);
  });
  afterEach(cleanup);

  it("opens an existing private Vault and creates a new one without a folder picker", async () => {
    const opened = vi.fn();
    const { unmount } = render(<Home onOpened={opened} />);
    const project = await screen.findByRole("button", { name: /寿命の研究/ });
    expect(screen.queryByText("ブラウザ表示")).not.toBeInTheDocument();
    expect(project).not.toHaveTextContent(vault.path);
    fireEvent.click(project);
    await waitFor(() => expect(mocks.open).toHaveBeenCalledWith(vault.path));
    expect(opened).toHaveBeenCalledWith(vault);
    unmount();

    render(<Home onOpened={opened} />);
    fireEvent.click(screen.getByRole("button", { name: "新規作成" }));
    expect(screen.getByText("このスマホ内に保存する")).toBeInTheDocument();
    fireEvent.change(screen.getByPlaceholderText("無期限寿命の経済学"), { target: { value: "新しい研究" } });
    fireEvent.click(screen.getByRole("button", { name: /^作成$/ }));
    await waitFor(() => expect(mocks.create).toHaveBeenCalledWith("新しい研究"));
    expect(screen.queryByText("ChatGPT接続")).not.toBeInTheDocument();
  });

  it("stages a selected encrypted Backup and removes the temporary copy after restore", async () => {
    render(<Home onOpened={vi.fn()} />);
    await screen.findByRole("button", { name: /寿命の研究/ });
    fireEvent.click(screen.getByRole("button", { name: "Backupを読み込む" }));
    fireEvent.click(screen.getByRole("button", { name: "選択" }));
    await waitFor(() => expect(mocks.dialogOpen).toHaveBeenCalledOnce());
    fireEvent.change(screen.getByPlaceholderText("12文字以上"), { target: { value: "safe-password-123" } });
    fireEvent.click(screen.getByRole("button", { name: "復元する" }));
    await waitFor(() => expect(mocks.restore).toHaveBeenCalledWith("transfer-1", "incoming.sanctum-backup", "safe-password-123"));
    expect(mocks.writeFile).toHaveBeenCalledWith("/private/imports/incoming.sanctum-backup", new Uint8Array([1, 2, 3]));
    expect(mocks.discard).toHaveBeenCalledWith("transfer-1", "incoming.sanctum-backup");
  });
});
