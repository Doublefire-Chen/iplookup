import { useEffect, useState } from "react";
import { RecordCard } from "../components/RecordCard";
import { SearchBox } from "../components/SearchBox";
import { injectedFor, mainHost } from "../injected";
import type { IpRecord } from "../record";

/** The Self Lookup, rendered straight from the Record the server injected. */
export function HomePage() {
  const [record, setRecord] = useState<IpRecord | undefined>(() => injectedFor("/").record);
  const [failed, setFailed] = useState(false);

  // Without an injected Record (`pnpm dev`, or after a client-side move from
  // another page): ask the server instead.
  useEffect(() => {
    if (record) return;
    fetch("/json")
      .then((res) => (res.ok ? (res.json() as Promise<IpRecord>) : Promise.reject()))
      .then(setRecord, () => setFailed(true));
  }, [record]);

  return (
    <>
      <SearchBox />
      {record ? (
        <RecordCard title="Your IP address" record={record} />
      ) : (
        <section className="card">
          <p className="muted">{failed ? "Your Record couldn't be loaded." : "Loading…"}</p>
        </section>
      )}
      <p className="hint muted">
        From a terminal: <code>curl {mainHost()}</code>
      </p>
    </>
  );
}
