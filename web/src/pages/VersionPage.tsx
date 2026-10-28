import { useEffect, useState } from "react";
import { CopyButton } from "../components/CopyButton";
import { injectedFor, mainHost } from "../injected";
import { VERSION_PATH } from "../links";
import {
  DATA_SOURCES,
  formatTime,
  isHealthy,
  isVersionInfo,
  type VersionInfo,
} from "../version";

type View = { info: VersionInfo } | { loading: true } | { failed: true };

/** `/version`: the backend version and each Data Source's state, with its credit. */
export function VersionPage() {
  const [view, setView] = useState<View>(() => {
    const injected = injectedFor(VERSION_PATH).version;
    return injected ? { info: injected } : { loading: true };
  });

  // After a client-side move (the footer link) nothing was injected: ask
  // the server for the same answer as JSON.
  useEffect(() => {
    if (!("loading" in view)) return;
    let cancelled = false;
    void loadVersion().then((next) => {
      if (!cancelled) setView(next);
    });
    return () => {
      cancelled = true;
    };
  }, [view]);

  const command = `curl ${mainHost()}${VERSION_PATH}`;
  return (
    <>
      <title>Version · iplookup</title>
      {"info" in view ? (
        <VersionDetails info={view.info} />
      ) : (
        <section className="card">
          <p className="muted">
            {"failed" in view ? "The version couldn't be loaded." : "Loading…"}
          </p>
        </section>
      )}
      <section className="card code-card" aria-label="From a terminal">
        <div className="code-head">
          <h2>From a terminal</h2>
          <CopyButton text={command} />
        </div>
        <pre className="code">
          <code>{command}</code>
        </pre>
      </section>
    </>
  );
}

function VersionDetails({ info }: { info: VersionInfo }) {
  const healthy = isHealthy(info);
  return (
    <>
      <section className="card record-card" aria-label="Backend">
        <div className="record-head">
          <h2>Backend version</h2>
          <span className={healthy ? "badge" : "badge badge-error"}>
            {healthy ? "Healthy" : "Unavailable"}
          </span>
        </div>
        <p className="record-ip">{info.version}</p>
        <p className="muted version-note">
          {healthy
            ? "Both Data Sources are loaded."
            : "Not every Data Source is loaded, so Lookups are incomplete."}
        </p>
      </section>
      <div className="self-cards">
        {DATA_SOURCES.map((ds) => {
          const state = info.data[ds.key];
          return (
            <section key={ds.key} className="card record-card" aria-label={ds.title}>
              <div className="record-head">
                <h2>{ds.title}</h2>
                {!state && <span className="badge badge-error">Not loaded</span>}
              </div>
              <p className="record-none">{ds.product}</p>
              <dl className="record-rows">
                {state && (
                  <>
                    <div className="record-row">
                      <dt>Database</dt>
                      <dd>{state.source}</dd>
                    </div>
                    <div className="record-row">
                      <dt>Built</dt>
                      <dd>{formatTime(state.build)}</dd>
                    </div>
                    <div className="record-row">
                      <dt>Last checked</dt>
                      <dd>{state.checked ? formatTime(state.checked) : "Not yet"}</dd>
                    </div>
                  </>
                )}
                <div className="record-row">
                  <dt>Credit</dt>
                  <dd>
                    {ds.key === "location" ? (
                      <>
                        This product includes GeoLite Data created by MaxMind, available from{" "}
                        <a href={ds.url}>maxmind.com</a>.
                      </>
                    ) : (
                      <>
                        IP address data powered by <a href={ds.url}>IPinfo</a>.
                      </>
                    )}
                  </dd>
                </div>
              </dl>
            </section>
          );
        })}
      </div>
    </>
  );
}

/** The /version JSON (its body has the same shape on 200 and 503). */
async function loadVersion(): Promise<View> {
  try {
    const res = await fetch(VERSION_PATH, { headers: { accept: "application/json" } });
    const body: unknown = await res.json();
    if (isVersionInfo(body)) return { info: body };
  } catch {
    // Network failure or a body that isn't JSON.
  }
  return { failed: true };
}
