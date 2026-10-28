import { describe, expect, it } from "vitest";
import { mapCircle, mapCircles } from "./map";
import type { IpRecord } from "./record";

const google: IpRecord = {
  ip: "8.8.8.8",
  loc: "37.4056,-122.0775",
  accuracy_radius: 1000,
};

describe("mapCircle", () => {
  it("centres on loc with the Accuracy Radius in metres", () => {
    expect(mapCircle(google)).toEqual({
      center: { lat: 37.4056, lon: -122.0775 },
      radiusMeters: 1_000_000,
    });
  });

  it("keeps small radii exact", () => {
    expect(mapCircle({ ip: "1.1.1.1", loc: "-33.8688,151.2093", accuracy_radius: 1 })).toEqual({
      center: { lat: -33.8688, lon: 151.2093 },
      radiusMeters: 1000,
    });
  });

  it("gives no map for a Bogon", () => {
    expect(mapCircle({ ip: "10.0.0.1", bogon: true })).toBeUndefined();
  });

  it("gives no map without a Record", () => {
    expect(mapCircle(undefined)).toBeUndefined();
  });

  it("gives no map without loc, or with a malformed one", () => {
    expect(mapCircle({ ip: "8.8.8.8", accuracy_radius: 1000 })).toBeUndefined();
    expect(mapCircle({ ...google, loc: "37.4,abc" })).toBeUndefined();
    expect(mapCircle({ ...google, loc: "" })).toBeUndefined();
  });

  it("gives no map without a usable Accuracy Radius", () => {
    expect(mapCircle({ ip: "8.8.8.8", loc: google.loc })).toBeUndefined();
    expect(mapCircle({ ...google, accuracy_radius: 0 })).toBeUndefined();
    expect(mapCircle({ ...google, accuracy_radius: -5 })).toBeUndefined();
    expect(mapCircle({ ...google, accuracy_radius: Number.NaN })).toBeUndefined();
  });
});

describe("mapCircles", () => {
  const sydney: IpRecord = { ip: "2606:4700::1111", loc: "-33.8688,151.2093", accuracy_radius: 20 };

  it("draws one circle per Record that has one, in order", () => {
    expect(mapCircles([google, sydney]).map((c) => c.radiusMeters)).toEqual([1_000_000, 20_000]);
  });

  it("draws identical circles once", () => {
    expect(mapCircles([google, { ...google, ip: "2001:4860:4860::8888" }])).toHaveLength(1);
  });

  it("is empty when no Record has a circle", () => {
    expect(mapCircles([undefined, { ip: "10.0.0.1", bogon: true }, { ip: "8.8.8.8" }])).toEqual([]);
  });
});
