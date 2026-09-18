import type { CitationRecord } from "./types";

export type CitationDraft = Omit<CitationRecord, "id">;

export function normalizeDoi(value: string) {
  return value
    .trim()
    .replace(/^doi:\s*/i, "")
    .replace(/^https?:\/\/(?:dx\.)?doi\.org\//i, "")
    .trim();
}

export async function lookupDoi(value: string): Promise<CitationDraft> {
  const doi = normalizeDoi(value);
  if (!doi || !doi.includes("/")) throw new Error("DOIを確認して");
  const response = await fetch(`https://api.crossref.org/works/${encodeURIComponent(doi)}`, {
    headers: { Accept: "application/json" },
  });
  if (!response.ok) {
    throw new Error(response.status === 404 ? "DOIが見つからない" : `DOIの取得に失敗した (${response.status})`);
  }
  const payload = await response.json() as { message?: Record<string, unknown> };
  const message = payload.message;
  if (!message) throw new Error("DOIの応答を読み取れない");
  const titles = asStringArray(message.title);
  const title = titles[0]?.trim();
  if (!title) throw new Error("文献名が見つからない");
  const authors = Array.isArray(message.author)
    ? message.author.map((item) => authorName(item)).filter(Boolean).join("; ")
    : "";
  const year = crossrefYear(message);
  const canonicalDoi = typeof message.DOI === "string" ? normalizeDoi(message.DOI) : doi;
  const url = typeof message.URL === "string" ? message.URL : `https://doi.org/${canonicalDoi}`;
  return {
    citationKey: citationKey(authors, year, title),
    title,
    authors,
    year,
    doi: canonicalDoi,
    url,
    rawCslJson: message,
  };
}

export function parseBibTeX(source: string): CitationDraft[] {
  const entries: CitationDraft[] = [];
  let cursor = 0;
  while (cursor < source.length) {
    const start = source.indexOf("@", cursor);
    if (start < 0) break;
    cursor = start + 1;
    const typeStart = cursor;
    while (cursor < source.length && /[\w-]/.test(source[cursor])) cursor += 1;
    const entryType = source.slice(typeStart, cursor).toLowerCase();
    while (/\s/.test(source[cursor] ?? "")) cursor += 1;
    const opener = source[cursor];
    if ((opener !== "{" && opener !== "(") || ["comment", "preamble", "string"].includes(entryType)) {
      cursor += 1;
      continue;
    }
    const closer = opener === "{" ? "}" : ")";
    cursor += 1;
    const keyStart = cursor;
    while (cursor < source.length && source[cursor] !== "," && source[cursor] !== closer) cursor += 1;
    const key = source.slice(keyStart, cursor).trim();
    if (source[cursor] !== ",") { cursor += 1; continue; }
    cursor += 1;
    const fields: Record<string, string> = {};
    while (cursor < source.length) {
      skipSpaceAndCommas();
      if (source[cursor] === closer) { cursor += 1; break; }
      const nameStart = cursor;
      while (cursor < source.length && /[\w-]/.test(source[cursor])) cursor += 1;
      const name = source.slice(nameStart, cursor).trim().toLowerCase();
      while (/\s/.test(source[cursor] ?? "")) cursor += 1;
      if (!name || source[cursor] !== "=") { cursor += 1; continue; }
      cursor += 1;
      while (/\s/.test(source[cursor] ?? "")) cursor += 1;
      fields[name] = readValue();
    }
    const title = cleanBibValue(fields.title ?? "");
    if (!key || !title) continue;
    const authors = cleanBibValue(fields.author ?? "").replace(/\s+and\s+/gi, "; ");
    const parsedYear = Number.parseInt(cleanBibValue(fields.year ?? ""), 10);
    const doi = normalizeDoi(cleanBibValue(fields.doi ?? ""));
    const url = cleanBibValue(fields.url ?? "");
    entries.push({
      citationKey: key,
      title,
      authors,
      year: Number.isFinite(parsedYear) ? parsedYear : null,
      doi: doi || null,
      url: url || (doi ? `https://doi.org/${doi}` : null),
      rawCslJson: { source: "bibtex", entryType, fields },
    });

    function skipSpaceAndCommas() {
      while (cursor < source.length && /[\s,]/.test(source[cursor])) cursor += 1;
    }

    function readValue() {
      const opening = source[cursor];
      if (opening === "{") {
        cursor += 1;
        const valueStart = cursor;
        let depth = 1;
        while (cursor < source.length && depth > 0) {
          if (source[cursor] === "\\") { cursor += 2; continue; }
          if (source[cursor] === "{") depth += 1;
          if (source[cursor] === "}") depth -= 1;
          cursor += 1;
        }
        return source.slice(valueStart, Math.max(valueStart, cursor - 1)).trim();
      }
      if (opening === '"') {
        cursor += 1;
        let value = "";
        while (cursor < source.length) {
          if (source[cursor] === "\\" && cursor + 1 < source.length) {
            value += source.slice(cursor, cursor + 2);
            cursor += 2;
          } else if (source[cursor] === '"') {
            cursor += 1;
            break;
          } else {
            value += source[cursor];
            cursor += 1;
          }
        }
        return value.trim();
      }
      const valueStart = cursor;
      while (cursor < source.length && source[cursor] !== "," && source[cursor] !== closer) cursor += 1;
      return source.slice(valueStart, cursor).trim();
    }
  }
  return entries;
}

function asStringArray(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : [];
}

function authorName(value: unknown) {
  if (!value || typeof value !== "object") return "";
  const author = value as Record<string, unknown>;
  const given = typeof author.given === "string" ? author.given.trim() : "";
  const family = typeof author.family === "string" ? author.family.trim() : "";
  return [given, family].filter(Boolean).join(" ");
}

function crossrefYear(message: Record<string, unknown>) {
  for (const key of ["published-print", "published-online", "published", "issued"]) {
    const value = message[key];
    if (!value || typeof value !== "object") continue;
    const dateParts = (value as Record<string, unknown>)["date-parts"];
    if (Array.isArray(dateParts) && Array.isArray(dateParts[0]) && typeof dateParts[0][0] === "number") {
      return dateParts[0][0];
    }
  }
  return null;
}

function citationKey(authors: string, year: number | null, title: string) {
  const family = authors.split(/[;,]/)[0]?.trim().split(/\s+/).at(-1) ?? "source";
  const word = title.match(/[\p{L}\p{N}]+/u)?.[0] ?? "work";
  const compact = `${family}${year ?? ""}${word}`
    .normalize("NFKD")
    .replace(/[^\p{L}\p{N}_:-]/gu, "")
    .slice(0, 64);
  return compact || `source${year ?? ""}`;
}

function cleanBibValue(value: string) {
  return value
    .replace(/[{}]/g, "")
    .replace(/\\([{}%&_#])/g, "$1")
    .replace(/\s+/g, " ")
    .trim();
}
