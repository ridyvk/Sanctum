import { useEffect, useState } from "react";
import { api } from "../api";
import { formatDateTime } from "../labels";
import type { IntegrityFinding, IntegrityReport } from "../types";

interface Props {
  onSelectBlock: (id: string) => void;
  onError: (message: string) => void;
}

export default function IntegrityView({ onSelectBlock, onError }: Props) {
  const [report, setReport] = useState<IntegrityReport | null>(null);
  const [loading, setLoading] = useState(true);

  const run = async () => {
    setLoading(true);
    try {
      setReport(await api.integrity());
    } catch (cause) {
      onError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => { void run(); }, []);

  const warnings = report?.findings.filter((finding) => finding.severity === "warning") ?? [];
  const fatals = report?.findings.filter((finding) => finding.severity === "fatal") ?? [];

  return (
    <section className="integrity-page">
      <header className="page-header">
        <div><h2>整合性検査</h2></div>
        <button className="button secondary" disabled={loading} onClick={() => void run()}>{loading ? "検査中" : "検査"}</button>
      </header>

      {loading && !report ? (
        <div className="center-message">検査中</div>
      ) : report && (
        <>
          <div className="integrity-summary">
            <SummaryCard value={report.healthyHypotheses} label="正常な仮説" tone="healthy" />
            <SummaryCard value={warnings.length} label="警告" tone="warning" />
            <SummaryCard value={fatals.length} label="重大" tone="fatal" />
            <SummaryCard value={`r${report.vaultRevision}`} label="検査リビジョン" tone="neutral" />
          </div>

          <div className="integrity-groups">
            {fatals.length > 0 && <FindingGroup title="対応が必要" description="原因を確認するまで新しい復旧点を作らない" findings={fatals} onSelectBlock={onSelectBlock} />}
            {warnings.length > 0 && <FindingGroup title="警告" description="研究構造または復旧状態を確認する" findings={warnings} onSelectBlock={onSelectBlock} />}
            {fatals.length === 0 && warnings.length === 0 && (
              <div className="integrity-clear"><div><h3>問題なし</h3></div></div>
            )}
          </div>

          <footer className="integrity-footer">{formatDateTime(report.checkedAt)}に検査。外部Backupは別に必要</footer>
        </>
      )}
    </section>
  );
}

function SummaryCard({ value, label, tone }: { value: number | string; label: string; tone: string }) {
  return <div className={`summary-card ${tone}`}><strong>{value}</strong><p>{label}</p></div>;
}

function FindingGroup({ title, description, findings, onSelectBlock }: { title: string; description: string; findings: IntegrityFinding[]; onSelectBlock: (id: string) => void }) {
  return (
    <section className="finding-group">
      <header><div><h3>{title}</h3><p>{description}</p></div><span>{findings.length}</span></header>
      <div className="finding-list">
        {findings.map((finding, index) => (
          <button key={`${finding.code}-${finding.entityId}-${index}`} onClick={() => finding.entityId && onSelectBlock(finding.entityId)} disabled={!finding.entityId}>
            <div><strong>{finding.message}</strong><code>{finding.code}</code></div>
          </button>
        ))}
      </div>
    </section>
  );
}
