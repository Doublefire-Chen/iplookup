/**
 * The Record (see CONTEXT.md): the answer to a Lookup, in ipinfo.io's JSON
 * shape (ADR 0001). Missing values are left out, so every item but `ip` and
 * `readme` is optional.
 */
export interface IpRecord {
  ip: string;
  hostname?: string;
  city?: string;
  region?: string;
  country?: string;
  /** `"lat,lon"` */
  loc?: string;
  /** `"AS<n> <name>"` */
  org?: string;
  postal?: string;
  timezone?: string;
  readme: string;
  country_name?: string;
  continent?: string;
  /** The Accuracy Radius, in kilometres. */
  accuracy_radius?: number;
  asn?: { asn?: string; name?: string; domain?: string };
}

export interface Coordinates {
  lat: number;
  lon: number;
}

/** Reads a Record's `loc` (`"lat,lon"`); `undefined` if absent or malformed. */
export function parseLoc(loc: string | undefined): Coordinates | undefined {
  const parts = loc?.split(",");
  if (parts?.length !== 2 || parts.some((p) => p.trim() === "")) return undefined;
  const [lat, lon] = parts.map(Number);
  if (!Number.isFinite(lat) || !Number.isFinite(lon)) return undefined;
  return { lat, lon };
}
