import { memo, useCallback, useMemo, useState } from "react";
import {
  Background,
  Controls,
  Handle,
  MarkerType,
  MiniMap,
  Position,
  ReactFlow,
  type Connection,
  type Edge,
  type Node,
  type NodeProps,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { Filter, Link2, Search, X } from "lucide-react";
import { api } from "../api";
import { blockKindLabel, blockStatusLabel, edgeTypeLabel } from "../labels";
import type { BlockStatus, EdgeType, GraphData, HypothesisBlock } from "../types";

interface Props {
  data: GraphData;
  selectedBlockId: string | null;
  onSelectBlock: (id: string) => void;
  onRefresh: () => Promise<void>;
  onError: (message: string) => void;
}

interface ResearchNodeData extends Record<string, unknown> {
  block: HypothesisBlock;
}

const edgeTypes: EdgeType[] = [
  "Supports",
  "Contradicts",
  "Depends on",
  "Derived from",
  "Assumes",
  "Extends",
  "Tests",
  "Alternative to",
  "Related to",
];

const statuses: BlockStatus[] = ["Idea", "Developing", "Testing", "Supported", "Weakly Supported", "Rejected", "Archived"];

const statusColor: Record<BlockStatus, string> = {
  Idea: "#99a2ad",
  Developing: "#7b9fc5",
  Testing: "#d4a94e",
  Supported: "#69aa86",
  "Weakly Supported": "#b9a568",
  Rejected: "#c36b6b",
  Archived: "#737985",
};

const edgeColor: Record<EdgeType, string> = {
  Supports: "#62a67f",
  Contradicts: "#c46868",
  "Depends on": "#8f83c5",
  "Derived from": "#6999b7",
  Assumes: "#b28d62",
  Extends: "#68a2a0",
  Tests: "#d0a34f",
  "Alternative to": "#aa78a9",
  "Related to": "#7f8791",
};

const ResearchNode = memo(({ data, selected }: NodeProps<Node<ResearchNodeData>>) => {
  const block = data.block;
  return (
    <div className={`research-node ${selected ? "selected" : ""}`} style={{ "--node-accent": statusColor[block.status] } as React.CSSProperties}>
      <Handle type="target" position={Position.Left} />
      <div className="research-node-head"><span>{blockKindLabel[block.kind]}</span><i /></div>
      <strong>{block.title}</strong>
      <p>{block.id.slice(0, 8).toUpperCase()} · {blockStatusLabel[block.status]}</p>
      <Handle type="source" position={Position.Right} />
    </div>
  );
});
ResearchNode.displayName = "ResearchNode";

const nodeTypes = { research: ResearchNode };

export default function ResearchGraph({ data, selectedBlockId, onSelectBlock, onRefresh, onError }: Props) {
  const [query, setQuery] = useState("");
  const [selectedEdgeType, setSelectedEdgeType] = useState<EdgeType>("Related to");
  const [statusFilter, setStatusFilter] = useState<Set<BlockStatus>>(new Set(statuses));
  const [edgeFilter, setEdgeFilter] = useState<Set<EdgeType>>(new Set(edgeTypes));
  const [showFilters, setShowFilters] = useState(false);

  const visibleIds = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    return new Set(data.blocks.filter((block) =>
      statusFilter.has(block.status)
      && (!normalized || `${block.title} ${block.tags.join(" ")} ${block.bodyMarkdown}`.toLowerCase().includes(normalized)),
    ).map((block) => block.id));
  }, [data.blocks, query, statusFilter]);

  const positionMap = useMemo(
    () => new Map(data.positions.filter((position) => position.viewId === "main").map((position) => [position.blockId, position])),
    [data.positions],
  );

  const nodes = useMemo<Node<ResearchNodeData>[]>(() => data.blocks
    .filter((block) => visibleIds.has(block.id))
    .map((block, index) => {
      const saved = positionMap.get(block.id);
      return {
        id: block.id,
        type: "research",
        data: { block },
        position: saved ?? { x: (index % 5) * 270, y: Math.floor(index / 5) * 165 },
        selected: selectedBlockId === block.id,
      };
    }), [data.blocks, positionMap, selectedBlockId, visibleIds]);

  const edges = useMemo<Edge[]>(() => data.edges
    .filter((edge) => visibleIds.has(edge.sourceBlockId) && visibleIds.has(edge.targetBlockId) && edgeFilter.has(edge.edgeType))
    .map((edge) => ({
      id: edge.id,
      source: edge.sourceBlockId,
      target: edge.targetBlockId,
      label: edgeTypeLabel[edge.edgeType],
      type: "smoothstep",
      markerEnd: { type: MarkerType.ArrowClosed, color: edgeColor[edge.edgeType] },
      style: { stroke: edgeColor[edge.edgeType], strokeWidth: 1.5 },
      labelStyle: { fill: edgeColor[edge.edgeType], fontSize: 10, fontWeight: 600 },
      labelBgStyle: { fill: "var(--surface-1)", fillOpacity: 0.92 },
    })), [data.edges, edgeFilter, visibleIds]);

  const connect = useCallback(async (connection: Connection) => {
    if (!connection.source || !connection.target) return;
    try {
      await api.createEdge(connection.source, connection.target, selectedEdgeType);
      await onRefresh();
    } catch (cause) {
      onError(cause instanceof Error ? cause.message : String(cause));
    }
  }, [onError, onRefresh, selectedEdgeType]);

  const toggleStatus = (status: BlockStatus) => setStatusFilter((current) => {
    const next = new Set(current);
    next.has(status) ? next.delete(status) : next.add(status);
    return next;
  });
  const toggleEdge = (type: EdgeType) => setEdgeFilter((current) => {
    const next = new Set(current);
    next.has(type) ? next.delete(type) : next.add(type);
    return next;
  });

  return (
    <section className="graph-pane" aria-label="研究グラフ">
      <header className="graph-toolbar">
        <div className="graph-search"><Search size={15} /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="グラフを検索" />{query && <button onClick={() => setQuery("")} aria-label="検索を消す"><X size={13} /></button>}</div>
        <div className="edge-mode"><Link2 size={14} /><span>接続</span><select value={selectedEdgeType} onChange={(event) => setSelectedEdgeType(event.target.value as EdgeType)}>{edgeTypes.map((type) => <option key={type} value={type}>{edgeTypeLabel[type]}</option>)}</select></div>
        <button className={`button compact ${showFilters ? "active" : "secondary"}`} onClick={() => setShowFilters((value) => !value)}><Filter size={14} /> 絞り込み</button>
        <span className="graph-count">{nodes.length} ブロック · {edges.length} 関係</span>
      </header>
      {showFilters && (
        <div className="graph-filters">
          <div><span>状態</span>{statuses.map((status) => <label key={status}><input type="checkbox" checked={statusFilter.has(status)} onChange={() => toggleStatus(status)} /><i style={{ background: statusColor[status] }} />{blockStatusLabel[status]}</label>)}</div>
          <div><span>関係</span>{edgeTypes.map((type) => <label key={type}><input type="checkbox" checked={edgeFilter.has(type)} onChange={() => toggleEdge(type)} /><i style={{ background: edgeColor[type] }} />{edgeTypeLabel[type]}</label>)}</div>
        </div>
      )}
      <div className="graph-canvas">
        <ReactFlow
          nodes={nodes}
          edges={edges}
          nodeTypes={nodeTypes}
          onConnect={(connection) => void connect(connection)}
          onNodeClick={(_, node) => onSelectBlock(node.id)}
          onNodeDragStop={(_, node) => void api.setGraphPosition({ blockId: node.id, viewId: "main", x: node.position.x, y: node.position.y }).then(onRefresh).catch((cause) => onError(String(cause)))}
          fitView
          minZoom={0.15}
          maxZoom={2}
          onlyRenderVisibleElements
          nodesDraggable
          proOptions={{ hideAttribution: true }}
        >
          <Background color="var(--graph-grid)" gap={28} size={1} />
          <Controls showInteractive={false} />
          <MiniMap nodeColor={(node) => statusColor[(node.data as ResearchNodeData).block.status]} maskColor="var(--minimap-mask)" pannable zoomable />
        </ReactFlow>
      </div>
    </section>
  );
}
