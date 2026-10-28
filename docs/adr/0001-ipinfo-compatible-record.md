# The Record keeps ipinfo.io's JSON shape

The JSON Record uses ipinfo.io's flat keys exactly (`ip`, `hostname`, `city`, `region`, `country`, `loc`, `org`, `postal`, `timezone`, `readme`), including awkward ones like `loc` as a `"lat,lon"` string and `org` as `"AS8434 Name"`. This means scripts and tools written for ipinfo.io work against ip.aka.cy by changing only the URL. We rejected a cleaner nested, typed shape because it would be compatible with nothing.

## Consequences

- New information goes only in extra keys added on the end: `country_name`, `continent`, `accuracy_radius` (km), and `asn` (`{"asn","name","domain"}`, the same key ipinfo's paid tier uses). Existing keys are never renamed or retyped.
- `readme` points to the project README on GitHub, the same URL for every deployment.
- Missing values leave the key out. We never send `null` or `""`.
- Edge cases also copy ipinfo.io: a Bogon returns `{"ip":…,"bogon":true}`; `ip` echoes the address exactly as requested (no canonicalising), though `::ffff:a.b.c.d` is looked up as `a.b.c.d`; invalid input and hostnames return `404` with `{"status":404,"error":{"title":"Wrong ip","message":"Please provide a valid IP address"}}`.
- One deliberate difference: a single Field that has no value returns `404` with an empty body, not ipinfo's literal `undefined`, so `$(curl -s ip.aka.cy/city)` comes back empty.
