// Checking what a visitor types into the search box. The rules match the
// server's (Rust's standard address parser, server/src/lookup.rs), so the
// box never opens a page the server would answer with "Wrong ip".

/** `a.b.c.d` with decimal octets 0-255 and no leading zeros. */
export function isIPv4(text: string): boolean {
  const octets = text.split(".");
  return (
    octets.length === 4 && octets.every((o) => /^(0|[1-9][0-9]{0,2})$/.test(o) && Number(o) <= 255)
  );
}

const HEX_GROUP = /^[0-9a-fA-F]{1,4}$/;

/**
 * Eight groups of 1-4 hex digits, where one `::` may stand for one or more
 * zero groups and the last two groups may be written as an IPv4 address.
 * No zone IDs (`%eth0`) or brackets.
 */
export function isIPv6(text: string): boolean {
  const lastColon = text.lastIndexOf(":");
  if (lastColon === -1) return false;
  let groups = text;
  const tail = text.slice(lastColon + 1);
  if (tail.includes(".")) {
    if (!isIPv4(tail)) return false;
    groups = `${text.slice(0, lastColon + 1)}0:0`;
  }
  const halves = groups.split("::");
  const split = (s: string) => (s === "" ? [] : s.split(":"));
  if (halves.length === 1) {
    const all = split(groups);
    return all.length === 8 && all.every((g) => HEX_GROUP.test(g));
  }
  if (halves.length !== 2) return false;
  const all = [...split(halves[0]), ...split(halves[1])];
  return all.length <= 7 && all.every((g) => HEX_GROUP.test(g));
}

/**
 * The address to look up from what was typed (surrounding spaces dropped),
 * or `undefined` when it isn't an IP address.
 */
export function parseIpInput(input: string): string | undefined {
  const text = input.trim();
  return isIPv4(text) || isIPv6(text) ? text : undefined;
}
