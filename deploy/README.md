# Deploying iplookup

iplookup runs as one static binary behind nginx, which terminates TLS and
answers ACME challenges. The binary serves the API and the web pages
itself. The only state is the data directory (the two Data Source files,
which the app downloads and keeps up to date); everything else can be
reinstalled from a release.

Files (in this directory, and at the top of each release tarball):

| File | Installed as |
| --- | --- |
| `iplookup` (release only) | `/opt/iplookup/vX.Y.Z/iplookup` |
| `web/` (release only) | `/opt/iplookup/vX.Y.Z/web/` |
| `iplookup.toml` | `/etc/iplookup/config.toml` (root:iplookup, 0640) |
| `iplookup.service` | `/etc/systemd/system/iplookup.service` |
| `nginx-iplookup.conf` | `/etc/nginx/sites-available/ip.example.com` |
| `unbound-iplookup.conf` | `/etc/unbound/unbound.conf.d/iplookup.conf` |

(In a release tarball this file is `DEPLOY.md` and the API documentation is
`README.md`.)

The samples are written for the reference deployment, ip.aka.cy. On your
own domain, replace `ip.aka.cy`, `v4.ip.aka.cy` and `v6.ip.aka.cy` in
`nginx-iplookup.conf` and in `[hosts]` of the config.

Layout on the server:

```
/opt/iplookup/v0.1.0/iplookup     the binary of one release
/opt/iplookup/v0.1.0/web/         its web pages
/opt/iplookup/current -> v0.1.0   the release in use (unit and config point here)
/usr/local/bin/iplookup -> /opt/iplookup/current/iplookup
/etc/iplookup/config.toml         config with credentials (root:iplookup, 0640)
/var/lib/iplookup/                Data Source files (iplookup:iplookup, 0750,
                                  created by systemd's StateDirectory)
```

Upgrading or rolling back is pointing `current` at another release
directory and restarting.

## Before you start

- **DNS.** `ip.example.com` with A and AAAA records, `v4.ip.example.com`
  with only an A record, `v6.ip.example.com` with only an AAAA record, all
  pointing at the server. Behind Cloudflare they must be DNS-only (grey
  cloud): a proxy would hide the visitor's address and protocol.
- **Data Source accounts.** A free MaxMind account (account ID and a
  GeoLite licence key) and a free IPinfo account (Lite token).
- **Firewall.** Ports 80 and 443 open on IPv4 and IPv6. Port 18310 and 53
  stay closed (both listen on loopback only anyway).

## Getting a release

Pushing a tag `vX.Y.Z` (equal to the versions in `server/Cargo.toml` and
`web/package.json`) makes the CI workflow (`.github/workflows/ci.yml`)
attach to a GitHub Release:

- `iplookup-vX.Y.Z-x86_64-linux-musl.tar.gz` and
  `iplookup-vX.Y.Z-aarch64-linux-musl.tar.gz`: each unpacks to a directory
  of the same name holding a statically linked binary (any Linux
  distribution), the web pages (`web/`), these deploy files, `README.md`
  and `LICENSE`;
- `SHA256SUMS`.

Without a release, build the same tarballs yourself (macOS or Linux, with
[zig](https://ziglang.org), `cargo install cargo-zigbuild` and, on macOS,
GNU tar as `gtar`):

```sh
rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl
(cd server && cargo zigbuild --release --locked \
  --target x86_64-unknown-linux-musl --target aarch64-unknown-linux-musl)
(cd web && pnpm install --frozen-lockfile && pnpm build)
scripts/package.sh v0.1.0 dist
```

## Fresh install

Run as root. Set `V` to the release and `ARCH` to `x86_64` or `aarch64`.

```sh
V=v0.1.0 ARCH=x86_64
cd /tmp
base=https://github.com/Doublefire-Chen/iplookup/releases/download/$V
curl -fLO $base/iplookup-$V-$ARCH-linux-musl.tar.gz
curl -fLO $base/SHA256SUMS
sha256sum --ignore-missing -c SHA256SUMS
tar -xzf iplookup-$V-$ARCH-linux-musl.tar.gz
pkg=/tmp/iplookup-$V-$ARCH-linux-musl

# 1. A dedicated system user with no login shell.
useradd --system --user-group --home-dir /var/lib/iplookup --no-create-home \
  --shell /usr/sbin/nologin iplookup

# 2. Program files (owned by root, read-only to the service).
mkdir -p /opt/iplookup/$V
install -m 0755 $pkg/iplookup /opt/iplookup/$V/iplookup
cp -R $pkg/web /opt/iplookup/$V/web
ln -sfn $V /opt/iplookup/current
ln -sfn /opt/iplookup/current/iplookup /usr/local/bin/iplookup

# 3. Local resolver for reverse DNS (hostname). Optional: without it, remove
#    [dns] resolver from the config and the system resolver is used.
apt install unbound
install -m 0644 $pkg/unbound-iplookup.conf /etc/unbound/unbound.conf.d/iplookup.conf
unbound-checkconf && systemctl restart unbound
dig +short -x 8.8.8.8 @127.0.0.1          # dns.google.
#    /etc/resolv.conf is not changed; only iplookup asks unbound. If the
#    unbound package's resolvconf hook switched it anyway (unbound-resolvconf
#    service), disable that: systemctl disable --now unbound-resolvconf

# 4. Config: put in the MaxMind account ID and licence key and the IPinfo
#    token; on your own domain, change [hosts].
install -d -m 0755 /etc/iplookup
install -o root -g iplookup -m 0640 $pkg/iplookup.toml /etc/iplookup/config.toml
editor /etc/iplookup/config.toml

# 5. The service. The first start downloads both Data Sources (a few
#    seconds; GeoLite2 City is about 50 MB) before listening.
install -m 0644 $pkg/iplookup.service /etc/systemd/system/iplookup.service
systemctl daemon-reload
systemctl enable --now iplookup
journalctl -u iplookup -n 20       # "... listening on http://127.0.0.1:18310"
curl -fsS http://127.0.0.1:18310/version

# 6. nginx, first without TLS so certbot can get the certificate: install
#    the site, comment out its second (port 443) server block, then
mkdir -p /var/www/letsencrypt
install -m 0644 $pkg/nginx-iplookup.conf /etc/nginx/sites-available/ip.example.com
ln -sfn /etc/nginx/sites-available/ip.example.com /etc/nginx/sites-enabled/ip.example.com
nginx -t && systemctl reload nginx
certbot certonly --webroot -w /var/www/letsencrypt \
  -d ip.example.com -d v4.ip.example.com -d v6.ip.example.com
#    then uncomment the 443 block (check the ssl_certificate paths) and
nginx -t && systemctl reload nginx
```

If a certificate for the three names already exists, skip the commenting
and certbot steps and point `ssl_certificate` at it. Renewal needs nothing
more: certbot's timer reuses the webroot, which nginx serves on port 80.
Use `certbot certonly --webroot`, not `certbot --nginx`: the nginx plugin
offers to add an HTTP→HTTPS redirect, which must not exist here.

## Check it

```sh
curl -fsS http://127.0.0.1:18310/version        # health check: fails unless both Data Sources are loaded
curl http://ip.example.com                       # your Record, over plain HTTP, no redirect
curl https://ip.example.com/8.8.8.8/org          # AS15169 Google LLC
curl -4 https://ip.example.com/ip; curl -6 https://ip.example.com/ip
curl https://v4.ip.example.com/ip; curl https://v6.ip.example.com/ip
curl -sI -H 'Accept: text/html' http://ip.example.com/ | head -n1   # 301 (browsers only)
```

Then open `https://ip.example.com/` in a browser: it shows your IPv4 and
IPv6 Records side by side.

`curl -fsS http://127.0.0.1:18310/version` is also the check for uptime
monitoring: `/version` answers `503` (and `curl -f` fails) unless both
Data Sources are loaded. From outside, `https://ip.example.com/version`.

## Running it

- Logs go to the journal: `journalctl -u iplookup -f`. Startup, data
  downloads and update failures are logged at `info`/`warn`; set
  `log_level = "debug"` in the config for one line per request.
- **Data updates need nothing:** the app checks both Data Sources every
  `[data] update_interval` (12 h) and swaps new files in without a
  restart. A failed check keeps the current data and retries later.
  `sudo -u iplookup iplookup -c /etc/iplookup/config.toml update` checks
  once, now (always as `iplookup`, so the files stay writable by the
  service).
- **Config changes** need `systemctl restart iplookup`.
- **The unit** restarts the app if it exits with an error and confines it:
  it runs as `iplookup` with no capabilities, sees `/usr`, `/etc` and
  `/opt` read-only and `/home` not at all, and can write only
  `/var/lib/iplookup` (systemd's `StateDirectory`). If you move
  `[data] dir`, add it to the unit as `ReadWritePaths=`.
- nginx must stay a pure proxy on both ports (ADR 0002): no
  `return 301 https://…` on port 80, and no HSTS, CORS or caching headers
  of its own. The app sets them. It believes `X-Real-IP` and
  `X-Forwarded-Proto` only from `trusted_proxies` (127.0.0.1 and ::1 in the
  sample).

## Upgrade

```sh
V=v0.2.0 ARCH=x86_64
# download, verify and unpack as in the fresh install, then:
mkdir -p /opt/iplookup/$V
install -m 0755 $pkg/iplookup /opt/iplookup/$V/iplookup
cp -R $pkg/web /opt/iplookup/$V/web
ln -sfn $V /opt/iplookup/current          # binary and web pages together
systemctl restart iplookup
journalctl -u iplookup -n 20
curl -fsS http://127.0.0.1:18310/version  # "version": "0.2.0"
```

Compare the new release's `iplookup.service`, `iplookup.toml`,
`nginx-iplookup.conf` and `unbound-iplookup.conf` with the installed ones
and carry over changes (`systemctl daemon-reload` /
`nginx -t && systemctl reload nginx`). The data directory is reused as is.
Keep the previous release directory for rollback; delete older ones
whenever.

## Rollback

```sh
ln -sfn v0.1.0 /opt/iplookup/current
systemctl restart iplookup
```

## Backup

Nothing needs backing up but `/etc/iplookup/config.toml` (credentials).
The Data Source files are downloaded again if missing. Don't keep old
copies of them around: the GeoLite licence asks for outdated copies to be
destroyed within 30 days.
