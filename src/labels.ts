import type { AttachmentRelation, BlockKind, BlockStatus, EdgeType, SnapshotKind } from "./types";

export const blockKindLabel: Record<BlockKind, string> = {
  Hypothesis: "仮説",
  Assumption: "前提",
  Method: "手法",
  Evidence: "根拠",
};

export const blockStatusLabel: Record<BlockStatus, string> = {
  Idea: "アイデア",
  Developing: "検討中",
  Testing: "検証中",
  Supported: "支持",
  "Weakly Supported": "弱い支持",
  Rejected: "棄却",
  Archived: "アーカイブ",
};

export const edgeTypeLabel: Record<EdgeType, string> = {
  Supports: "支持する",
  Contradicts: "矛盾する",
  "Depends on": "依存する",
  "Derived from": "導出元",
  Assumes: "前提とする",
  Extends: "拡張する",
  Tests: "検証する",
  "Alternative to": "代替案",
  "Related to": "関連",
};

export const attachmentRelationLabel: Record<AttachmentRelation, string> = {
  Supports: "支持",
  Contradicts: "反証",
  Background: "背景資料",
  Method: "手法",
  Dataset: "データ",
  Reference: "参考資料",
  Other: "その他",
};

export const snapshotKindLabel: Record<SnapshotKind, string> = {
  TenMinute: "10分",
  Daily: "日次",
  Weekly: "週次",
  Manual: "手動",
};

export const formatDateTime = (value: string) => new Date(value).toLocaleString("ja-JP");
export const formatDate = (value: string) => new Date(value).toLocaleDateString("ja-JP");
