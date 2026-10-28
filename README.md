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

## Web page

A browser (any request whose `Accept` header contains `text/html`) gets a web
page instead of JSON: `/` shows your own Record, `/{ip}` that address's
Record as a table, with the raw JSON and a ready-to-copy
`curl https://{hosts.main}/{ip}` command. Both pages have a search box.
Invalid addresses and unknown paths get a 404 page (curl still gets the
"Wrong ip" JSON), and a rate-limited `/{ip}` gets a 429 page with the same
`Retry-After`. `/json`, `/{ip}/json` and the Field paths always return JSON
or plain text, even in a browser.

Set the public hostname the pages show in their commands:

```toml
[hosts]
main = "ip.example.com"   # no scheme; unset, pages use the host they came from
```

Plain HTTP is answered directly, so `curl ip.aka.cy` works without
`https://` or `-L`; only browsers are redirected (`301`) to the same URL on
HTTPS, and HTTPS responses carry `Strict-Transport-Security`. Scripts
should still use `https://`. The app learns the scheme from the
`X-Forwarded-Proto` header of a trusted proxy (`trusted_proxies`); a request
from anyone else, or without the header, is never redirected and gets no
HSTS.

## Rate limits

Looking up your own address (`/`, `/json`, `/{field}`) is never limited.
Looking up other addresses (`/{ip}`, `/{ip}/json`, `/{ip}/{field}`, and
the `/{ip}` web page) is limited per client, where a client is an IPv4 address or an IPv6 /64: by
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
- `web/`: the web pages (Vite + React + TypeScript, pnpm). The server serves
  the build (`pnpm build` → `web/dist`) from `[frontend] dir`, injecting the
  Record into `index.html`.
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

To work on the web pages, run the server (with the real-data databases,
say) and the Vite dev server side by side:

```sh
cd server && cargo run -- --config iplookup.toml serve   # listens on 127.0.0.1:18310
cd web && pnpm dev                                       # http://localhost:5173
```

`pnpm dev` serves the pages itself and proxies `/json` and `/{ip}/json` to
the server; the pages then have no injected Record and fetch it instead. To check the real
thing, `pnpm build` and point `[frontend] dir` at `../web/dist`.

The real-data tests read both databases from `server/testdata/`, which is
gitignored and must never be committed; fill or refresh it with
`iplookup --config <your config> update --data-dir server/testdata`. Without
the files those tests skip. The updater tests use a local stand-in for both
download servers.
No test does real DNS, except one opt-in check of the real resolver:
`cargo test --test reverse_dns -- --ignored`.

## Licence

GPL-3.0. See [LICENSE](LICENSE).
