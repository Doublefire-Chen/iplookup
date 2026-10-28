import { describe, expect, it } from "vitest";
import { formatAge, formatTime, isHealthy, isVersionInfo, type VersionInfo } from "./version";

const state = { source: "MaxMind GeoLite2 City", build: "2026-10-28T00:00:00Z" };

describe("isVersionInfo", () => {
  it("accepts the /version shape, healthy or not", () => {
    expect(isVersionInfo({ version: "0.1.0", data: { location: state, network: state } })).toBe(
      true,
    );
    expect(isVersionInfo({ version: "0.1.0", data: {} })).toBe(true);
  });

  it("rejects anything else", () => {
    expect(isVersionInfo(null)).toBe(false);
    expect(isVersionInfo({ status: 404, error: {} })).toBe(false);
    expect(isVersionInfo({ version: "0.1.0", data: null })).toBe(false);
  });
});

describe("isHealthy", () => {
  it("needs both Data Sources", () => {
    const info = (data: VersionInfo["data"]): VersionInfo => ({ version: "0.1.0", data });
    expect(isHealthy(info({ location: state, network: state }))).toBe(true);
    expect(isHealthy(info({ location: state }))).toBe(false);
    expect(isHealthy(info({ network: state }))).toBe(false);
    expect(isHealthy(info({}))).toBe(false);
  });
});

describe("formatTime", () => {
  const now = new Date("2026-10-28T07:30:00Z");

  it("shows UTC to the minute plus the age", () => {
    expect(formatTime("2026-10-28T04:00:00Z", now)).toBe("2026-10-28 04:00 UTC (3 hours ago)");
    expect(formatTime("2026-10-20T00:00:00Z", now)).toBe("2026-10-20 00:00 UTC (8 days ago)");
  });

  it("leaves text that isn't a time alone", () => {
    expect(formatTime("soon", now)).toBe("soon");
  });
});

describe("formatAge", () => {
  it("picks a unit", () => {
    expect(formatAge(20_000)).toBe("just now");
    expect(formatAge(60_000)).toBe("1 minute ago");
    expect(formatAge(59 * 60_000)).toBe("59 minutes ago");
    expect(formatAge(47 * 3_600_000)).toBe("47 hours ago");
    expect(formatAge(48 * 3_600_000)).toBe("2 days ago");
    expect(formatAge(-2 * 60_000)).toBe("in 2 minutes");
  });
});
