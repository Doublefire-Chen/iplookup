import { describe, expect, it } from "vitest";
import { parseLoc } from "./record";

describe("parseLoc", () => {
  it("reads ipinfo's lat,lon string", () => {
    expect(parseLoc("37.4056,-122.0775")).toEqual({ lat: 37.4056, lon: -122.0775 });
  });

  it("returns undefined for a missing or malformed loc", () => {
    expect(parseLoc(undefined)).toBeUndefined();
    expect(parseLoc("")).toBeUndefined();
    expect(parseLoc("37.4")).toBeUndefined();
    expect(parseLoc("37.4,")).toBeUndefined();
    expect(parseLoc("a,b")).toBeUndefined();
  });
});
