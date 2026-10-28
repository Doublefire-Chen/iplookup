import { ipVersion, recordRows } from "../format";
import type { IpRecord } from "../record";

/** One Record as a card: the address up top, then what is known about it. */
export function RecordCard({ title, record }: { title: string; record: IpRecord }) {
  const rows = recordRows(record);
  return (
    <section className="card record-card" aria-label={title}>
      <div className="record-head">
        <h2>{title}</h2>
        <span className="badge">{ipVersion(record.ip)}</span>
      </div>
      <p className="record-ip">{record.ip}</p>
      {record.bogon ? (
        <p className="muted">Not a public address: it has no location or network.</p>
      ) : rows.length === 0 ? (
        <p className="muted">Nothing is known about this address.</p>
      ) : (
        <dl className="record-rows">
          {rows.map((row) => (
            <div key={row.label} className="record-row">
              <dt>{row.label}</dt>
              <dd>{row.value}</dd>
            </div>
          ))}
        </dl>
      )}
    </section>
  );
}
