import type { IpRecord } from "./record";

/**
 * What the server embeds in index.html for the first paint (server/src/page.rs):
 * `<script id="iplookup-data" type="application/json">`. Every item is optional.
 */
export interface PageData {
  /** The Record the page shows: the visitor's own on the home page. */
  record?: IpRecord;
}

/** The embedded page data, or `{}` when there is none (as under `pnpm dev`). */
export function readPageData(doc: Document = document): PageData {
  const text = doc.getElementById("iplookup-data")?.textContent;
  if (!text) return {};
  try {
    const data: unknown = JSON.parse(text);
    return data !== null && typeof data === "object" ? (data as PageData) : {};
  } catch {
    return {};
  }
}
