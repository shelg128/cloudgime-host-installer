//! Single-consumer wakeup ordering. No frame retention or queue/drop policy change.
use std::{collections::VecDeque,sync::{Arc,Weak}};
use tokio::sync::{Mutex,Notify};
pub(super) async fn pop_or_wait<T>(queue:&Weak<Mutex<VecDeque<T>>>,wake:&Notify)->Option<T>{
    loop {
        let owner=queue.upgrade()?;
        let item=owner.lock().await.pop_back();
        drop(owner); // An idle consumer cannot keep its producer's queue alive.
        if item.is_some(){return item;}
        wake.notified().await;
    }
}
#[cfg(test)] mod tests {
    use super::*;use std::time::Duration;
    #[tokio::test] async fn reproduces_notify_waiters_loss_and_retained_permit_fix(){
        let legacy=Notify::new();legacy.notify_waiters();
        assert!(tokio::time::timeout(Duration::from_millis(15),legacy.notified()).await.is_err());
        let fixed=Notify::new();fixed.notify_one();
        assert!(tokio::time::timeout(Duration::from_millis(100),fixed.notified()).await.is_ok());
    }
    #[tokio::test] async fn fifo_and_frame_bytes_unchanged(){
        let q=Arc::new(Mutex::new(VecDeque::from([vec![3,4],vec![1,2]])));let n=Notify::new();
        assert_eq!(pop_or_wait(&Arc::downgrade(&q),&n).await,Some(vec![1,2]));
        assert_eq!(pop_or_wait(&Arc::downgrade(&q),&n).await,Some(vec![3,4]));
    }
    #[tokio::test] async fn waiting_consumer_releases_owner_and_exits_when_woken(){
        let q:Arc<Mutex<VecDeque<u8>>>=Arc::new(Mutex::new(VecDeque::new()));let weak=Arc::downgrade(&q);let n=Arc::new(Notify::new());
        let task=tokio::spawn({let n=n.clone();let w=weak.clone();async move{pop_or_wait(&w,&n).await}});
        tokio::task::yield_now().await;assert_eq!(Arc::strong_count(&q),1);drop(q);n.notify_one();
        assert_eq!(tokio::time::timeout(Duration::from_secs(1),task).await.unwrap().unwrap(),None);
    }
    #[tokio::test] async fn queued_frame_after_empty_pop_is_delivered(){
        let q=Arc::new(Mutex::new(VecDeque::new()));let n=Arc::new(Notify::new());
        let task=tokio::spawn({let n=n.clone();let w=Arc::downgrade(&q);async move{pop_or_wait(&w,&n).await}});
        tokio::task::yield_now().await;q.lock().await.push_front(77);n.notify_one();
        assert_eq!(tokio::time::timeout(Duration::from_secs(1),task).await.unwrap().unwrap(),Some(77));
    }
}
