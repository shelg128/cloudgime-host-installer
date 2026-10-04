//! Regression model for transport access coordination. Production wiring is
//! changed only after live lock-duration evidence confirms the shared-mutex path.
#[cfg(test)] mod tests {
    use std::sync::Arc;
    use tokio::sync::{Mutex,RwLock,oneshot};
    use std::time::Duration;
    #[tokio::test] async fn legacy_shared_mutex_blocks_video_behind_control_send(){
        let slot=Arc::new(Mutex::new(Some(1u8)));let(started,ready)=oneshot::channel();let(release,stop)=oneshot::channel();
        let sender=tokio::spawn({let slot=slot.clone();async move{let _guard=slot.lock().await;started.send(()).unwrap();stop.await.unwrap();}});
        ready.await.unwrap();assert!(tokio::time::timeout(Duration::from_millis(20),slot.lock()).await.is_err());release.send(()).unwrap();sender.await.unwrap();
    }
    #[tokio::test] async fn read_access_keeps_video_independent_of_control_send(){
        let slot=Arc::new(RwLock::new(Some(1u8)));let(started,ready)=oneshot::channel();let(release,stop)=oneshot::channel();
        let sender=tokio::spawn({let slot=slot.clone();async move{let _guard=slot.read().await;started.send(()).unwrap();stop.await.unwrap();}});
        ready.await.unwrap();let video=tokio::time::timeout(Duration::from_millis(100),slot.read()).await.unwrap();assert_eq!(*video,Some(1));drop(video);release.send(()).unwrap();sender.await.unwrap();
    }
    #[tokio::test] async fn replace_and_stop_wait_for_inflight_reads(){
        let slot=Arc::new(RwLock::new(Some(1u8)));let reader=slot.read().await;
        assert!(tokio::time::timeout(Duration::from_millis(20),slot.write()).await.is_err());drop(reader);
        let mut exclusive=slot.write().await;assert_eq!(exclusive.replace(2),Some(1));assert!(tokio::time::timeout(Duration::from_millis(20),slot.read()).await.is_err());assert_eq!(exclusive.take(),Some(2));drop(exclusive);assert_eq!(*slot.read().await,None);
    }
}
