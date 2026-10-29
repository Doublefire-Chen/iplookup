import { useEffect, useMemo, useState } from "react";
import { AccuracyMap } from "../components/AccuracyMap";
import { RecordCard } from "../components/RecordCard";
import { SearchBox } from "../components/SearchBox";
import { SelfCards } from "../components/SelfCards";
import { configuredHosts, injectedFor, mainHost } from "../injected";
import { mapCircles } from "../map";
import type { IpRecord } from "../record";
import { planCards } from "../selfCards";

/**
 * The Self Lookup: with `hosts.v4` and `hosts.v6` configured, an IPv4 and
 * an IPv6 card; otherwise the one Record the server injected.
 */
export function HomePage() {
  const injected = useMemo(() => injectedFor("/").record, []);
  const hosts = configuredHosts();
  const plans = useMemo(() => planCards(injected, hosts), [injected, hosts]);

  return (
    <>
      <SearchBox />
      {plans ? <SelfCards plans={plans} injected={injected} /> : <SingleCard injected={injected} />}
      <p className="hint muted">
        From a terminal: <code>curl {mainHost()}</code>
        {plans && hosts?.v4 && hosts.v6 && (
          <>
            , or force a protocol with <code>curl {hosts.v4}</code> /{" "}
            <code>curl {hosts.v6}</code>
          </>
        )}
      </p>
    </>
  );
}

/** The Record of whichever protocol the page was loaded over. */
function SingleCard({ injected }: { injected?: IpRecord }) {
  const [record, setRecord] = useState<IpRecord | undefined>(injected);
  const [failed, setFailed] = useState(false);

  // Without an injected Record (`pnpm dev`, or after a client-side move from
  // another page): ask the server instead.
  useEffect(() => {
    if (record) return;
    fetch("/json")
      .then((res) => (res.ok ? (res.json() as Promise<IpRecord>) : Promise.reject()))
      .then(setRecord, () => setFailed(true));
  }, [record]);

  const circles = useMemo(() => mapCircles([record]), [record]);
  return record ? (
    <div className="record-layout">
      <RecordCard title="Your IP address" record={record} />
      <AccuracyMap circles={circles} />
    </div>
  ) : (
    <section className="card">
      <p className="muted">{failed ? "Your Record couldn't be loaded." : "Loading…"}</p>
    </section>
  );
}
