//! Exercise the actual ICE network bind implementation, not a socket mock.
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;
use tokio::time::timeout;
use webrtc::util::vnet::net::Net;

#[tokio::test]
async fn windows_udp_ownership_rejects_wildcard_overlap_in_both_orders() {
    for wildcard_first in [false, true] {
        let net = Net::new(None);
        let first_ip = if wildcard_first {
            Ipv4Addr::UNSPECIFIED
        } else {
            Ipv4Addr::LOCALHOST
        };
        let second_ip = if wildcard_first {
            Ipv4Addr::LOCALHOST
        } else {
            Ipv4Addr::UNSPECIFIED
        };
        let owner = net.bind(SocketAddr::from((first_ip, 0))).await.unwrap();
        let port = owner.local_addr().unwrap().port();
        let overlap = net.bind(SocketAddr::from((second_ip, port))).await;
        assert!(
            overlap.is_err(),
            "a live ICE socket must reject wildcard/specific overlap (wildcard_first={wildcard_first})"
        );

        // Rejecting a competing bind must not close or break the original socket.
        let sender = tokio::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        sender
            .send_to(b"owner-still-live", (Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap();
        let mut bytes = [0_u8; 64];
        let (n, _) = timeout(Duration::from_secs(2), owner.recv_from(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&bytes[..n], b"owner-still-live");
        owner.close().await.unwrap();
        drop(owner);
        let replacement = net.bind(SocketAddr::from((second_ip, port))).await.unwrap();
        replacement.close().await.unwrap();
    }
}

#[tokio::test]
async fn windows_udp_ownership_allows_independent_device_sockets() {
    let net = Net::new(None);
    let pc = net
        .bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))
        .await
        .unwrap();
    let hp = net
        .bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))
        .await
        .unwrap();
    assert_ne!(
        pc.local_addr().unwrap().port(),
        hp.local_addr().unwrap().port()
    );
    pc.close().await.unwrap();
    hp.close().await.unwrap();
}
