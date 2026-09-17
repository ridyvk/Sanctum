import { describe, expect, it } from "vitest";
import { loadRecentVaults, recentVaultStorageKey, rememberVault } from "./recentVaults";
import type { VaultSummary } from "./types";

function memoryStorage(initial: string | null = null) {
  let value = initial;
  return {
    getItem: (key: string) => key === recentVaultStorageKey ? value : null,
    setItem: (key: string, next: string) => { if (key === recentVaultStorageKey) value = next; },
    value: () => value,
  };
}

const summary = (id: number): VaultSummary => ({
  vaultId: `vault-${id}`,
  name: `Research ${id}`,
  path: `/research/${id}.sanctum`,
  createdAt: "2026-01-01T00:00:00.000Z",
  revision: id,
});

describe("recent Vault metadata", () => {
  it("fails closed to an empty list when local metadata is corrupt", () => {
    expect(loadRecentVaults(memoryStorage("not json"))).toEqual([]);
  });

  it("deduplicates by Vault identity and path", () => {
    const storage = memoryStorage();
    rememberVault(summary(1), storage);
    rememberVault({ ...summary(1), name: "Renamed" }, storage);
    expect(loadRecentVaults(storage)).toHaveLength(1);
    expect(loadRecentVaults(storage)[0].name).toBe("Renamed");
  });

  it("retains only the twelve most recent projects", () => {
    const storage = memoryStorage();
    for (let index = 0; index < 20; index += 1) rememberVault(summary(index), storage);
    const recent = loadRecentVaults(storage);
    expect(recent).toHaveLength(12);
    expect(recent[0].vaultId).toBe("vault-19");
  });
});

