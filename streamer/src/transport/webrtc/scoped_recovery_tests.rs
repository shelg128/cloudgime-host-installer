#[path = "../../../../vendor/webrtc-util/src/conn/scoped_recovery.rs"]
mod budget_unit_tests;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{net::UdpSocket, time::timeout};
use webrtc::util::Conn;
use webrtc_util::conn::scoped_recovery::{CopyBudget, CopyScope, within};
fn packet() -> Vec<u8> {
    let mut p = vec![0x55; 100];
    p[0] = 0x80;
    p[1] = 120;
    p[2..4].copy_from_slice(&9u16.to_be_bytes());
    p[8..12].copy_from_slice(&12345u32.to_be_bytes());
    p
}
#[cfg(not(feature = "cloudgime-total-egress-qa"))]
#[tokio::test]
async fn scoped_socket_copy_requires_exact_write_and_unbind_revokes() {
    let tx = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let rx = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let target = rx.local_addr().unwrap();
    let port = tx.local_addr().unwrap().port();
    let data = packet();
    let mut buffer = [0u8; 1500];
    Conn::send_to(&tx, &data, target).await.unwrap();
    let (n, _) = rx.recv_from(&mut buffer).await.unwrap();
    assert_eq!(&buffer[..n], data.as_slice());
    assert!(
        timeout(Duration::from_millis(20), rx.recv_from(&mut buffer))
            .await
            .is_err()
    );
    let budget = CopyBudget::new(Duration::from_secs(20), 2000, 1200).unwrap();
    budget.activate(Duration::from_secs(15));
    tokio::time::sleep(Duration::from_millis(20)).await;
    let scope = CopyScope::new(target, port, 12345, 120, 9, budget.clone()).unwrap();
    within(scope, Conn::send_to(&tx, &data, target))
        .await
        .unwrap();
    for _ in 0..2 {
        let (n, _) = timeout(Duration::from_secs(1), rx.recv_from(&mut buffer))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&buffer[..n], data.as_slice());
    }
    assert!(
        timeout(Duration::from_millis(20), rx.recv_from(&mut buffer))
            .await
            .is_err()
    );
    assert_eq!(budget.snapshot().copies, 1);
    let live = Arc::new(AtomicBool::new(true));
    let scope =
        CopyScope::with_binding(target, port, 12345, 120, 9, budget.clone(), live.clone()).unwrap();
    live.store(false, Ordering::Release);
    within(scope, Conn::send_to(&tx, &data, target))
        .await
        .unwrap();
    rx.recv_from(&mut buffer).await.unwrap();
    assert!(
        timeout(Duration::from_millis(20), rx.recv_from(&mut buffer))
            .await
            .is_err()
    );
    assert_eq!(budget.snapshot().copies, 1);
    budget.revoke();
    let scope = CopyScope::new(target, port, 12345, 120, 9, budget.clone()).unwrap();
    within(scope, Conn::send_to(&tx, &data, target))
        .await
        .unwrap();
    rx.recv_from(&mut buffer).await.unwrap();
    assert!(
        timeout(Duration::from_millis(20), rx.recv_from(&mut buffer))
            .await
            .is_err()
    );
}
#[cfg(not(any(
    feature = "cloudgime-recovery-delay-qa",
    feature = "cloudgime-total-egress-qa",
    feature = "cloudgime-frame-tail-copy-qa"
)))]
#[tokio::test]
async fn share_limited_socket_preserves_originals_and_caps_protected_copies() {
    let tx = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let rx = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let target = rx.local_addr().unwrap();
    let port = tx.local_addr().unwrap().port();
    let budget = CopyBudget::new_with_share(Duration::from_secs(20), 2000, 1200, 25).unwrap();
    budget.activate(Duration::from_secs(15));
    tokio::time::sleep(Duration::from_millis(20)).await;
    let data = packet();
    let mut buffer = [0u8; 1500];
    for n in 0..8 {
        let mut original = data.clone();
        original[2..4].copy_from_slice(&(n as u16).to_be_bytes());
        let scope = CopyScope::new(target, port, 12345, 120, n, budget.clone()).unwrap();
        assert_eq!(
            within(scope, Conn::send_to(&tx, &original, target))
                .await
                .unwrap(),
            original.len()
        );
        let (size, _) = timeout(Duration::from_secs(1), rx.recv_from(&mut buffer))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&buffer[..size], &original);
        if n % 4 == 3 {
            let (size, _) = timeout(Duration::from_secs(1), rx.recv_from(&mut buffer))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(&buffer[..size], &original);
        } else {
            assert!(
                timeout(Duration::from_millis(2), rx.recv_from(&mut buffer))
                    .await
                    .is_err()
            );
        }
    }
    let s = budget.snapshot();
    assert_eq!(s.originals, 8);
    assert_eq!(s.copies, 2);
    assert_eq!(s.extra_bytes * 4, s.original_bytes);
    budget.revoke();
}
#[cfg(feature = "cloudgime-recovery-delay-qa")]
#[tokio::test]
async fn delayed_real_socket_sends_original_now_then_exact_older_copy() {
    let tx = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let rx = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let target = rx.local_addr().unwrap();
    let port = tx.local_addr().unwrap().port();
    let b = CopyBudget::new_with_share(Duration::from_secs(20), 2000, 1200, 25).unwrap();
    b.activate(Duration::from_secs(15));
    tokio::time::sleep(Duration::from_millis(20)).await;
    let mut buffer = [0u8; 1500];
    let mut last = vec![];
    for seq in 0..4u16 {
        let mut p = packet();
        p[2..4].copy_from_slice(&seq.to_be_bytes());
        within(
            CopyScope::new(target, port, 12345, 120, seq, b.clone()).unwrap(),
            Conn::send_to(&tx, &p, target),
        )
        .await
        .unwrap();
        let (n, _) = rx.recv_from(&mut buffer).await.unwrap();
        assert_eq!(&buffer[..n], &p);
        last = p;
    }
    assert_eq!(b.snapshot().copies, 0);
    tokio::time::sleep(Duration::from_millis(4)).await;
    let mut next = packet();
    next[2..4].copy_from_slice(&4u16.to_be_bytes());
    within(
        CopyScope::new(target, port, 12345, 120, 4, b.clone()).unwrap(),
        Conn::send_to(&tx, &next, target),
    )
    .await
    .unwrap();
    let (n, _) = rx.recv_from(&mut buffer).await.unwrap();
    assert_eq!(&buffer[..n], &next);
    let (n, _) = rx.recv_from(&mut buffer).await.unwrap();
    assert_eq!(&buffer[..n], &last);
    assert_eq!(b.snapshot().copies, 1);
    assert!(b.snapshot().extra_bytes * 4 <= b.snapshot().original_bytes);
    b.revoke();
}
#[cfg(feature = "cloudgime-total-egress-qa")]
#[path = "../../../../vendor/webrtc-util/src/conn/media_egress_qa.rs"]
mod media_ledger_tests;
#[cfg(feature = "cloudgime-total-egress-qa")]
#[tokio::test]
async fn total_egress_socket_preserves_retry_and_yields_optional_copy() {
    use webrtc_util::conn::media_egress_qa::{self, MediaLedger, WriteScope};
    let tx = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let rx = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let ep = rx.local_addr().unwrap();
    let port = tx.local_addr().unwrap().port();
    let ledger = MediaLedger::new();
    let live = Arc::new(AtomicBool::new(true));
    let b = CopyBudget::new_with_share(Duration::from_secs(20), 2000, 1200, 25).unwrap();
    b.activate(Duration::from_secs(15));
    tokio::time::sleep(Duration::from_millis(20)).await;
    let mut buf = [0u8; 1500];
    for seq in 0..8u16 {
        let mut data = packet();
        data[2..4].copy_from_slice(&seq.to_be_bytes());
        let original = within(
            CopyScope::new(ep, port, 12345, 120, seq, b.clone()).unwrap(),
            Conn::send_to(&tx, &data, ep),
        );
        media_egress_qa::within(
            WriteScope::new(ledger.clone(), live.clone(), 12345, 120, seq, true),
            original,
        )
        .await
        .unwrap();
        let (n, _) = rx.recv_from(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], &data);
        if seq == 7 {
            let (n, _) = rx.recv_from(&mut buf).await.unwrap();
            assert_eq!(&buf[..n], &data);
        }
        if seq == 2 {
            media_egress_qa::within(
                WriteScope::new(ledger.clone(), live.clone(), 12345, 120, seq, false),
                Conn::send_to(&tx, &data, ep),
            )
            .await
            .unwrap();
            let (n, _) = rx.recv_from(&mut buf).await.unwrap();
            assert_eq!(&buf[..n], &data);
        }
    }
    let s = ledger.snapshot();
    assert_eq!(s.originals, 8);
    assert_eq!(s.retries, 1);
    assert_eq!(s.copies, 1);
    assert_eq!(s.original_bytes, 1024);
    assert_eq!(s.retry_bytes + s.copy_bytes, 256);
    assert!(
        timeout(Duration::from_millis(5), rx.recv_from(&mut buf))
            .await
            .is_err()
    );
    b.revoke();
    ledger.close();
}
#[cfg(feature = "cloudgime-frame-tail-copy-qa")]
#[tokio::test]
async fn tail_socket_keeps_original_order_and_exact_protected_end_packet() {
    let tx = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let rx = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let target = rx.local_addr().unwrap();
    let port = tx.local_addr().unwrap().port();
    let b = CopyBudget::new_with_share(Duration::from_secs(20), 2000, 1200, 25).unwrap();
    b.activate(Duration::from_secs(10));
    tokio::time::sleep(Duration::from_millis(20)).await;
    let mut buf = [0u8; 1500];
    for seq in 0..5u16 {
        let mut p = vec![7u8; 1172];
        p[0] = 0x80;
        p[1] = 120;
        if seq == 4 {
            p[1] |= 0x80;
        }
        p[2..4].copy_from_slice(&seq.to_be_bytes());
        p[8..12].copy_from_slice(&12345u32.to_be_bytes());
        let scope = CopyScope::new(target, port, 12345, 120, seq, b.clone()).unwrap();
        assert_eq!(
            within(scope, Conn::send_to(&tx, &p, target)).await.unwrap(),
            p.len()
        );
        let (n, _) = timeout(Duration::from_secs(1), rx.recv_from(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&buf[..n], &p);
        if seq == 4 {
            let (n, _) = timeout(Duration::from_secs(1), rx.recv_from(&mut buf))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(&buf[..n], &p);
        } else {
            assert!(
                timeout(Duration::from_millis(2), rx.recv_from(&mut buf))
                    .await
                    .is_err()
            );
        }
    }
    let stats = b.snapshot();
    assert_eq!(stats.originals, 5);
    assert_eq!(stats.tail_copies, 1);
    assert_eq!(stats.body_copies, 0);
    assert!(stats.extra_bytes * 4 <= stats.original_bytes);
    let p = packet();
    Conn::send_to(&tx, &p, target).await.unwrap();
    let (n, _) = timeout(Duration::from_secs(1), rx.recv_from(&mut buf))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&buf[..n], &p);
    assert!(
        timeout(Duration::from_millis(3), rx.recv_from(&mut buf))
            .await
            .is_err()
    );
    assert_eq!(b.snapshot().copies, 1);
    b.revoke();
}
#[tokio::test]
async fn separate_peer_budgets_and_cancelled_context_do_not_leak() {
    let target = "127.0.0.1:12345".parse().unwrap();
    let one = CopyBudget::new(Duration::from_secs(20), 2000, 1200).unwrap();
    let two = CopyBudget::new(Duration::from_secs(20), 2000, 1200).unwrap();
    one.activate(Duration::from_secs(15));
    tokio::time::sleep(Duration::from_millis(20)).await;
    let data = packet();
    within(
        CopyScope::new(target, 99, 12345, 120, 9, one.clone()).unwrap(),
        async {
            assert!(webrtc_util::conn::scoped_recovery::permit(&data, target, 99).is_some());
        },
    )
    .await;
    within(
        CopyScope::new(target, 99, 12345, 120, 9, two).unwrap(),
        async {
            assert!(webrtc_util::conn::scoped_recovery::permit(&data, target, 99).is_none());
        },
    )
    .await;
    let scope = CopyScope::new(target, 99, 12345, 120, 9, one).unwrap();
    assert!(
        timeout(
            Duration::from_millis(2),
            within(scope, std::future::pending::<()>())
        )
        .await
        .is_err()
    );
    assert!(webrtc_util::conn::scoped_recovery::permit(&data, target, 99).is_none());
}
