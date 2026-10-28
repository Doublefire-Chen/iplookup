//! Tiny MaxMind DB files built at test time (no fixture files are
//! committed). Each one maps every address to the same data, so a test can
//! tell which version is loaded from a single Lookup.

/// A GeoLite2 City stand-in: every address is in `city`, US.
pub fn location(city: &str, build_epoch: u64) -> Vec<u8> {
    let names = |name: &str| map(vec![("en", text(name))]);
    let record = map(vec![
        ("city", map(vec![("names", names(city))])),
        (
            "country",
            map(vec![
                ("iso_code", text("US")),
                ("names", names("United States")),
            ]),
        ),
        (
            "location",
            map(vec![
                ("accuracy_radius", Value::U16(1000)),
                ("latitude", Value::F64(37.751)),
                ("longitude", Value::F64(-97.822)),
                ("time_zone", text("America/Chicago")),
            ]),
        ),
    ]);
    mmdb("GeoLite2-City", build_epoch, &record)
}

/// An IPinfo Lite stand-in: every address is in `asn`, run by `name`.
pub fn network(asn: &str, name: &str, build_epoch: u64) -> Vec<u8> {
    let record = map(vec![
        ("asn", text(asn)),
        ("as_name", text(name)),
        ("as_domain", text("example.com")),
    ]);
    mmdb("ipinfo_lite.mmdb", build_epoch, &record)
}

enum Value {
    Text(String),
    U16(u16),
    U32(u32),
    U64(u64),
    F64(f64),
    Map(Vec<(String, Value)>),
    Array(Vec<Value>),
}

fn text(s: &str) -> Value {
    Value::Text(s.to_owned())
}

fn map(pairs: Vec<(&str, Value)>) -> Value {
    Value::Map(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

/// An IPv6 database with a one-node search tree whose both branches point
/// at `record`, so every address (IPv4 included) finds it.
fn mmdb(database_type: &str, build_epoch: u64, record: &Value) -> Vec<u8> {
    const NODE_COUNT: u32 = 1;
    // A data pointer is node_count + 16 (the separator) + the offset.
    let pointer = (NODE_COUNT + 16).to_be_bytes();
    let mut out = Vec::new();
    for _ in 0..2 {
        out.extend_from_slice(&pointer[1..]); // 24-bit records
    }
    out.extend_from_slice(&[0; 16]);
    encode(record, &mut out);
    out.extend_from_slice(b"\xab\xcd\xefMaxMind.com");
    let metadata = map(vec![
        ("binary_format_major_version", Value::U16(2)),
        ("binary_format_minor_version", Value::U16(0)),
        ("build_epoch", Value::U64(build_epoch)),
        ("database_type", text(database_type)),
        ("description", map(vec![("en", text("iplookup test data"))])),
        ("ip_version", Value::U16(6)),
        ("languages", Value::Array(vec![text("en")])),
        ("node_count", Value::U32(NODE_COUNT)),
        ("record_size", Value::U16(24)),
    ]);
    encode(&metadata, &mut out);
    out
}

fn encode(value: &Value, out: &mut Vec<u8>) {
    match value {
        Value::Text(s) => {
            control(2, s.len(), out);
            out.extend_from_slice(s.as_bytes());
        }
        Value::F64(f) => {
            control(3, 8, out);
            out.extend_from_slice(&f.to_be_bytes());
        }
        Value::U16(n) => uint(5, u64::from(*n), out),
        Value::U32(n) => uint(6, u64::from(*n), out),
        Value::U64(n) => uint(9, *n, out),
        Value::Map(pairs) => {
            control(7, pairs.len(), out);
            for (key, value) in pairs {
                encode(&Value::Text(key.clone()), out);
                encode(value, out);
            }
        }
        Value::Array(items) => {
            control(11, items.len(), out);
            for item in items {
                encode(item, out);
            }
        }
    }
}

fn uint(type_number: u8, n: u64, out: &mut Vec<u8>) {
    let bytes = n.to_be_bytes();
    let skip = bytes.iter().take_while(|&&b| b == 0).count();
    control(type_number, 8 - skip, out);
    out.extend_from_slice(&bytes[skip..]);
}

/// A control byte (plus the extended type byte and size bytes).
fn control(type_number: u8, size: usize, out: &mut Vec<u8>) {
    let (size_bits, extra): (u8, Vec<u8>) = match size {
        0..29 => (size as u8, vec![]),
        29..285 => (29, vec![(size - 29) as u8]),
        285..65821 => (30, ((size - 285) as u16).to_be_bytes().to_vec()),
        _ => (31, ((size - 65821) as u32).to_be_bytes()[1..].to_vec()),
    };
    if type_number <= 7 {
        out.push(type_number << 5 | size_bits);
    } else {
        out.push(size_bits);
        out.push(type_number - 7);
    }
    out.extend(extra);
}
