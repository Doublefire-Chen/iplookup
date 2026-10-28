// The page data of the document as loaded (see pageData.ts).

import { pageDataFor, readPageData, type PageData } from "./pageData";

// Read once: they describe the request that loaded this document.
const loaded = readPageData();
const loadedPath = window.location.pathname;

/** The injected data, if the server injected it for `path`. */
export function injectedFor(path: string): PageData {
  return pageDataFor(loaded, loadedPath, path);
}

/** The host for commands: `hosts.main`, else the one this page came from. */
export function mainHost(): string {
  return loaded.hosts?.main ?? window.location.host;
}
