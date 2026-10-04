use std::net::{IpAddr, SocketAddr, TcpStream};

use anyhow::{Result, ensure};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConnectionScope {
    #[default]
    Loopback,
    TrustedPrivate,
}

impl ConnectionScope {
    pub fn validate_bind(self, address: SocketAddr) -> Result<()> {
        self.validate(address, true)
    }

    pub fn validate_destination(self, address: SocketAddr) -> Result<()> {
        self.validate(address, false)
    }

    pub fn validate_connection(self, stream: &TcpStream) -> Result<()> {
        self.validate_destination(stream.local_addr()?)?;
        self.validate_destination(stream.peer_addr()?)
    }

    fn validate(self, address: SocketAddr, ephemeral: bool) -> Result<()> {
        ensure!(
            ephemeral || address.port() != 0,
            "netplay port must be nonzero"
        );
        if let SocketAddr::V6(address) = address {
            ensure!(
                address.scope_id() == 0 && address.flowinfo() == 0,
                "netplay address must not have a scope or flow label"
            );
        }
        let allowed = address.ip().is_loopback()
            || (self == Self::TrustedPrivate && private_address(address.ip()));
        ensure!(
            allowed,
            "netplay endpoint is outside the selected connection scope"
        );
        Ok(())
    }
}

fn private_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            let [first, second, _, _] = address.octets();
            address.is_private() || (first == 100 && (64..=127).contains(&second))
        }
        IpAddr::V6(address) => address.octets()[0] & 0xfe == 0xfc,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trusted_private_scope_has_explicit_range_boundaries() {
        for ip in [
            "127.0.0.1",
            "10.0.0.0",
            "10.255.255.255",
            "172.16.0.0",
            "172.31.255.255",
            "192.168.0.0",
            "192.168.255.255",
            "100.64.0.0",
            "100.127.255.255",
            "::1",
            "fc00::",
            "fdff:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
        ] {
            let address = SocketAddr::new(ip.parse().unwrap(), 1234);
            ConnectionScope::TrustedPrivate
                .validate_destination(address)
                .unwrap();
            assert_eq!(
                ConnectionScope::Loopback
                    .validate_destination(address)
                    .is_ok(),
                address.ip().is_loopback()
            );
        }
        for ip in [
            "0.0.0.0",
            "9.255.255.255",
            "11.0.0.0",
            "172.15.255.255",
            "172.32.0.0",
            "192.167.255.255",
            "192.169.0.0",
            "100.63.255.255",
            "100.128.0.0",
            "169.254.1.1",
            "224.0.0.1",
            "255.255.255.255",
            "8.8.8.8",
            "::",
            "fbff::",
            "fe00::",
            "fe80::1",
            "ff02::1",
            "2001:db8::1",
            "::ffff:127.0.0.1",
            "::ffff:192.168.1.1",
        ] {
            let address = SocketAddr::new(ip.parse().unwrap(), 1234);
            assert!(
                ConnectionScope::TrustedPrivate
                    .validate_destination(address)
                    .is_err(),
                "{ip}"
            );
        }
    }

    #[test]
    fn ephemeral_bind_is_allowed_but_destination_and_scoped_ipv6_are_rejected() {
        let address = "127.0.0.1:0".parse().unwrap();
        ConnectionScope::Loopback.validate_bind(address).unwrap();
        assert!(
            ConnectionScope::Loopback
                .validate_destination(address)
                .is_err()
        );
        let address = SocketAddr::V6(std::net::SocketAddrV6::new(
            "fd00::1".parse().unwrap(),
            1234,
            0,
            1,
        ));
        assert!(
            ConnectionScope::TrustedPrivate
                .validate_bind(address)
                .is_err()
        );
        let address = SocketAddr::V6(std::net::SocketAddrV6::new(
            "fd00::1".parse().unwrap(),
            1234,
            1,
            0,
        ));
        assert!(
            ConnectionScope::TrustedPrivate
                .validate_bind(address)
                .is_err()
        );
    }
}
