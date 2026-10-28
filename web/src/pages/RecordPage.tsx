import { useEffect, useState } from "react";
import { useLocation, useParams } from "react-router";
import { CopyButton } from "../components/CopyButton";
import { RecordCard } from "../components/RecordCard";
import { SearchBox } from "../components/SearchBox";
import { curlCommand, recordJson } from "../format";
import { injectedFor, mainHost } from "../injected";
import { isIPv4, isIPv6 } from "../ip";
import type { PageError } from "../pageData";
import type { IpRecord } from "../record";
import { NotFoundPage } from "./NotFoundPage";

type View = { record: IpRecord } | { error: PageError } | { loading: true } | { failed: true };

/** `/{ip}`: an Explicit Lookup's Record, as a table, raw JSON and a curl command. */
export function RecordPage() {
  const { pathname } = useLocation();
  // A new address is a new page: start again from its own data.
  return <RecordView key={pathname} path={pathname} />;
}

function RecordView({ path }: { path: string }) {
  const { ip = "" } = useParams();
  const [view, setView] = useState<View>(() => initialView(path, ip));

  // After a client-side move (the search box) nothing was injected for this
  // path: ask the API, which counts against the rate limit like the page.
  useEffect(() => {
    if (!("loading" in view)) return;
    let cancelled = false;
    void loadRecord(ip).then((next) => {
      if (!cancelled) setView(next);
    });
    return () => {
      cancelled = true;
    };
  }, [ip, view]);

  if ("error" in view && view.error.status === 404) return <NotFoundPage />;
  return (
    <>
      <title>{`${ip} · iplookup`}</title>
      <SearchBox />
      {"record" in view ? (
        <RecordDetails record={view.record} />
      ) : "error" in view ? (
        <section className="card message-card">
          <h1>{view.error.status === 429 ? "Too many lookups" : view.error.title}</h1>
          <p className="muted">{view.error.message}</p>
        </section>
      ) : (
        <section className="card">
          <p className="muted">
            {"failed" in view ? "This Record couldn't be loaded." : "Loading…"}
          </p>
        </section>
      )}
    </>
  );
}

function RecordDetails({ record }: { record: IpRecord }) {
  const command = curlCommand(mainHost(), record.ip);
  const json = recordJson(record);
  return (
    <>
      <RecordCard title="IP address" record={record} />
      <section className="card code-card" aria-label="From a terminal">
        <div className="code-head">
          <h2>From a terminal</h2>
          <CopyButton text={command} />
        </div>
        <pre className="code">
          <code>{command}</code>
        </pre>
      </section>
      <section className="card code-card" aria-label="Raw JSON">
        <div className="code-head">
          <h2>Raw JSON</h2>
          <CopyButton text={json} />
        </div>
        <pre className="code">
          <code>{json}</code>
        </pre>
      </section>
    </>
  );
}

/** The server's injected answer for this path, else a 404 for a non-address, else a fetch. */
function initialView(path: string, ip: string): View {
  const injected = injectedFor(path);
  if (injected.record) return { record: injected.record };
  if (injected.error) return { error: injected.error };
  if (!isIPv4(ip) && !isIPv6(ip)) {
    return { error: { status: 404, title: "Wrong ip", message: "Please provide a valid IP address" } };
  }
  return { loading: true };
}

/** The Record from `/{ip}/json`, or the API's error (Wrong ip, rate limit). */
async function loadRecord(ip: string): Promise<View> {
  try {
    const res = await fetch(`/${ip}/json`);
    const body: unknown = await res.json();
    if (res.ok) return { record: body as IpRecord };
    const error = (body as { error?: { title?: string; message?: string } }).error;
    if (error?.title && error.message) {
      return { error: { status: res.status, title: error.title, message: error.message } };
    }
  } catch {
    // Network failure or a body that isn't JSON.
  }
  return { failed: true };
}
