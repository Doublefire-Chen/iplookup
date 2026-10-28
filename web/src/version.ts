// The /version answer (server/src/version.rs) and what its page shows.

/** One loaded Data Source. Times are RFC 3339 UTC; `checked` is left out until the updater has checked. */
export interface SourceState {
  source: string;
  build: string;
  checked?: string;
}

/** `GET /version`. A Data Source that isn't loaded is left out of `data` (and the status is 503). */
export interface VersionInfo {
  version: string;
  data: {
    location?: SourceState;
    network?: SourceState;
  };
}

export type SourceKey = "location" | "network";

/** Each Data Source in display order, with the credit its licence asks for. */
export const DATA_SOURCES: {
  key: SourceKey;
  title: string;
  product: string;
  vendor: string;
  url: string;
}[] = [
  {
    key: "location",
    title: "Location Data",
    product: "GeoLite2 City",
    vendor: "MaxMind",
    url: "https://www.maxmind.com",
  },
  {
    key: "network",
    title: "Network Data",
    product: "IPinfo Lite",
    vendor: "IPinfo",
    url: "https://ipinfo.io",
  },
];

/** Whether the body is a /version answer (it is for both 200 and 503). */
export function isVersionInfo(body: unknown): body is VersionInfo {
  if (body === null || typeof body !== "object") return false;
  const v = body as Partial<VersionInfo>;
  return typeof v.version === "string" && v.data !== null && typeof v.data === "object";
}

/** Healthy when both Data Sources are loaded (the server's 200). */
export function isHealthy(info: VersionInfo): boolean {
  return info.data.location !== undefined && info.data.network !== undefined;
}

/** "2026-10-28 04:00 UTC (3 hours ago)", or the text as given if it isn't a time. */
export function formatTime(iso: string, now: Date = new Date()): string {
  const at = new Date(iso);
  if (Number.isNaN(at.getTime())) return iso;
  const utc = `${at.toISOString().slice(0, 16).replace("T", " ")} UTC`;
  return `${utc} (${formatAge(now.getTime() - at.getTime())})`;
}

/** "just now", "5 minutes ago", "3 hours ago", "2 days ago" (or "in …" for the future). */
export function formatAge(ms: number): string {
  const future = ms < 0;
  const minutes = Math.floor(Math.abs(ms) / 60_000);
  if (minutes < 1) return "just now";
  const [n, unit] =
    minutes < 60
      ? [minutes, "minute"]
      : minutes < 48 * 60
        ? [Math.floor(minutes / 60), "hour"]
        : [Math.floor(minutes / (24 * 60)), "day"];
  const amount = `${n} ${unit}${n === 1 ? "" : "s"}`;
  return future ? `in ${amount}` : `${amount} ago`;
}
