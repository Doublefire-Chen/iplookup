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

Work in progress: the full API documentation comes with later releases.

## Layout

- `server/`: the Rust server (axum). `iplookup --config iplookup.toml serve`.
- `web/`: the web pages (Vite + React + TypeScript, pnpm).
- `deploy/`: sample deployment files.

## Data

Location Data comes from MaxMind GeoLite2 City and Network Data from IPinfo
Lite. For now, put `GeoLite2-City.mmdb` and `ipinfo_lite.mmdb` in the
configured data directory (`[data] dir`) by hand.

This product includes GeoLite Data created by MaxMind, available from
<https://www.maxmind.com>. IP address data is powered by
[IPinfo](https://ipinfo.io), licensed under CC BY-SA 4.0.

## Development

```sh
cd server && cargo test
cd web && pnpm install && pnpm test && pnpm build
```

The real-data tests read both databases from `server/testdata/`, which is
gitignored and must never be committed. Without the files those tests skip.

## Licence

GPL-3.0. See [LICENSE](LICENSE).
