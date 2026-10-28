import { describe, expect, it } from "vitest";
import { readPageData } from "./pageData";

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
