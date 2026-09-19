import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import AppUpdater from "./AppUpdater";

const mocks = vi.hoisted(() => ({
  check: vi.fn(),
  getVersion: vi.fn(),
  relaunch: vi.fn(),
}));

vi.mock("@tauri-apps/api/app", () => ({ getVersion: mocks.getVersion }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: mocks.check }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: mocks.relaunch }));

describe("AppUpdater", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.getVersion.mockResolvedValue("0.4.5");
    mocks.relaunch.mockResolvedValue(undefined);
  });

  it("shows the installed version and confirms when it is current", async () => {
    mocks.check.mockResolvedValue(null);
    render(<AppUpdater desktop autoCheck={false} />);

    expect(await screen.findByText("v0.4.5")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "更新を確認" }));

    expect(await screen.findByRole("button", { name: "最新版" })).toBeInTheDocument();
    expect(mocks.check).toHaveBeenCalledWith({ timeout: 15_000 });
  });

  it("downloads an available update and relaunches", async () => {
    const update = {
      version: "0.4.6",
      close: vi.fn().mockResolvedValue(undefined),
      downloadAndInstall: vi.fn(async (onEvent: (event: unknown) => void) => {
        onEvent({ event: "Started", data: { contentLength: 100 } });
        onEvent({ event: "Progress", data: { chunkLength: 50 } });
        onEvent({ event: "Finished" });
      }),
    };
    mocks.check.mockResolvedValue(update);
    render(<AppUpdater desktop autoCheck={false} />);

    fireEvent.click(await screen.findByRole("button", { name: "更新を確認" }));
    fireEvent.click(await screen.findByRole("button", { name: "v0.4.6へ更新" }));

    await waitFor(() => expect(update.downloadAndInstall).toHaveBeenCalledOnce());
    await waitFor(() => expect(mocks.relaunch).toHaveBeenCalledOnce());
  });
});
