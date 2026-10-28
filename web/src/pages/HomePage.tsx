import { useEffect, useState } from "react";
import { RecordCard } from "../components/RecordCard";
import { readPageData } from "../pageData";
import type { IpRecord } from "../record";

// Read once: it describes the request that loaded this page.
const injected = readPageData().record;

/** The Self Lookup, rendered straight from the Record the server injected. */
export function HomePage() {
  const [record, setRecord] = useState<IpRecord | undefined>(injected);
  const [failed, setFailed] = useState(false);

  // Only without an injected Record (`pnpm dev`): ask the server instead.
  useEffect(() => {
    if (record) return;
    fetch("/json")
      .then((res) => (res.ok ? (res.json() as Promise<IpRecord>) : Promise.reject()))
      .then(setRecord, () => setFailed(true));
  }, [record]);

  return (
    <>
      {record ? (
        <RecordCard title="Your IP address" record={record} />
      ) : (
        <section className="card">
          <p className="muted">{failed ? "Your Record couldn't be loaded." : "Loading…"}</p>
        </section>
      )}
      <p className="hint muted">
        From a terminal: <code>curl {window.location.host}</code>
      </p>
    </>
  );
}
