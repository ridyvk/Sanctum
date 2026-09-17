import type { VaultSummary } from "./types";

const KEY = "sanctum.recentVaults.v1";
const LIMIT = 12;

export interface RecentVault {
  vaultId: string;
  name: string;
  path: string;
  lastOpenedAt: string;
}

export function loadRecentVaults(storage: Pick<Storage, "getItem"> = localStorage): RecentVault[] {
  try {
    const parsed = JSON.parse(storage.getItem(KEY) ?? "[]") as unknown;
    if (!Array.isArray(parsed)) return [];
    return parsed.filter((entry): entry is RecentVault => {
      if (!entry || typeof entry !== "object") return false;
      const value = entry as Record<string, unknown>;
      return ["vaultId", "name", "path", "lastOpenedAt"].every((key) => typeof value[key] === "string");
    }).slice(0, LIMIT);
  } catch {
    return [];
  }
}

export function rememberVault(
  summary: VaultSummary,
  storage: Pick<Storage, "getItem" | "setItem"> = localStorage,
): RecentVault[] {
  const current = loadRecentVaults(storage);
  const next = [
    {
      vaultId: summary.vaultId,
      name: summary.name,
      path: summary.path,
      lastOpenedAt: new Date().toISOString(),
    },
    ...current.filter((item) => item.path !== summary.path && item.vaultId !== summary.vaultId),
  ].slice(0, LIMIT);
  storage.setItem(KEY, JSON.stringify(next));
  return next;
}

export const recentVaultStorageKey = KEY;
