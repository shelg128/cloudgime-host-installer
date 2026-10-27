#[cfg(feature = "cloudgime-datagram-redundancy-qa")]
#[path = "../../../../vendor/webrtc-util/src/conn/protected_datagram.rs"]
mod protected_datagram;
use tokio::net::UdpSocket;
use tokio::time::{Duration, timeout};
use webrtc::util::Conn;
#[cfg(feature = "cloudgime-datagram-redundancy-qa")]
#[tokio::test]
async fn qa_redundancy_exact_ciphertext_and_control_isolation() {
    let sender = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let receiver = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let dst = receiver.local_addr().unwrap();
    let mut data = vec![0x55u8; 100];
    data[0] = 0x90;
    data[1] = 98;
    data[2] = 0x12;
    data[3] = 0x34;
    assert_eq!(
        Conn::send_to(&sender, &data, dst).await.unwrap(),
        data.len()
    );
    let mut b = [0u8; 1500];
    for _ in 0..2 {
        let (n, _) = timeout(Duration::from_secs(1), receiver.recv_from(&mut b))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&b[..n], data.as_slice());
    }
    assert!(
        timeout(Duration::from_millis(30), receiver.recv_from(&mut b))
            .await
            .is_err()
    );
    data[1] = 111;
    Conn::send_to(&sender, &data, dst).await.unwrap();
    let (n, _) = receiver.recv_from(&mut b).await.unwrap();
    assert_eq!(&b[..n], data.as_slice());
    assert!(
        timeout(Duration::from_millis(30), receiver.recv_from(&mut b))
            .await
            .is_err()
    );
}

#[cfg(not(feature = "cloudgime-datagram-redundancy-qa"))]
#[tokio::test]
async fn normal_build_never_duplicates_video_datagrams() {
    let sender = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let receiver = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let dst = receiver.local_addr().unwrap();
    let mut data = vec![0x55; 100];
    data[0] = 0x90;
    data[1] = 98;
    Conn::send_to(&sender, &data, dst).await.unwrap();
    let mut b = [0u8; 1500];
    let (n, _) = receiver.recv_from(&mut b).await.unwrap();
    assert_eq!(&b[..n], data.as_slice());
    assert!(
        timeout(Duration::from_millis(50), receiver.recv_from(&mut b))
            .await
            .is_err()
    );
}
