import { useCallback, useEffect, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { Download, LoaderCircle, RefreshCw } from "lucide-react";

interface Props {
  desktop: boolean;
  autoCheck?: boolean;
  onInstallStateChange?: (installing: boolean) => void;
}

type Phase = "idle" | "checking" | "current" | "available" | "downloading" | "error";

export default function AppUpdater({ desktop, autoCheck = true, onInstallStateChange }: Props) {
  const updateRef = useRef<Update | null>(null);
  const mountedRef = useRef(true);
  const busyRef = useRef(false);
  const [version, setVersion] = useState("");
  const [phase, setPhase] = useState<Phase>("idle");
  const [availableVersion, setAvailableVersion] = useState("");
  const [downloaded, setDownloaded] = useState(0);
  const [total, setTotal] = useState<number | null>(null);

  const checkNow = useCallback(async (manual: boolean) => {
    if (!desktop || busyRef.current) return;
    busyRef.current = true;
    setPhase("checking");
    try {
      const found = await check({ timeout: 15_000 });
      if (!mountedRef.current) {
        await found?.close();
        return;
      }
      if (updateRef.current && updateRef.current !== found) await updateRef.current.close();
      updateRef.current = found;
      if (found) {
        setAvailableVersion(found.version);
        setPhase("available");
      } else {
        setAvailableVersion("");
        setPhase("current");
      }
    } catch {
      if (mountedRef.current) setPhase(manual ? "error" : "idle");
    } finally {
      busyRef.current = false;
    }
  }, [desktop]);

  useEffect(() => {
    mountedRef.current = true;
    if (!desktop) return;
    void getVersion().then((value) => {
      if (mountedRef.current) setVersion(value);
    });
    const timer = autoCheck ? window.setTimeout(() => void checkNow(false), 1_200) : undefined;
    return () => {
      mountedRef.current = false;
      if (timer !== undefined) window.clearTimeout(timer);
      const pending = updateRef.current;
      updateRef.current = null;
      void pending?.close();
    };
  }, [autoCheck, checkNow, desktop]);

  const install = async () => {
    const pending = updateRef.current;
    if (!pending || busyRef.current) return;
    busyRef.current = true;
    setDownloaded(0);
    setTotal(null);
    setPhase("downloading");
    onInstallStateChange?.(true);
    try {
      await pending.downloadAndInstall((event) => {
        if (event.event === "Started") {
          setTotal(event.data.contentLength ?? null);
        } else if (event.event === "Progress") {
          setDownloaded((value) => value + event.data.chunkLength);
        }
      });
      await relaunch();
    } catch {
      if (mountedRef.current) setPhase("error");
    } finally {
      busyRef.current = false;
      onInstallStateChange?.(false);
    }
  };

  if (!desktop) return null;

  const percent = total && total > 0 ? Math.min(100, Math.round((downloaded / total) * 100)) : null;

  return (
    <div className="app-updater" aria-live="polite">
      <span>{version ? `v${version}` : "Sanctum"}</span>
      {phase === "available" ? (
        <button type="button" onClick={() => void install()}>
          <Download size={13} /> v{availableVersion}へ更新
        </button>
      ) : phase === "downloading" ? (
        <span className="update-progress"><LoaderCircle className="spin" size={13} /> 更新中{percent === null ? "" : ` ${percent}%`}</span>
      ) : (
        <button type="button" disabled={phase === "checking"} onClick={() => void checkNow(true)}>
          {phase === "checking" ? <LoaderCircle className="spin" size={13} /> : <RefreshCw size={13} />}
          {phase === "checking" ? "確認中" : phase === "current" ? "最新版" : "更新を確認"}
        </button>
      )}
      {phase === "error" && <span className="update-error">更新を確認できなかった</span>}
    </div>
  );
}
