//! The address a request comes from. `X-Forwarded-For` is honored only when the TCP peer is
//! a configured trusted proxy: the header is plain request data, so from any other peer it
//! could name whatever address the sender likes. Spring's `forward-headers-strategy:
//! framework` takes its first entry from anyone.

use std::net::{IpAddr, SocketAddr};

use axum::extract::ConnectInfo;
use axum::http::request::Parts;
use ipnet::IpNet;

/// Reverse proxies whose `X-Forwarded-For` is trusted, as addresses and CIDR ranges.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrustedProxies(Vec<IpNet>);

/// Loopback and the private ranges: where a reverse proxy on the same host, in a container
/// network or on the LAN connects from. A peer on a public address is never among them.
pub const DEFAULT_TRUSTED_PROXIES: [&str; 6] = [
    "127.0.0.0/8",
    "::1/128",
    "10.0.0.0/8",
    "172.16.0.0/12",
    "192.168.0.0/16",
    "fc00::/7",
];

impl TrustedProxies {
    /// The list used when none is configured.
    pub fn private_networks() -> Self {
        Self(
            DEFAULT_TRUSTED_PROXIES
                .iter()
                .map(|net| net.parse().expect("valid CIDR literal"))
                .collect(),
        )
    }

    pub fn parse(entries: &[String]) -> anyhow::Result<Self> {
        entries
            .iter()
            .map(|entry| {
                entry
                    .parse::<IpNet>()
                    .or_else(|_| entry.parse::<IpAddr>().map(IpNet::from))
                    .map_err(|_| {
                        anyhow::anyhow!(
                            "server.trusted-proxies: {entry:?} is not an IP address or CIDR range"
                        )
                    })
            })
            .collect::<anyhow::Result<Vec<_>>>()
            .map(Self)
    }

    fn contains(&self, ip: IpAddr) -> bool {
        self.0.iter().any(|net| net.contains(&ip))
    }
}

/// The TCP peer, or behind trusted proxies the nearest `X-Forwarded-For` entry that is not
/// one of them: each proxy appends the address it received the request from, so only the
/// entries added by trusted proxies are reliable and anything left of them is not.
pub fn client_ip(parts: &Parts, trusted: &TrustedProxies) -> Option<IpAddr> {
    let peer = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()?
        .0
        .ip()
        .to_canonical();
    if !trusted.contains(peer) {
        if parts.headers.contains_key("X-Forwarded-For") {
            warn_ignored_once(peer);
        }
        return Some(peer);
    }
    let mut client = peer;
    let forwarded = parts
        .headers
        .get_all("X-Forwarded-For")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','));
    for entry in forwarded.rev() {
        let Some(ip) = parse_forwarded_entry(entry) else {
            break;
        };
        client = ip;
        if !trusted.contains(ip) {
            break;
        }
    }
    Some(client)
}

/// A reverse proxy that is not listed makes every client appear under the proxy's address,
/// which is easy to miss. Logged once per process: any client can send the header.
fn warn_ignored_once(peer: IpAddr) {
    static WARNED: std::sync::Once = std::sync::Once::new();
    WARNED.call_once(|| {
        tracing::warn!(
            "X-Forwarded-For from {peer} ignored: the sender is not in server.trusted-proxies, so its own address is used as the client address. If {peer} is your reverse proxy, add it to server.trusted-proxies"
        );
    });
}

/// An `X-Forwarded-For` entry: an address, optionally with a port (`1.2.3.4:5678`,
/// `[::1]:5678`) or in brackets.
fn parse_forwarded_entry(entry: &str) -> Option<IpAddr> {
    let entry = entry.trim();
    entry
        .parse::<IpAddr>()
        .ok()
        .or_else(|| entry.parse::<SocketAddr>().ok().map(|addr| addr.ip()))
        .or_else(|| {
            entry
                .strip_prefix('[')?
                .strip_suffix(']')?
                .parse::<IpAddr>()
                .ok()
        })
        .map(|ip| ip.to_canonical())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(peer: Option<&str>, forwarded: &[&str]) -> Parts {
        let mut request = axum::http::Request::builder();
        for value in forwarded {
            request = request.header("X-Forwarded-For", *value);
        }
        let (mut parts, ()) = request.body(()).unwrap().into_parts();
        if let Some(peer) = peer {
            let addr = SocketAddr::new(peer.parse().unwrap(), 40000);
            parts.extensions.insert(ConnectInfo(addr));
        }
        parts
    }

    fn trusted(entries: &[&str]) -> TrustedProxies {
        let entries: Vec<String> = entries.iter().map(|e| e.to_string()).collect();
        TrustedProxies::parse(&entries).unwrap()
    }

    fn resolve(peer: &str, forwarded: &[&str], proxies: &[&str]) -> String {
        client_ip(&parts(Some(peer), forwarded), &trusted(proxies))
            .unwrap()
            .to_string()
    }

    #[test]
    fn forwarded_header_is_ignored_from_an_untrusted_peer() {
        assert_eq!(
            resolve("203.0.113.9", &["198.51.100.1"], &[]),
            "203.0.113.9"
        );
        assert_eq!(
            resolve("203.0.113.9", &["198.51.100.1"], &["10.0.0.0/8"]),
            "203.0.113.9"
        );
    }

    #[test]
    fn trusted_peer_without_the_header_is_the_client() {
        assert_eq!(resolve("10.0.0.2", &[], &["10.0.0.0/8"]), "10.0.0.2");
    }

    #[test]
    fn nearest_untrusted_entry_wins_over_client_supplied_ones() {
        // the client sent "1.1.1.1" itself; the proxy appended the address it saw
        assert_eq!(
            resolve("10.0.0.2", &["1.1.1.1, 198.51.100.7"], &["10.0.0.0/8"]),
            "198.51.100.7"
        );
        // two proxies in a row, the outer one listed by address
        assert_eq!(
            resolve(
                "10.0.0.2",
                &["1.1.1.1, 198.51.100.7, 192.0.2.4"],
                &["10.0.0.0/8", "192.0.2.4"]
            ),
            "198.51.100.7"
        );
        // repeated header lines read as one list
        assert_eq!(
            resolve(
                "10.0.0.2",
                &["1.1.1.1", "198.51.100.7, 10.0.0.3"],
                &["10.0.0.0/8"]
            ),
            "198.51.100.7"
        );
    }

    #[test]
    fn all_trusted_entries_fall_back_to_the_first() {
        assert_eq!(
            resolve("10.0.0.2", &["10.0.0.9, 10.0.0.3"], &["10.0.0.0/8"]),
            "10.0.0.9"
        );
    }

    #[test]
    fn an_entry_that_is_not_an_address_stops_the_walk() {
        assert_eq!(
            resolve(
                "10.0.0.2",
                &["198.51.100.7, unknown, 10.0.0.3"],
                &["10.0.0.0/8"]
            ),
            "10.0.0.3"
        );
        assert_eq!(
            resolve("10.0.0.2", &["unknown"], &["10.0.0.0/8"]),
            "10.0.0.2"
        );
    }

    #[test]
    fn entries_with_ports_and_brackets_are_read() {
        assert_eq!(
            resolve("10.0.0.2", &["198.51.100.7:4711"], &["10.0.0.0/8"]),
            "198.51.100.7"
        );
        assert_eq!(
            resolve("10.0.0.2", &["[2001:db8::7]:4711"], &["10.0.0.0/8"]),
            "2001:db8::7"
        );
        assert_eq!(
            resolve("10.0.0.2", &["[2001:db8::7]"], &["10.0.0.0/8"]),
            "2001:db8::7"
        );
    }

    #[test]
    fn ipv4_mapped_addresses_match_ipv4_entries() {
        assert_eq!(
            resolve("::ffff:127.0.0.1", &["::ffff:198.51.100.7"], &["127.0.0.1"]),
            "198.51.100.7"
        );
    }

    #[test]
    fn ipv6_ranges_are_matched() {
        assert_eq!(
            resolve("fd00::2", &["2001:db8::7"], &["fd00::/8"]),
            "2001:db8::7"
        );
    }

    #[test]
    fn private_networks_trust_local_proxies_but_no_public_peer() {
        let default = TrustedProxies::private_networks();
        for (peer, expected) in [
            ("127.0.0.1", "198.51.100.7"),
            ("172.18.0.2", "198.51.100.7"),
            ("192.168.1.5", "198.51.100.7"),
            ("fd00::2", "198.51.100.7"),
            // a public peer names itself, whatever it claims
            ("203.0.113.9", "203.0.113.9"),
            ("172.32.0.1", "172.32.0.1"),
        ] {
            let resolved = client_ip(&parts(Some(peer), &["198.51.100.7"]), &default);
            assert_eq!(resolved.unwrap().to_string(), expected, "{peer}");
        }
    }

    #[test]
    fn no_peer_address_means_no_client_address() {
        assert_eq!(
            client_ip(&parts(None, &["198.51.100.7"]), &trusted(&["10.0.0.0/8"])),
            None
        );
    }

    #[test]
    fn invalid_entries_are_rejected() {
        for entry in ["proxy.local", "10.0.0.0/33", "10.0.0.1/", ""] {
            let error = TrustedProxies::parse(&[entry.to_string()]).unwrap_err();
            assert!(
                error.to_string().contains("server.trusted-proxies"),
                "{entry}"
            );
        }
    }
}
