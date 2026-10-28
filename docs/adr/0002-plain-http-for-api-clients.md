# Plain HTTP is served to API clients, not redirected

`curl ip.aka.cy` uses `http://` and curl does not follow redirects by default, so the usual nginx `301 → https` on port 80 would make the main use case print a "301 Moved Permanently" page. On all three hostnames (`ip`, `v4.ip`, `v6.ip`), port 80 is therefore proxied to the app instead of redirected. The app redirects to HTTPS only requests that want a page (`Accept: text/html`); every other client gets its JSON or plain-text answer over HTTP. Do not "fix" this by adding a blanket HTTPS redirect in nginx.

## Consequences

- nginx still answers `/.well-known/acme-challenge/` on port 80 itself, so certbot renewal keeps working.
- HSTS is sent only on HTTPS responses; browsers stay on HTTPS, while curl ignores HSTS.
- The API documentation (README) recommends `https://` for scripts.
