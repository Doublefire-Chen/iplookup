// Turning a Record into what the cards show.

import { parseLoc, type IpRecord } from "./record";

/** "IPv4" or "IPv6", from the address's text. */
export function ipVersion(ip: string): "IPv4" | "IPv6" {
  return ip.includes(":") ? "IPv6" : "IPv4";
}

/** "Mountain View, California, United States": the known parts, without repeats. */
export function formatLocation(record: IpRecord): string | undefined {
  const parts = [record.city, record.region, record.country_name ?? record.country];
  const seen = new Set<string>();
  const unique = parts.filter((p): p is string => {
    if (!p || seen.has(p)) return false;
    seen.add(p);
    return true;
  });
  return unique.length > 0 ? unique.join(", ") : undefined;
}

/** "AS15169 Google LLC": `org`, or else whatever the `asn` object has. */
export function formatNetwork(record: IpRecord): string | undefined {
  if (record.org) return record.org;
  const parts = [record.asn?.asn, record.asn?.name].filter(Boolean);
  return parts.length > 0 ? parts.join(" ") : undefined;
}

/** "1,000 km" */
export function formatKm(km: number): string {
  return `${km.toLocaleString("en-US")} km`;
}

/** "37.4056, -122.0775 (within 1,000 km)": coordinates plus the Accuracy Radius. */
export function formatCoordinates(record: IpRecord): string | undefined {
  const c = parseLoc(record.loc);
  if (!c) return undefined;
  const at = `${c.lat.toFixed(4)}, ${c.lon.toFixed(4)}`;
  return record.accuracy_radius === undefined
    ? at
    : `${at} (within ${formatKm(record.accuracy_radius)})`;
}

export interface Row {
  label: string;
  value: string;
}

/** The rows of a Record's card, in display order, leaving out what's unknown. */
export function recordRows(record: IpRecord): Row[] {
  const rows: [string, string | undefined][] = [
    ["Hostname", record.hostname],
    ["Location", formatLocation(record)],
    ["Coordinates", formatCoordinates(record)],
    ["Postal code", record.postal],
    ["Timezone", record.timezone],
    ["Network", formatNetwork(record)],
    ["Domain", record.asn?.domain],
  ];
  return rows
    .filter((r): r is [string, string] => r[1] !== undefined && r[1] !== "")
    .map(([label, value]) => ({ label, value }));
}

/** "curl https://ip.example.com/8.8.8.8": the API call for an address's Record. */
export function curlCommand(host: string, ip: string): string {
  return `curl https://${host}/${ip}`;
}

/** A Record as the API prints it: two-space indented JSON. */
export function recordJson(record: IpRecord): string {
  return JSON.stringify(record, null, 2);
}
