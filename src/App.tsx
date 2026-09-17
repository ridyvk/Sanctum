import { lazy, Suspense, useEffect, useState } from "react";
import { Moon, Sun } from "lucide-react";
import { api } from "./api";
import Home from "./components/Home";
import { rememberVault } from "./recentVaults";
import type { VaultSummary } from "./types";

const Workspace = lazy(() => import("./components/Workspace"));

type Theme = "dark" | "light";

export default function App() {
  const [vault, setVault] = useState<VaultSummary | null>(null);
  const [theme, setTheme] = useState<Theme>(() =>
    localStorage.getItem("sanctum.theme") === "light" ? "light" : "dark",
  );

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    localStorage.setItem("sanctum.theme", theme);
  }, [theme]);

  const opened = (summary: VaultSummary) => {
    rememberVault(summary);
    setVault(summary);
  };

  const close = async () => {
    await api.closeVault();
    setVault(null);
  };

  return (
    <div className="app-shell">
      <button
        className="theme-toggle icon-button"
        aria-label={theme === "dark" ? "ライトモードに切替" : "ダークモードに切替"}
        onClick={() => setTheme((current) => (current === "dark" ? "light" : "dark"))}
      >
        {theme === "dark" ? <Sun size={16} /> : <Moon size={16} />}
      </button>
      {vault ? (
        <Suspense fallback={<div className="center-message">研究を開いている</div>}>
          <Workspace vault={vault} onClose={close} />
        </Suspense>
      ) : (
        <Home onOpened={opened} />
      )}
    </div>
  );
}
