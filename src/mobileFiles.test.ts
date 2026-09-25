import { beforeEach, describe, expect, it, vi } from "vitest";
import { saveMobileFile } from "./mobileFiles";

const mocks = vi.hoisted(() => ({
  save: vi.fn(),
  readFile: vi.fn(),
  writeFile: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({ save: mocks.save }));
vi.mock("@tauri-apps/plugin-fs", () => ({ readFile: mocks.readFile, writeFile: mocks.writeFile }));

describe("Android file export verification", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.save.mockResolvedValue("content://documents/export");
    mocks.writeFile.mockResolvedValue(undefined);
  });

  it("accepts a complete read-back without Web Crypto", async () => {
    const bytes = new Uint8Array([1, 2, 3]);
    mocks.readFile.mockResolvedValueOnce(bytes).mockResolvedValueOnce(new Uint8Array(bytes));
    await expect(saveMobileFile("/private/export", "backup.sanctum-backup")).resolves.toBe(true);
    expect(mocks.writeFile).toHaveBeenCalledWith("content://documents/export", bytes);
  });

  it("rejects a same-length corrupted document", async () => {
    mocks.readFile.mockResolvedValueOnce(new Uint8Array([1, 2, 3])).mockResolvedValueOnce(new Uint8Array([1, 9, 3]));
    await expect(saveMobileFile("/private/export", "backup.sanctum-backup")).rejects.toThrow("検証に失敗");
  });
});
