# iplookup

A self-hosted, ipinfo.io-compatible IP lookup service. It tells you where
an IP address is and which network it belongs to, for IPv4 and IPv6, from
a terminal or a browser. The JSON has ipinfo.io's shape, so scripts written
for ipinfo.io work by changing only the URL.

```console
$ curl ip.example.com
{
  "ip": "81.2.69.142",
  ...
}
```

The reference deployment is [ip.aka.cy](https://ip.aka.cy) (with
`v4.ip.aka.cy` and `v6.ip.aka.cy`). The examples below use
`ip.example.com`; replace it with the deployment you use. To run your own,
see [Install and deploy](#install-and-deploy).

- [API](#api): [routes](#routes), [the Record](#the-record),
  [Fields](#fields), [Bogons](#bogons), [errors](#errors),
  [rate limits](#rate-limits), [/version](#version-and-health-check),
  [IPv4 and IPv6](#ipv4-and-ipv6), [HTTP and HTTPS](#http-and-https)
- [Web page](#web-page)
- [Data and credits](#data-and-credits)
- [Install and deploy](#install-and-deploy)
- [Development](#development)

## API

A **Lookup** turns an IP address into a **Record**. A **Self Lookup** looks
up the address the request came from; an **Explicit Lookup** looks up an
address you name in the path. A **Field** is one item of the Record on its
own, as plain text.

### Routes

| Path | Answer |
| --- | --- |
| `/` | Your Record: JSON (a page in a browser) |
| `/json` | Your Record: always JSON |
| `/{field}` | One Field of your Record: plain text |
| `/{ip}` | That address's Record: JSON (a page in a browser) |
| `/{ip}/json` | That address's Record: always JSON |
| `/{ip}/{field}` | One Field of that address's Record: plain text |
| `/version` | Server version and Data Source state: JSON (a page in a browser) |

Anything else is a `404` [error](#errors). `{ip}` is an IPv4 or IPv6
address written in the usual way (`8.8.8.8`, `2001:4860:4860::8888`);
hostnames, leading zeros, zone IDs (`fe80::1%eth0`) and CIDR ranges are
not addresses.

**Page or data.** `/`, `/{ip}` and `/version` answer with a web page when
the request's `Accept` header contains `text/html` (as browsers' does) and
with JSON otherwise, so curl, scripts and `fetch()` get data from the same
URL a person opens in a browser. These responses carry `Vary: Accept`.
`/json`, `/{ip}/json` and the Field paths ignore `Accept`: they always
return JSON or plain text, even in a browser.

Every JSON and plain-text response carries
`Access-Control-Allow-Origin: *`, so web pages anywhere can call the API.
JSON is pretty-printed and ends in a newline; its content type is
`application/json; charset=utf-8`.

```console
$ curl ip.example.com/8.8.8.8
$ curl ip.example.com/2001:4860:4860::8888/json
$ curl -H 'Accept: application/json' https://ip.example.com/8.8.8.8
```

### The Record

```console
$ curl https://ip.example.com/81.2.69.142
{
  "ip": "81.2.69.142",
  "hostname": "142.69.2.81.in-addr.arpa",
  "city": "Milton Keynes",
  "region": "England",
  "country": "GB",
  "loc": "51.9582,-0.5872",
  "org": "AS20712 Andrews & Arnold Ltd",
  "postal": "MK17",
  "timezone": "Europe/London",
  "readme": "https://github.com/Doublefire-Chen/iplookup/blob/main/README.md",
  "country_name": "United Kingdom",
  "continent": "Europe",
  "accuracy_radius": 200,
  "asn": {
    "asn": "AS20712",
    "name": "Andrews & Arnold Ltd",
    "domain": "aa.net.uk"
  }
}
```

The keys come in this order: ipinfo.io's keys first, with ipinfo.io's
formats, then the extras.

| Key | Value | From |
| --- | --- | --- |
| `ip` | The address exactly as you wrote it (not canonicalised); for a Self Lookup, your address | the request |
| `hostname` | The reverse DNS (PTR) name, without the trailing dot. Not forward-confirmed. Left out when there is none or the DNS answer takes too long (500 ms by default) | reverse DNS |
| `city` | City name, in English | Location Data |
| `region` | Region (state, province) name, in English | Location Data |
| `country` | ISO 3166-1 alpha-2 code, e.g. `GB` | Location Data |
| `loc` | `"latitude,longitude"`, four decimals. A rough centre, never a precise point: see `accuracy_radius` | Location Data |
| `org` | `"AS<number> <network name>"`, e.g. `AS15169 Google LLC` | Network Data |
| `postal` | Postal code | Location Data |
| `timezone` | IANA time zone, e.g. `Europe/London` | Location Data |
| `readme` | A link to this document, the same for every deployment | always |
| `country_name` | Country name, in English (extra) | Location Data |
| `continent` | Continent name, in English, e.g. `North America` (extra) | Location Data |
| `accuracy_radius` | How far, in kilometres, the true location may be from `loc` (extra) | Location Data |
| `asn` | `{"asn": "AS15169", "name": "Google LLC", "domain": "google.com"}`, shaped like ipinfo.io's paid-tier `asn` object (extra) | Network Data |

A key whose value is unknown is left out; nothing is ever `null` or `""`.
Many addresses have no `city` or `postal`, some have no location at all.
`::ffff:a.b.c.d` (an IPv4-mapped IPv6 address) is looked up as `a.b.c.d`,
though `ip` still echoes what you wrote.

ipinfo.io's `anycast`, privacy, abuse, company and carrier data are not
available: neither Data Source has them.

### Fields

The nine ipinfo.io keys can be fetched on their own as plain text
(`text/plain; charset=utf-8`), the value followed by a newline:

`ip`, `hostname`, `city`, `region`, `country`, `loc`, `org`, `postal`,
`timezone`

```console
$ curl ip.example.com/ip
81.2.69.142
$ curl ip.example.com/8.8.8.8/org
AS15169 Google LLC
$ city=$(curl -s https://ip.example.com/city)
```

Field names are lower case and exact. The extras (`country_name`,
`continent`, `accuracy_radius`, `asn`) and `readme` are JSON-only; use
`jq` for those: `curl -s ip.example.com/json | jq -r .asn.domain`.

A Field with no value (say, `city` for an address without one) returns
`404` with an **empty** body, so `$(curl -s …)` comes back empty. (ipinfo.io
answers `undefined` instead; this is the one deliberate difference.)
`/{ip}/{name}` with a name that is not a Field is a "Wrong ip" error.

### Bogons

A Bogon is an address that isn't routable on the public internet: private
(`10.0.0.0/8`, `192.168.0.0/16`, `fc00::/7` …), loopback, link-local,
documentation, multicast and other reserved ranges, including 6to4 and
Teredo addresses that embed one. Its Record has nothing but the address:

```console
$ curl ip.example.com/192.168.1.1
{
  "ip": "192.168.1.1",
  "bogon": true
}
```

The same goes for a Self Lookup from a Bogon (for example over a LAN).
Every Field of a Bogon except `ip` is empty (`404`). Bogons have no
`readme` and are never looked up in DNS.

### Errors

Errors are JSON in ipinfo.io's shape, with `Access-Control-Allow-Origin: *`.
(A browser asking for a page, `Accept: text/html`, gets an error page
with the same status instead.)

An invalid address (including hostnames such as `google.com`), an unknown
path, or `/{ip}/{x}` where `x` is neither `json` nor a Field:

```console
$ curl -i ip.example.com/google.com
HTTP/1.1 404 Not Found
content-type: application/json; charset=utf-8

{
  "status": 404,
  "error": {
    "title": "Wrong ip",
    "message": "Please provide a valid IP address"
  }
}
```

Too many Explicit Lookups (see [Rate limits](#rate-limits)):

```console
$ curl -i ip.example.com/8.8.8.8
HTTP/1.1 429 Too Many Requests
retry-after: 1
content-type: application/json; charset=utf-8

{
  "status": 429,
  "error": {
    "title": "Rate limit exceeded",
    "message": "Too many lookups of other addresses. Please retry after 1 second."
  }
}
```

An empty Field is `404` with an empty body, as above. `/version` answers
`503` when a Data Source isn't loaded (see below).

### Rate limits

Self Lookups (`/`, `/json`, `/{field}`) are never limited, and neither is
`/version`.

Explicit Lookups (`/{ip}`, `/{ip}/json`, `/{ip}/{field}`, and the `/{ip}`
web page) are limited per client, where a client is an IPv4 address or an
IPv6 /64. The defaults are 60 in a row, then 1 more per second (a token
bucket: after a minute without Explicit Lookups, you have 60 again). Over
the limit you get `429` with a `Retry-After` header in seconds and the
error above; a browser gets a "Too many lookups" page. "Wrong ip" requests
don't count.

Operators set this in the config file:

```toml
[rate_limit]
burst = 60        # lookups in a row
per_second = 1    # lookups back per second (fractions allowed)
allow = ["192.0.2.0/24", "2001:db8::1"]  # IPs and CIDRs never limited
```

Behind a reverse proxy, list it in `trusted_proxies` so clients are told
apart by `X-Real-IP` rather than all sharing the proxy's bucket.

### Version and health check

`/version` reports the server version and each Data Source's state:
`source` (which Data Source), `build` (when the loaded database was built)
and `checked` (the last successful check for new data, left out until
there is one). Times are RFC 3339 in UTC.

```console
$ curl ip.example.com/version
{
  "version": "0.1.0",
  "data": {
    "location": {
      "source": "MaxMind GeoLite2 City",
      "build": "2026-10-27T17:52:02Z",
      "checked": "2026-10-28T21:41:25Z"
    },
    "network": {
      "source": "IPinfo Lite",
      "build": "2026-10-28T08:01:57Z",
      "checked": "2026-10-28T21:41:26Z"
    }
  }
}
```

It doubles as the health check for uptime monitoring: `200` when both
databases are loaded, otherwise `503` with the same JSON, the Data Source
that isn't loaded left out. `curl -fsS https://ip.example.com/version`
fails on a `503`. A browser gets a page with the same information and the
data credits.

### IPv4 and IPv6

`ip.example.com` answers over both protocols, so a Self Lookup returns the
address your connection happened to use. To pick one, use curl's `-4` or
`-6`:

```console
$ curl -4 ip.example.com/ip
81.2.69.142
$ curl -6 ip.example.com/ip
2001:db8:1234::1
```

A deployment may also have a hostname per protocol, reachable only over
that protocol (ip.aka.cy has `v4.ip.aka.cy` and `v6.ip.aka.cy`). They
answer exactly like the main one:

```console
$ curl v4.ip.example.com      # your IPv4 Record (fails without IPv4)
$ curl v6.ip.example.com/ip   # your IPv6 address (fails without IPv6)
```

### HTTP and HTTPS

**Scripts should use `https://`.** Plain HTTP is answered directly rather
than redirected, so a quick `curl ip.example.com` works without
`https://` or `-L`. Browsers (requests with `Accept: text/html`) are
redirected (`301`) to the same URL on HTTPS, and HTTPS responses carry
`Strict-Transport-Security`. Over plain HTTP the answer can be read or
changed on the way, so anything that acts on the result should ask over
HTTPS:

```sh
curl -fsS https://ip.example.com/ip
```

## Web page

A browser gets web pages from the same URLs:

- `/` shows your Record. With per-protocol hostnames configured, it shows
  your IPv4 and IPv6 Records side by side, fetching the other one from
  `https://v4…/json` or `https://v6…/json`; a protocol you don't have shows
  "No IPv6 connectivity" (or IPv4).
- `/{ip}` shows that address's Record as a table, the raw JSON, and a
  ready-to-copy `curl` command.
- Both have a search box. Records with coordinates and an Accuracy Radius
  get a map with a circle of that radius, deliberately without a pin (the
  location is never a precise point). The map uses OpenStreetMap's tiles,
  which the browser loads from `tile.openstreetmap.org`.
- `/version` shows the server version and the Data Sources' state.
- Invalid addresses and unknown paths get a 404 page; a rate-limited
  `/{ip}` gets a "Too many lookups" page.

## Data and credits

**Location Data** (city, region, country, postal code, coordinates, time
zone, Accuracy Radius) comes from **MaxMind GeoLite2 City**. **Network
Data** (ASN, network name and domain) comes from **IPinfo Lite**.
**hostname** is the server's own reverse DNS. The data is approximate:
GeoLite places many addresses only at country level, at the centre of the
country.

This product includes GeoLite Data created by MaxMind, available from
<https://www.maxmind.com>.
IP address data is powered by [IPinfo](https://ipinfo.io), licensed under
[CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/).
Map tiles and map data © [OpenStreetMap](https://www.openstreetmap.org/copyright)
contributors.

If you run iplookup, the GeoLite licence (the
[GeoLite End User License Agreement](https://www.maxmind.com/en/geolite2/eula))
applies to you:

- keep the attribution above wherever the data is shown (the web pages'
  footer and `/version` page do);
- **use new GeoLite releases promptly and destroy old copies within 30
  days.** iplookup does this itself: it checks for new data every
  `[data] update_interval` (12 hours by default), replaces the file in
  place and keeps no old copies. Keep the updater configured, and don't
  keep copies of the database elsewhere;
- don't use the data to identify a specific household or individual.

You need your own free accounts: a MaxMind account ID and GeoLite licence
key, and an IPinfo Lite token.

## Install and deploy

iplookup is one static binary for Linux (x86_64 or aarch64), a directory of
web files and one config file, run by systemd behind nginx. Each
[release](https://github.com/Doublefire-Chen/iplookup/releases) has a
tarball per architecture with the binary, the web build, `LICENSE`, this
README and the deploy samples:

- `iplookup.toml`: the config, every key documented with its default;
- `iplookup.service`: a hardened systemd unit;
- `nginx-iplookup.conf`: TLS, ACME challenges, and both ports proxied
  (port 80 must not be redirected wholesale:
  [ADR 0002](docs/adr/0002-plain-http-for-api-clients.md));
- `unbound-iplookup.conf`: a localhost-only resolver for reverse DNS;
- `DEPLOY.md`: install, check, upgrade and rollback steps.

The same files are in [`deploy/`](deploy/); the steps are in
[`deploy/README.md`](deploy/README.md). In short:

1. Point `ip.example.com` (A and AAAA), and optionally `v4.ip.example.com`
   (A only) and `v6.ip.example.com` (AAAA only), at the server.
2. Unpack the release to `/opt/iplookup/vX.Y.Z` and link
   `/opt/iplookup/current` to it.
3. Install the config as `/etc/iplookup/config.toml` with your MaxMind and
   IPinfo credentials and your hostnames.
4. Install and start the systemd unit; on first start iplookup downloads
   both databases, then keeps them up to date by itself.
5. Install the nginx site and get a certificate for the hostnames.
6. Check: `curl -fsS http://127.0.0.1:18310/version`.

The command line:

```console
$ iplookup -c /etc/iplookup/config.toml serve    # run the server
$ iplookup -c /etc/iplookup/config.toml update   # check both Data Sources once, now
$ iplookup -c config.toml update --data-dir DIR  # ... into another directory
```

`-c` defaults to `iplookup.toml` (or `$IPLOOKUP_CONFIG`).

## Development

Layout:

- `server/`: the Rust server (axum). It serves the API and the web build
  from `[frontend] dir`, injecting the Record into `index.html`.
- `web/`: the web pages (Vite + React + TypeScript, pnpm).
- `deploy/`: the deploy samples.
- `scripts/package.sh`: builds release tarballs (used by CI on `v*` tags).

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

`pnpm dev` serves the pages itself and proxies `/json`, `/{ip}/json` and
(for the page's fetch, not the page) `/version` to the server; the pages
then have no injected Record and fetch it instead. To check the real
thing, `pnpm build` and point `[frontend] dir` at `../web/dist`.

The real-data tests read both databases from `server/testdata/`, which is
gitignored and must never be committed (licence); fill or refresh it with
`iplookup --config <your config> update --data-dir server/testdata`.
Without the files those tests skip. The updater tests use a local stand-in
for both download servers. No test does real DNS, except one opt-in check
of the real resolver: `cargo test --test reverse_dns -- --ignored`.

Releases: set the same version in `server/Cargo.toml` and
`web/package.json`, commit, and push a tag `vX.Y.Z`; CI checks the versions
match, builds both architectures and publishes the GitHub Release.

## Licence

GPL-3.0. See [LICENSE](LICENSE).
