/// Where the audience should point their phones.
///
/// The QR code is the one thing a whole room scans without reading it, so the
/// URL behind it must never come from an unchecked header. `public` wins when
/// it is configured; otherwise the Host is used only if it looks like a host.
pub fn audience_url(
    public: Option<&str>,
    host: Option<&str>,
    proto: Option<&str>,
    id: &str,
) -> String {
    if let Some(base) = public {
        return format!("{}/s/{id}", base.trim_end_matches('/'));
    }
    let host = host.filter(|h| is_host(h)).unwrap_or("localhost");
    let scheme = match proto.map(str::trim) {
        Some("https") => "https",
        Some("http") => "http",
        // Nothing said, so guess from the address. A laptop on a venue network
        // serves plain http, and a guess of https there produces a QR nobody in
        // the room can open.
        _ if is_local(host) => "http",
        _ => "https",
    };
    format!("{scheme}://{host}/s/{id}")
}

/// A host and optional port, and nothing else. Anything with a slash, space,
/// credential or control character is a header someone wrote by hand.
fn is_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 255
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'))
}

/// Addresses a phone reaches over plain http on the same network.
fn is_local(host: &str) -> bool {
    let name = host.rsplit_once(':').map_or(host, |(h, _)| h);
    let name = name.trim_start_matches('[').trim_end_matches(']');

    if name == "localhost" || name == "::1" || name.ends_with(".local") || name.ends_with(".lan") {
        return true;
    }
    // A bare name with no dot is a machine on the local network.
    if !name.contains('.') && !name.contains(':') {
        return true;
    }
    let octets: Vec<&str> = name.split('.').collect();
    if octets.len() != 4 {
        return false;
    }
    let Ok(first) = octets[0].parse::<u8>() else {
        return false;
    };
    let second = octets[1].parse::<u8>().ok();
    match (first, second) {
        (127, _) | (10, _) => true,
        (192, Some(168)) => true,
        (172, Some(n)) if (16..=31).contains(&n) => true,
        (169, Some(254)) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_configured_public_url_wins() {
        let url = audience_url(
            Some("https://palmcast.example/"),
            Some("evil.test"),
            None,
            "abc",
        );
        assert_eq!(url, "https://palmcast.example/s/abc");
    }

    #[test]
    fn a_venue_laptop_gets_http_so_the_room_can_open_it() {
        for host in [
            "192.168.1.50:8080",
            "10.0.0.9:8080",
            "172.20.1.1",
            "kyles-laptop:8080",
            "studio.local",
        ] {
            let url = audience_url(None, Some(host), None, "abc");
            assert!(url.starts_with("http://"), "{host} produced {url}");
        }
    }

    #[test]
    fn a_public_host_gets_https() {
        let url = audience_url(None, Some("palmcast.example"), None, "abc");
        assert_eq!(url, "https://palmcast.example/s/abc");
    }

    #[test]
    fn a_proxy_header_is_believed_when_present() {
        let url = audience_url(None, Some("palmcast.example"), Some("http"), "abc");
        assert!(url.starts_with("http://"), "{url}");
    }

    #[test]
    fn a_hostile_host_header_cannot_reach_the_qr() {
        for host in [
            "evil.test/../../x",
            "evil.test\nX-Injected: 1",
            "user:pass@evil.test",
            "evil test",
            "",
        ] {
            let url = audience_url(None, Some(host), None, "abc");
            assert_eq!(
                url, "http://localhost/s/abc",
                "a crafted host survived: {host}"
            );
        }
    }

    #[test]
    fn a_plain_unknown_host_still_works() {
        let url = audience_url(None, Some("palmcast.example:8443"), None, "abc");
        assert_eq!(url, "https://palmcast.example:8443/s/abc");
    }
}
