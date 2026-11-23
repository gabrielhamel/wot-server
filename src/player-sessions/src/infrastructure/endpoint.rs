use std::{
    io,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket},
};

pub(super) fn advertised_base(
    configured: Option<SocketAddrV4>,
    bound: SocketAddr,
    client: SocketAddr,
) -> io::Result<SocketAddrV4> {
    let SocketAddr::V4(bound) = bound else {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "BaseApp requires IPv4",
        ));
    };
    if client.ip().is_loopback() && (bound.ip().is_unspecified() || bound.ip().is_loopback()) {
        return Ok(SocketAddrV4::new(Ipv4Addr::LOCALHOST, bound.port()));
    }
    if let Some(address) = configured {
        return Ok(address);
    }
    if !bound.ip().is_unspecified() {
        return Ok(bound);
    }
    let route = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    route.connect(client)?;
    let SocketAddr::V4(local) = route.local_addr()? else {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Client route requires IPv4",
        ));
    };
    Ok(SocketAddrV4::new(*local.ip(), bound.port()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_listener_advertises_a_reachable_interface_and_actual_port() {
        let bound = "0.0.0.0:20017".parse().unwrap();
        let client = "127.0.0.1:41000".parse().unwrap();
        assert_eq!(
            advertised_base(None, bound, client).unwrap(),
            "127.0.0.1:20017".parse().unwrap()
        );
    }

    #[test]
    fn explicit_destination_and_concrete_listener_take_precedence_over_routing() {
        let bound = "192.0.2.1:41000".parse().unwrap();
        let client = "127.0.0.1:42000".parse().unwrap();
        let external = "198.51.100.1:20017".parse().unwrap();
        assert_eq!(
            advertised_base(Some(external), bound, client).unwrap(),
            external
        );
        assert_eq!(
            SocketAddr::V4(advertised_base(None, bound, client).unwrap()),
            bound
        );
    }

    #[test]
    fn local_clients_bypass_the_public_destination_when_loopback_is_bound() {
        let external = "198.51.100.1:30017".parse().unwrap();
        let client = "127.0.0.1:42000".parse().unwrap();
        for bound in ["0.0.0.0:41000", "127.0.0.1:41000"] {
            assert_eq!(
                advertised_base(Some(external), bound.parse().unwrap(), client).unwrap(),
                "127.0.0.1:41000".parse().unwrap()
            );
        }
    }

    #[test]
    fn remote_clients_keep_the_public_destination() {
        let external = "198.51.100.1:30017".parse().unwrap();
        for client in ["192.168.1.42:42000", "203.0.113.42:42000"] {
            assert_eq!(
                advertised_base(
                    Some(external),
                    "0.0.0.0:41000".parse().unwrap(),
                    client.parse().unwrap()
                )
                .unwrap(),
                external
            );
        }
    }
}
