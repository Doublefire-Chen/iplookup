import { useEffect, useState } from "react";
import type { IpRecord } from "../record";
import {
  fetchSelfRecord,
  PROTOCOLS,
  settleCard,
  type CardPlans,
  type CardState,
  type Protocol,
} from "../selfCards";
import { RecordCard } from "./RecordCard";

/**
 * The visitor's IPv4 and IPv6 Self Lookups side by side. Cards with a
 * `fetchUrl` start loading and are fetched in parallel; the rest are
 * already filled.
 */
export function SelfCards({ plans, injected }: { plans: CardPlans; injected?: IpRecord }) {
  const [states, setStates] = useState<Record<Protocol, CardState>>(() => ({
    IPv4: plans.IPv4.state,
    IPv6: plans.IPv6.state,
  }));

  useEffect(() => {
    let live = true;
    for (const protocol of PROTOCOLS) {
      const url = plans[protocol].fetchUrl;
      if (!url) continue;
      void fetchSelfRecord(url).then((fetched) => {
        if (live) {
          setStates((s) => ({ ...s, [protocol]: settleCard(protocol, fetched, injected) }));
        }
      });
    }
    return () => {
      live = false;
    };
  }, [plans, injected]);

  return (
    <div className="self-cards">
      {PROTOCOLS.map((protocol) => (
        <SelfCard key={protocol} protocol={protocol} state={states[protocol]} />
      ))}
    </div>
  );
}

function SelfCard({ protocol, state }: { protocol: Protocol; state: CardState }) {
  const title = `Your ${protocol} address`;
  if (state.kind === "loaded") return <RecordCard title={title} record={state.record} />;
  return (
    <section className="card record-card" aria-label={title} aria-busy={state.kind === "loading"}>
      <div className="record-head">
        <h2>{title}</h2>
        <span className="badge">{protocol}</span>
      </div>
      {state.kind === "loading" ? (
        <p className="muted">Loading…</p>
      ) : (
        <>
          <p className="record-none">No {protocol} connectivity</p>
          <p className="muted">Your device couldn't reach this site over {protocol}.</p>
        </>
      )}
    </section>
  );
}
