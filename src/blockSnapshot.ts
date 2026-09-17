import type { BlockSnapshot } from "./types";

export function toBlockSnapshot(block: BlockSnapshot): BlockSnapshot {
  return {
    id: block.id,
    title: block.title,
    bodyMarkdown: block.bodyMarkdown,
    researchNotesMarkdown: block.researchNotesMarkdown,
    kind: block.kind,
    status: block.status,
    tags: [...block.tags],
  };
}

export const blockFingerprint = (block: BlockSnapshot) => JSON.stringify(toBlockSnapshot(block));
