use std::{io, net::SocketAddr};

use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;
use winapi::shared::ws2def::{SOL_SOCKET, SO_EXCLUSIVEADDRUSE};

/// Binds an ICE UDP socket with exclusive address ownership on Windows.
///
/// Without this option Windows permits a wildcard and interface-specific socket
/// to own the same numeric port. Independent WebRTC processes can then steal
/// consent/media packets from one another. Set the option before bind so a busy
/// port is skipped by ICE's existing port-range allocator instead.
pub(super) fn bind(addr: SocketAddr) -> io::Result<UdpSocket> {
    let domain = if addr.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    };
    let socket = Socket::new(domain, Type::DGRAM, Some(Protocol::UDP))?;
    let enabled: i32 = 1;
    unsafe {
        let result = winapi::um::winsock2::setsockopt(
            socket.as_raw_socket() as usize,
            SOL_SOCKET,
            SO_EXCLUSIVEADDRUSE,
            (&enabled as *const i32).cast(),
            std::mem::size_of_val(&enabled) as i32,
        );
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    socket.set_nonblocking(true)?;
    socket.bind(&addr.into())?;
    UdpSocket::from_std(socket.into())
}

use std::os::windows::io::AsRawSocket;

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, UdpSocket as StdUdpSocket};

    #[tokio::test]
    async fn overlapping_specific_and_wildcard_bind_is_rejected() {
        let owner = bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let port = owner.local_addr().unwrap().port();
        assert!(bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, port))).is_err());
        drop(owner);
        assert!(bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, port))).is_ok());
    }

    #[tokio::test]
    async fn exclusive_socket_still_sends_and_receives_udp() {
        let owner = bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let port = owner.local_addr().unwrap().port();
        let sender = StdUdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        sender
            .send_to(b"exclusive-live", (Ipv4Addr::LOCALHOST, port))
            .unwrap();
        let mut bytes = [0_u8; 32];
        let (size, _) = owner.recv_from(&mut bytes).await.unwrap();
        assert_eq!(&bytes[..size], b"exclusive-live");
    }
}
