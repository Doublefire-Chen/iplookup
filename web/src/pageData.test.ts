import { describe, expect, it } from "vitest";
import { pageDataFor, readPageData } from "./pageData";

/** A stand-in for `document` holding one element's text. */
function doc(text: string | undefined): Document {
  return {
    getElementById: (id: string) =>
      id === "iplookup-data" && text !== undefined ? { textContent: text } : null,
  } as unknown as Document;
}

describe("readPageData", () => {
  it("reads the embedded block, escapes and all", () => {
    expect(
      readPageData(doc('{"record":{"ip":"8.8.8.8","hostname":"\\u003c/script\\u003e"}}')),
    ).toEqual({ record: { ip: "8.8.8.8", hostname: "</script>" } });
  });

  it("is empty without a usable block", () => {
    expect(readPageData(doc(undefined))).toEqual({});
    expect(readPageData(doc("not json"))).toEqual({});
    expect(readPageData(doc("null"))).toEqual({});
  });
});

describe("pageDataFor", () => {
  const data = { record: { ip: "8.8.8.8" }, hosts: { main: "ip.example.com" } };

  it("is the injected data on the path it was injected for", () => {
    expect(pageDataFor(data, "/8.8.8.8", "/8.8.8.8")).toBe(data);
  });

  it("is nothing after a client-side move to another path", () => {
    expect(pageDataFor(data, "/8.8.8.8", "/")).toEqual({});
    expect(pageDataFor(data, "/8.8.8.8", "/1.1.1.1")).toEqual({});
  });
});
