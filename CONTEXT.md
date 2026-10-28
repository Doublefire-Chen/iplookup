# iplookup

A public service at ip.aka.cy that tells you where an IP address is and which network it belongs to, for both people in a browser and scripts using curl. Handles IPv4 and IPv6.

## Language

**Lookup**:
One request that turns an IP address into a Record.
_Avoid_: query, search, resolve

**Self Lookup**:
A Lookup of the IP address the request came from.
_Avoid_: my-IP lookup, own lookup

**Explicit Lookup**:
A Lookup of an IP address the requester names.
_Avoid_: manual lookup, target lookup

**Record**:
The answer to a Lookup: the IP address plus everything known about it (hostname, location, network).
_Avoid_: result, response, info

**Field**:
One named item of a Record (such as ip, city or org) that can be requested on its own and is returned as plain text.
_Avoid_: attribute, property, key

**Bogon**:
An IP address that is not routable on the public internet (private, loopback, link-local, documentation and other reserved ranges). Its Record carries no Location Data, Network Data or hostname.
_Avoid_: private IP, reserved IP, internal address

**Location Data**:
The part of a Record that says where an IP address is (city, region, country, postal code, coordinates, timezone). Comes from MaxMind GeoLite2 City.
_Avoid_: geo, geoip, geolocation

**Accuracy Radius**:
How far, in kilometres, the true location may be from the Location Data's coordinates. The coordinates are a rough centre, never a precise point.
_Avoid_: precision, error margin

**Network Data**:
The part of a Record that says which network an IP address belongs to (ASN, organisation name, organisation domain). Comes from IPinfo Lite.
_Avoid_: ASN info, org info, ISP

**Data Source**:
A third-party database that supplies Location Data or Network Data. Each one must be credited wherever its data is shown.
_Avoid_: provider, feed, vendor
