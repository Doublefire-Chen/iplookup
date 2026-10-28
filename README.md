# iplookup

A self-hosted, ipinfo.io-compatible IP lookup service. `curl` it to get your
Record as JSON:

```console
$ curl ip.aka.cy
{
  "ip": "8.8.8.8",
  "country": "US",
  "loc": "37.7510,-97.8220",
  "org": "AS15169 Google LLC",
  ...
}
```

Single Fields (`ip`, `hostname`, `city`, `region`, `country`, `loc`, `org`,
`postal`, `timezone`) come back as plain text, for yours or any address:

```console
$ curl ip.aka.cy/ip
8.8.8.8
$ curl ip.aka.cy/1.1.1.1/org
AS13335 Cloudflare, Inc.
```

A Field with no value returns 404 with an empty body.

## Rate limits

Looking up your own address (`/`, `/json`, `/{field}`) is never limited.
Looking up other addresses (`/{ip}`, `/{ip}/json`, `/{ip}/{field}`) is
limited per client, where a client is an IPv4 address or an IPv6 /64: by
default 60 in a row, then 1 more per second. Over the limit you get `429`
with a `Retry-After` header (in seconds) and

```json
{
  "status": 429,
  "error": {
    "title": "Rate limit exceeded",
    "message": "Too many lookups of other addresses. Please retry after 1 second."
  }
}
```

Operators set this in the config file:

```toml
[rate_limit]
burst = 60        # lookups in a row
per_second = 1    # tokens back per second (fractions allowed)
allow = ["192.0.2.0/24", "2001:db8::1"]  # IPs and CIDRs never limited
```

Behind a reverse proxy, list it in `trusted_proxies` so clients are told
apart by `X-Real-IP` rather than all sharing the proxy's bucket.

Work in progress: the full API documentation comes with later releases.

## Layout

- `server/`: the Rust server (axum). `iplookup --config iplookup.toml serve`.
- `web/`: the web pages (Vite + React + TypeScript, pnpm).
- `deploy/`: sample deployment files.

## Data

Location Data comes from MaxMind GeoLite2 City and Network Data from IPinfo
Lite. Both are free with an account; put the MaxMind account ID and licence
key (`[maxmind]`) and the IPinfo token (`[ipinfo]`) in the config file (see
`deploy/iplookup.toml`) and keep it mode 0600.

iplookup downloads both files into the data directory (`[data] dir`) itself:
missing files before it starts listening (it exits with an error if that
fails), then checks for new data every `[data] update_interval` (default
`12h`) and swaps it in without a restart. A failed update keeps the current
data, logs a warning and retries later. Only one copy of each file is kept.
`iplookup update` checks once, now; `iplookup update --data-dir DIR` fills
another directory.

This product includes GeoLite Data created by MaxMind, available from
<https://www.maxmind.com>. IP address data is powered by
[IPinfo](https://ipinfo.io), licensed under CC BY-SA 4.0.

## Development

```sh
cd server && cargo test
cd web && pnpm install && pnpm test && pnpm build
```

The real-data tests read both databases from `server/testdata/`, which is
gitignored and must never be committed; fill or refresh it with
`iplookup --config <your config> update --data-dir server/testdata`. Without
the files those tests skip. The updater tests use a local stand-in for both
download servers.
No test does real DNS, except one opt-in check of the real resolver:
`cargo test --test reverse_dns -- --ignored`.

## Licence

GPL-3.0. See [LICENSE](LICENSE).
