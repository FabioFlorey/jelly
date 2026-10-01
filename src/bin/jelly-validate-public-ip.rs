use std::{env, net::Ipv4Addr, process::ExitCode};

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, c, d] = ip.octets();

    // 192.0.0.0/24 is protocol-assignment space. Only the two
    // globally reachable anycast addresses are accepted from that block.
    if a == 192 && b == 0 && c == 0 {
        return matches!(d, 9 | 10);
    }

    !(a == 0
        || a == 10
        || a == 127
        || a >= 224
        || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
        || (a == 192 && b == 0 && c == 2)
        || (a == 192 && b == 88 && c == 99)
        || (a == 198 && (b == 18 || b == 19))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113))
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(value) = args.next() else {
        eprintln!("usage: jelly-validate-public-ip <ipv4>");
        return ExitCode::from(2);
    };
    if args.next().is_some() {
        eprintln!("usage: jelly-validate-public-ip <ipv4>");
        return ExitCode::from(2);
    }

    let Ok(ip) = value.parse::<Ipv4Addr>() else {
        return ExitCode::FAILURE;
    };

    if is_public_ipv4(ip) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn public(value: &str) -> bool {
        is_public_ipv4(value.parse().unwrap())
    }

    #[test]
    fn accepts_public_unicast_addresses() {
        assert!(public("1.1.1.1"));
        assert!(public("8.8.8.8"));
        assert!(public("93.184.216.34"));
    }

    #[test]
    fn rejects_non_public_ranges() {
        for value in [
            "0.0.0.0",
            "10.0.0.1",
            "100.64.0.1",
            "127.0.0.1",
            "169.254.1.1",
            "172.16.0.1",
            "192.168.1.1",
            "192.0.2.1",
            "192.88.99.1",
            "198.18.0.1",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.1",
            "255.255.255.255",
        ] {
            assert!(!public(value), "{value} must not be treated as public");
        }
    }

    #[test]
    fn handles_protocol_assignment_exceptions() {
        assert!(public("192.0.0.9"));
        assert!(public("192.0.0.10"));
        assert!(!public("192.0.0.8"));
        assert!(!public("192.0.0.11"));
    }
}
