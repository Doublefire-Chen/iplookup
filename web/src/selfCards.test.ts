import { afterEach, describe, expect, it, vi } from "vitest";
import { fetchSelfRecord, planCards, settleCard } from "./selfCards";

const hosts = { main: "ip.example.com", v4: "v4.ip.example.com", v6: "v6.ip.example.com" };
const v4 = { ip: "8.8.8.8", city: "Mountain View" };
const v6 = { ip: "2001:4860:4860::8888", city: "Mountain View" };
const lan = { ip: "192.168.1.2", bogon: true as const };

describe("planCards", () => {
  it("is not configured without both hosts (one card, no fetches)", () => {
    expect(planCards(v4, undefined)).toBeUndefined();
    expect(planCards(v4, { main: "ip.example.com" })).toBeUndefined();
    expect(planCards(v4, { v4: "v4.ip.example.com" })).toBeUndefined();
    expect(planCards(v6, { v6: "v6.ip.example.com" })).toBeUndefined();
  });

  it("fills the injected Record's card and fetches only the other", () => {
    expect(planCards(v4, hosts)).toEqual({
      IPv4: { state: { kind: "loaded", record: v4 } },
      IPv6: { state: { kind: "loading" }, fetchUrl: "https://v6.ip.example.com/json" },
    });
    expect(planCards(v6, hosts)).toEqual({
      IPv4: { state: { kind: "loading" }, fetchUrl: "https://v4.ip.example.com/json" },
      IPv6: { state: { kind: "loaded", record: v6 } },
    });
  });

  it("fetches both without an injected Record, or with a Bogon", () => {
    for (const record of [undefined, lan, { ip: "::1", bogon: true as const }]) {
      expect(planCards(record, hosts)).toEqual({
        IPv4: { state: { kind: "loading" }, fetchUrl: "https://v4.ip.example.com/json" },
        IPv6: { state: { kind: "loading" }, fetchUrl: "https://v6.ip.example.com/json" },
      });
    }
  });
});

describe("settleCard", () => {
  it("shows a fetched Record of its protocol", () => {
    expect(settleCard("IPv6", v6, v4)).toEqual({ kind: "loaded", record: v6 });
    expect(settleCard("IPv4", v4, lan)).toEqual({ kind: "loaded", record: v4 });
  });

  it("means no connectivity when the fetch failed", () => {
    expect(settleCard("IPv6", undefined, v4)).toEqual({ kind: "no-connectivity" });
    expect(settleCard("IPv4", undefined, undefined)).toEqual({ kind: "no-connectivity" });
  });

  it("ignores a Record of the other protocol", () => {
    expect(settleCard("IPv6", v4, v4)).toEqual({ kind: "no-connectivity" });
  });

  it("falls back to the injected Bogon on the protocol the page came over", () => {
    expect(settleCard("IPv4", undefined, lan)).toEqual({ kind: "loaded", record: lan });
    expect(settleCard("IPv6", undefined, lan)).toEqual({ kind: "no-connectivity" });
  });
});

describe("fetchSelfRecord", () => {
  afterEach(() => vi.unstubAllGlobals());

  const respond = (body: unknown, status = 200) =>
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response(JSON.stringify(body), { status })),
    );

  it("returns the Record", async () => {
    respond(v6);
    expect(await fetchSelfRecord("https://v6.ip.example.com/json")).toEqual(v6);
  });

  it("is undefined on an error status, bad body or network error", async () => {
    respond({ status: 429 }, 429);
    expect(await fetchSelfRecord("u")).toBeUndefined();
    respond(["not a record"]);
    expect(await fetchSelfRecord("u")).toBeUndefined();
    vi.stubGlobal("fetch", vi.fn(async () => Promise.reject(new TypeError("Failed to fetch"))));
    expect(await fetchSelfRecord("u")).toBeUndefined();
  });

  it("gives up after the timeout", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(
        (_url: string, init: RequestInit) =>
          new Promise((_resolve, reject) =>
            init.signal?.addEventListener("abort", () => reject(init.signal?.reason)),
          ),
      ),
    );
    expect(await fetchSelfRecord("u", 20)).toBeUndefined();
  });
});
