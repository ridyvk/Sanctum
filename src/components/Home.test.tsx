import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import Home from "./Home";

const mocks = vi.hoisted(() => ({
  status: vi.fn(),
  install: vi.fn(),
  open: vi.fn(),
}));

vi.mock("../api", () => ({
  isDesktopRuntime: () => true,
  api: {
    chatGptPluginStatus: mocks.status,
    installChatGptPlugin: mocks.install,
    openChatGptPlugin: mocks.open,
  },
}));
vi.mock("./AppUpdater", () => ({ default: () => <span>Updater</span> }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));

const uninstalled = {
  installed: false,
  pluginPath: "C:\\Users\\Keiya\\plugins\\sanctum",
  marketplacePath: "C:\\Users\\Keiya\\.agents\\plugins\\marketplace.json",
  deepLink: "codex://plugins/sanctum",
};

describe("Home ChatGPT connection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    mocks.status.mockResolvedValue(uninstalled);
    mocks.install.mockResolvedValue({ ...uninstalled, installed: true });
    mocks.open.mockResolvedValue(undefined);
  });

  it("installs the personal Sanctum plugin from the Home screen", async () => {
    render(<Home onOpened={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: "ChatGPT接続" }));
    fireEvent.click(await screen.findByRole("button", { name: "登録する" }));

    await waitFor(() => expect(mocks.install).toHaveBeenCalledOnce());
    expect(await screen.findByText("登録済み")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "ChatGPTで開く" })).toBeInTheDocument();
  });
});
