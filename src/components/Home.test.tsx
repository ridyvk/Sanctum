import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import Home from "./Home";

const mocks = vi.hoisted(() => ({
  pluginStatus: vi.fn(),
  installPlugin: vi.fn(),
  openPlugin: vi.fn(),
  tunnelStatus: vi.fn(),
  configureTunnel: vi.fn(),
  startTunnel: vi.fn(),
  stopTunnel: vi.fn(),
  openTunnelSettings: vi.fn(),
  openConnectors: vi.fn(),
  openTunnelAdmin: vi.fn(),
  dialogOpen: vi.fn(),
}));

vi.mock("../api", () => ({
  isDesktopRuntime: () => true,
  isAndroidRuntime: () => false,
  api: {
    chatGptPluginStatus: mocks.pluginStatus,
    installChatGptPlugin: mocks.installPlugin,
    openChatGptPlugin: mocks.openPlugin,
    chatGptTunnelStatus: mocks.tunnelStatus,
    configureChatGptTunnel: mocks.configureTunnel,
    startChatGptTunnel: mocks.startTunnel,
    stopChatGptTunnel: mocks.stopTunnel,
    openChatGptTunnelSettings: mocks.openTunnelSettings,
    openChatGptConnectors: mocks.openConnectors,
    openChatGptTunnelAdmin: mocks.openTunnelAdmin,
  },
}));
vi.mock("./AppUpdater", () => ({ default: () => <span>Updater</span> }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: mocks.dialogOpen, save: vi.fn() }));

const uninstalled = {
  installed: false,
  pluginPath: "C:\\Users\\Keiya\\plugins\\sanctum",
  marketplacePath: "C:\\Users\\Keiya\\.agents\\plugins\\marketplace.json",
  deepLink: "codex://plugins/sanctum",
};

const tunnelUnconfigured = {
  configured: false,
  running: false,
  ready: false,
  tunnelId: null,
  clientPath: null,
  hasCredential: false,
  adminUiUrl: null,
  mcpUrl: "http://127.0.0.1:43991/mcp",
  lastError: null,
};

describe("Home ChatGPT connection", () => {
  afterEach(cleanup);

  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    mocks.pluginStatus.mockResolvedValue(uninstalled);
    mocks.installPlugin.mockResolvedValue({ ...uninstalled, installed: true });
    mocks.openPlugin.mockResolvedValue(undefined);
    mocks.tunnelStatus.mockResolvedValue(tunnelUnconfigured);
    mocks.openTunnelSettings.mockResolvedValue(undefined);
    mocks.openConnectors.mockResolvedValue(undefined);
    mocks.openTunnelAdmin.mockResolvedValue(undefined);
  });

  it("installs the personal Sanctum plugin from the Home screen", async () => {
    render(<Home onOpened={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: "ChatGPT接続" }));
    fireEvent.click(await screen.findByRole("tab", { name: "Codex / Work" }));
    fireEvent.click(await screen.findByRole("button", { name: "登録する" }));

    await waitFor(() => expect(mocks.installPlugin).toHaveBeenCalledOnce());
    expect(await screen.findByText("登録済み")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Codexで開く" })).toBeInTheDocument();
  });

  it("stores the private tunnel configuration and starts it", async () => {
    const configured = {
      ...tunnelUnconfigured,
      configured: true,
      running: true,
      ready: true,
      hasCredential: true,
      tunnelId: "tunnel_0123456789abcdef0123456789abcdef",
      clientPath: "C:\\Users\\Keiya\\AppData\\Local\\Sanctum\\bin\\tunnel-client.exe",
      adminUiUrl: "http://127.0.0.1:49152/ui",
    };
    mocks.dialogOpen.mockResolvedValue("C:\\Users\\Keiya\\Downloads\\tunnel-client.exe");
    mocks.configureTunnel.mockResolvedValue(configured);
    render(<Home onOpened={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: "ChatGPT接続" }));
    await screen.findByText("未設定");
    fireEvent.change(screen.getByLabelText("Tunnel ID"), {
      target: { value: "tunnel_0123456789abcdef0123456789abcdef" },
    });
    fireEvent.change(screen.getByLabelText("Runtime API key"), {
      target: { value: "sk-runtime-secret-value" },
    });
    fireEvent.click(screen.getByRole("button", { name: "選択" }));
    await waitFor(() => expect(mocks.dialogOpen).toHaveBeenCalledOnce());
    fireEvent.click(screen.getByRole("button", { name: "保存して起動" }));

    await waitFor(() => expect(mocks.configureTunnel).toHaveBeenCalledWith(
      "tunnel_0123456789abcdef0123456789abcdef",
      "sk-runtime-secret-value",
      "C:\\Users\\Keiya\\Downloads\\tunnel-client.exe",
    ));
    expect(await screen.findByText("Tunnel準備完了")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "ChatGPTで追加" })).toBeInTheDocument();
  });
});
