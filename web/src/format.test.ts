import { describe, expect, it } from "vitest";
import {
  formatCoordinates,
  formatKm,
  formatLocation,
  formatNetwork,
  ipVersion,
  recordRows,
} from "./format";
import type { IpRecord } from "./record";

const google: IpRecord = {
  ip: "8.8.8.8",
  hostname: "dns.google",
  city: "Mountain View",
  region: "California",
  country: "US",
  loc: "37.4056,-122.0775",
  org: "AS15169 Google LLC",
  postal: "94043",
  timezone: "America/Los_Angeles",
  readme: "https://github.com/Doublefire-Chen/iplookup/blob/main/README.md",
  country_name: "United States",
  continent: "North America",
  accuracy_radius: 1000,
  asn: { asn: "AS15169", name: "Google LLC", domain: "google.com" },
};

describe("ipVersion", () => {
  it("tells IPv4 from IPv6", () => {
    expect(ipVersion("8.8.8.8")).toBe("IPv4");
    expect(ipVersion("2001:4860:4860::8888")).toBe("IPv6");
    expect(ipVersion("::ffff:8.8.8.8")).toBe("IPv6");
  });
});

describe("formatLocation", () => {
  it("joins city, region and country name", () => {
    expect(formatLocation(google)).toBe("Mountain View, California, United States");
  });

  it("falls back to the country code and skips repeats and gaps", () => {
    expect(formatLocation({ ip: "x", city: "Singapore", region: "Singapore", country: "SG" })).toBe(
      "Singapore, SG",
    );
    expect(formatLocation({ ip: "x", country_name: "Sweden" })).toBe("Sweden");
  });

  it("is undefined when nothing is known", () => {
    expect(formatLocation({ ip: "x" })).toBeUndefined();
  });
});

describe("formatNetwork", () => {
  it("prefers org", () => {
    expect(formatNetwork(google)).toBe("AS15169 Google LLC");
  });

  it("falls back to the asn object", () => {
    expect(formatNetwork({ ip: "x", asn: { asn: "AS13335", name: "Cloudflare, Inc." } })).toBe(
      "AS13335 Cloudflare, Inc.",
    );
    expect(formatNetwork({ ip: "x", asn: { name: "Example" } })).toBe("Example");
    expect(formatNetwork({ ip: "x", asn: {} })).toBeUndefined();
    expect(formatNetwork({ ip: "x" })).toBeUndefined();
  });
});

describe("formatCoordinates", () => {
  it("shows four decimals and the Accuracy Radius", () => {
    expect(formatCoordinates(google)).toBe("37.4056, -122.0775 (within 1,000 km)");
    expect(formatCoordinates({ ip: "x", loc: "59.33,18.06", accuracy_radius: 5 })).toBe(
      "59.3300, 18.0600 (within 5 km)",
    );
  });

  it("leaves out a missing radius, and is undefined without a loc", () => {
    expect(formatCoordinates({ ip: "x", loc: "1,2" })).toBe("1.0000, 2.0000");
    expect(formatCoordinates({ ip: "x", accuracy_radius: 5 })).toBeUndefined();
    expect(formatCoordinates({ ip: "x", loc: "nowhere" })).toBeUndefined();
  });
});

describe("formatKm", () => {
  it("groups thousands", () => {
    expect(formatKm(20000)).toBe("20,000 km");
  });
});

describe("recordRows", () => {
  it("lists every known item in order", () => {
    expect(recordRows(google)).toEqual([
      { label: "Hostname", value: "dns.google" },
      { label: "Location", value: "Mountain View, California, United States" },
      { label: "Coordinates", value: "37.4056, -122.0775 (within 1,000 km)" },
      { label: "Postal code", value: "94043" },
      { label: "Timezone", value: "America/Los_Angeles" },
      { label: "Network", value: "AS15169 Google LLC" },
      { label: "Domain", value: "google.com" },
    ]);
  });

  it("leaves out what the Record doesn't have", () => {
    expect(recordRows({ ip: "1.2.3.4", country: "US", org: "AS1 Example" })).toEqual([
      { label: "Location", value: "US" },
      { label: "Network", value: "AS1 Example" },
    ]);
    expect(recordRows({ ip: "10.0.0.1", bogon: true })).toEqual([]);
  });
});
