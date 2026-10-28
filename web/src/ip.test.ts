import { describe, expect, it } from "vitest";
import { isIPv4, isIPv6, parseIpInput } from "./ip";

describe("parseIpInput", () => {
  it("accepts IPv4 and IPv6 addresses, trimmed", () => {
    for (const ip of [
      "8.8.8.8",
      "0.0.0.0",
      "255.255.255.255",
      "10.0.0.1",
      "2001:4860:4860::8888",
      "2001:4860:4860:0:0:0:0:8888",
      "::",
      "::1",
      "fe80::1",
      "1::",
      "1:2:3:4:5:6:7::",
      "::2:3:4:5:6:7:8",
      "2001:DB8::ABCD",
      "::ffff:8.8.8.8",
      "64:ff9b::1.1.1.1",
      "1:2:3:4:5:6:1.2.3.4",
    ]) {
      expect(parseIpInput(ip), ip).toBe(ip);
    }
    expect(parseIpInput("  1.1.1.1\n")).toBe("1.1.1.1");
  });

  it("rejects what the server calls Wrong ip", () => {
    for (const text of [
      "",
      "   ",
      "8.8.8.08",
      "999.1.1.1",
      "256.0.0.0",
      "1.2.3",
      "1.2.3.4.5",
      "1.2.3.",
      "+1.2.3.4",
      "google.com",
      "8.8.8.8/32",
      "http://8.8.8.8",
      "1 .2.3.4",
      "1:2:3:4:5:6:7:8:9",
      "1:2:3:4:5:6:7",
      "1:2:3:4:5:6:7:8::",
      "1::2::3",
      ":::",
      ":1::",
      "12345::",
      "fe80::1%eth0",
      "[::1]",
      "::ffff:8.8.8.08",
      "1.2.3.4::",
      "1:2:3:4:5:6:7:1.2.3.4",
      "g::1",
    ]) {
      expect(parseIpInput(text), text).toBeUndefined();
    }
  });

  it("tells the two families apart", () => {
    expect(isIPv4("8.8.8.8")).toBe(true);
    expect(isIPv6("8.8.8.8")).toBe(false);
    expect(isIPv4("::1")).toBe(false);
    expect(isIPv6("::1")).toBe(true);
  });
});
