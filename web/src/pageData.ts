import type { IpRecord } from "./record";

/**
 * What the server embeds in index.html for the first paint (server/src/page.rs):
 * `<script id="iplookup-data" type="application/json">`. Every item is optional.
 */
export interface PageData {
  /** The Record the page shows: the visitor's own on the home page, the looked-up one on a Record page. */
  record?: IpRecord;
  /** Why the page has no Record: an unknown path (404) or the rate limit (429). */
  error?: PageError;
  /** `hosts.*` from the server's config. */
  hosts?: { main?: string };
}

/** The page's HTTP status, with the title and message the JSON error would carry. */
export interface PageError {
  status: number;
  title: string;
  message: string;
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

/**
 * What was injected for `path`: the loaded data when `path` is the one the
 * server answered (`loadedPath`), else nothing. After a client-side route
 * change the page must fetch its own Record instead of showing another
 * path's.
 */
export function pageDataFor(data: PageData, loadedPath: string, path: string): PageData {
  return path === loadedPath ? data : {};
}
