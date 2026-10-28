// The home page's IPv4 and IPv6 cards: which to fill from the injected
// Record, which to fetch from `hosts.v4` / `hosts.v6`, and what a fetch's
// outcome means. Pure, so Vitest covers it (selfCards.test.ts).

import { ipVersion } from "./format";
import type { PageData } from "./pageData";
import type { IpRecord } from "./record";

export type Protocol = "IPv4" | "IPv6";
export const PROTOCOLS: readonly Protocol[] = ["IPv4", "IPv6"];

/** How long a cross-host Self Lookup may take before the protocol counts as missing. */
export const FETCH_TIMEOUT_MS = 5000;

/** One protocol's card. */
export type CardState =
  | { kind: "loading" }
  | { kind: "loaded"; record: IpRecord }
  /** The protocol's fetch failed or timed out: the visitor can't reach it. */
  | { kind: "no-connectivity" };

/** A card to show at once, plus where to fetch its Record if it isn't known yet. */
export interface CardPlan {
  state: CardState;
  fetchUrl?: string;
}

export type CardPlans = Record<Protocol, CardPlan>;

/**
 * The two cards for the home page, or `undefined` when `hosts.v4` or
 * `hosts.v6` isn't configured (the page then shows its single Self Lookup
 * card and fetches nothing cross-host).
 *
 * The injected Record fills its own protocol's card, so only the other one
 * is fetched. A Bogon (say, the page opened over a LAN) says nothing about
 * the visitor's public address, so both protocols are fetched then.
 */
export function planCards(
  record: IpRecord | undefined,
  hosts: PageData["hosts"],
): CardPlans | undefined {
  const v4 = hosts?.v4;
  const v6 = hosts?.v6;
  if (!v4 || !v6) return undefined;
  const plan = (protocol: Protocol, host: string): CardPlan =>
    record && !record.bogon && ipVersion(record.ip) === protocol
      ? { state: { kind: "loaded", record } }
      : { state: { kind: "loading" }, fetchUrl: `https://${host}/json` };
  return { IPv4: plan("IPv4", v4), IPv6: plan("IPv6", v6) };
}

/**
 * A protocol's card once its fetch is over: `fetched` is the Record it
 * returned, or `undefined` if it failed or timed out.
 *
 * - A Record of the right protocol is shown.
 * - Anything else means no connectivity on that protocol, except when the
 *   page itself was loaded over it with a Bogon address: the visitor
 *   evidently has that protocol, so the Bogon Record is shown instead.
 *   (A Record of the wrong protocol means the host isn't single-protocol,
 *   and says nothing about this one.)
 */
export function settleCard(
  protocol: Protocol,
  fetched: IpRecord | undefined,
  injected: IpRecord | undefined,
): CardState {
  if (fetched && ipVersion(fetched.ip) === protocol) return { kind: "loaded", record: fetched };
  if (injected?.bogon && ipVersion(injected.ip) === protocol) {
    return { kind: "loaded", record: injected };
  }
  return { kind: "no-connectivity" };
}

/** `url`'s Record, or `undefined` on any failure or after `timeoutMs`. */
export async function fetchSelfRecord(
  url: string,
  timeoutMs: number = FETCH_TIMEOUT_MS,
): Promise<IpRecord | undefined> {
  try {
    const res = await fetch(url, { signal: AbortSignal.timeout(timeoutMs) });
    if (!res.ok) return undefined;
    const data: unknown = await res.json();
    return data !== null && typeof data === "object" && typeof (data as IpRecord).ip === "string"
      ? (data as IpRecord)
      : undefined;
  } catch {
    return undefined;
  }
}
