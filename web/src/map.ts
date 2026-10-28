import { parseLoc, type Coordinates, type IpRecord } from "./record";

/** What a map draws for one Record: a circle, never a pin (CONTEXT.md, Accuracy Radius). */
export interface MapCircle {
  center: Coordinates;
  /** The Accuracy Radius in metres, as Leaflet wants it. */
  radiusMeters: number;
}

/**
 * The circle to draw for a Record, or `undefined` for "no map": Bogons,
 * Records without (valid) `loc`, and Records without an Accuracy Radius.
 * Without a radius we can't say how rough the coordinates are, and any
 * circle we picked would claim a precision the data doesn't have.
 */
export function mapCircle(record: IpRecord | undefined): MapCircle | undefined {
  if (!record || record.bogon) return undefined;
  const center = parseLoc(record.loc);
  const km = record.accuracy_radius;
  if (!center || km === undefined || !Number.isFinite(km) || km <= 0) return undefined;
  return { center, radiusMeters: km * 1000 };
}

/**
 * The circles for several Records (the IPv4 and IPv6 Self Lookup cards), in
 * order, leaving out Records without one and drawing identical circles once.
 * Empty means no map.
 */
export function mapCircles(records: readonly (IpRecord | undefined)[]): MapCircle[] {
  const circles: MapCircle[] = [];
  for (const record of records) {
    const c = mapCircle(record);
    if (!c) continue;
    const same = circles.some(
      (d) =>
        d.center.lat === c.center.lat &&
        d.center.lon === c.center.lon &&
        d.radiusMeters === c.radiusMeters,
    );
    if (!same) circles.push(c);
  }
  return circles;
}
