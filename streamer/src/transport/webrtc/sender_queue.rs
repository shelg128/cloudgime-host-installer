//! Single-consumer media queue. A retained notify permit prevents a frame
//! arriving between an empty pop and waiting from sleeping until another frame.
use std::{
    collections::VecDeque,
    sync::{Arc, Weak},
    time::Instant,
};
use tokio::{
    sync::{Mutex, Notify},
    task::JoinHandle,
};

pub(super) struct QueuedFrame<T> {
    pub important: bool,
    pub samples: Vec<T>,
    pub enqueued_at: Instant,
}

pub(super) struct FrameQueue<T> {
    frames: Mutex<VecDeque<QueuedFrame<T>>>,
    pub wake: Arc<Notify>,
    capacity: usize,
}
impl<T> FrameQueue<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            frames: Mutex::new(VecDeque::new()),
            wake: Arc::new(Notify::new()),
            capacity: capacity.max(1),
        }
    }
    pub async fn push(&self, samples: Vec<T>, important: bool) -> bool {
        let mut frames = self.frames.lock().await;
        if important {
            // A new IDR is independently decodable; older queued IDRs/deltas
            // only add latency. Never drop part of an in-flight encoded frame.
            frames.clear();
        } else if frames.len() >= self.capacity {
            return false;
        }
        frames.push_back(QueuedFrame {
            samples,
            important,
            enqueued_at: Instant::now(),
        });
        drop(frames);
        self.wake.notify_one();
        true
    }
    pub async fn pop(&self) -> Option<QueuedFrame<T>> {
        self.frames.lock().await.pop_front()
    }
    pub async fn clear(&self, important: bool) {
        let mut frames = self.frames.lock().await;
        if important {
            frames.clear();
        } else {
            frames.retain(|f| f.important);
        }
    }
}

pub(super) async fn next_frame<T>(
    queue: &Weak<FrameQueue<T>>,
    wake: &Notify,
) -> Option<QueuedFrame<T>> {
    loop {
        let owner = queue.upgrade()?;
        let frame = owner.pop().await;
        drop(owner); // do not retain the queue while waiting for a producer
        if frame.is_some() {
            return frame;
        }
        wake.notified().await;
    }
}

/// Tasks cannot outlive their track owner. Replacement awaits cancellation
/// before starting another consumer of the same queue.
#[derive(Default)]
pub(super) struct SenderWorkers(Vec<JoinHandle<()>>);
impl SenderWorkers {
    pub fn push(&mut self, task: JoinHandle<()>) {
        self.0.push(task);
    }
    pub async fn stop(&mut self) {
        let tasks = std::mem::take(&mut self.0);
        for task in &tasks {
            task.abort();
        }
        for task in tasks {
            let _ = task.await;
        }
    }
}
impl Drop for SenderWorkers {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::sync::oneshot;
    #[tokio::test]
    async fn enqueue_between_empty_pop_and_wait_is_not_lost() {
        let q = Arc::new(FrameQueue::new(2));
        assert!(q.pop().await.is_none());
        q.push(vec![17], false).await;
        tokio::time::timeout(Duration::from_millis(100), q.wake.notified())
            .await
            .unwrap();
        assert_eq!(q.pop().await.unwrap().samples, [17]);
    }
    #[tokio::test]
    async fn deltas_stay_fifo_and_bound() {
        let q = Arc::new(FrameQueue::new(2));
        assert!(q.push(vec![1, 2], false).await);
        assert!(q.push(vec![3], false).await);
        assert!(!q.push(vec![4], false).await);
        assert_eq!(
            next_frame(&Arc::downgrade(&q), &q.wake)
                .await
                .unwrap()
                .samples,
            [1, 2]
        );
        assert_eq!(q.pop().await.unwrap().samples, [3]);
    }
    #[tokio::test]
    async fn repeated_keyframes_replace_stale_queue_not_grow_it() {
        let q = FrameQueue::new(2);
        for i in 0..1000 {
            assert!(q.push(vec![i], true).await);
        }
        assert_eq!(q.pop().await.unwrap().samples, [999]);
        assert!(q.pop().await.is_none());
    }
    #[tokio::test]
    async fn keyframe_replaces_queued_dependencies_atomically() {
        let q = FrameQueue::new(3);
        q.push(vec![1], true).await;
        q.push(vec![2], false).await;
        q.push(vec![3, 4], true).await;
        assert_eq!(q.pop().await.unwrap().samples, [3, 4]);
        assert!(q.pop().await.is_none());
    }
    #[tokio::test]
    async fn queue_clear_keeps_only_keyframes_when_requested() {
        let q = FrameQueue::new(3);
        q.push(vec![1], true).await;
        q.push(vec![2], false).await;
        q.clear(false).await;
        assert_eq!(q.pop().await.unwrap().samples, [1]);
        q.push(vec![3], true).await;
        q.clear(true).await;
        assert!(q.pop().await.is_none());
    }
    #[tokio::test]
    async fn missing_queue_exits_instead_of_spinning() {
        let q: Arc<FrameQueue<u8>> = Arc::new(FrameQueue::new(2));
        let weak = Arc::downgrade(&q);
        let wake = q.wake.clone();
        drop(q);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), next_frame(&weak, &wake))
                .await
                .unwrap()
                .is_none()
        );
    }
    struct Exit(Option<oneshot::Sender<()>>);
    impl Drop for Exit {
        fn drop(&mut self) {
            if let Some(tx) = self.0.take() {
                let _ = tx.send(());
            }
        }
    }
    async fn worker() -> (JoinHandle<()>, oneshot::Receiver<()>) {
        let (started_tx, started_rx) = oneshot::channel();
        let (exit_tx, exit_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            let _exit = Exit(Some(exit_tx));
            let _ = started_tx.send(());
            std::future::pending::<()>().await;
        });
        started_rx.await.unwrap();
        (task, exit_rx)
    }
    #[tokio::test]
    async fn owner_drop_cancels_sender_and_rtcp_workers() {
        let mut workers = SenderWorkers::default();
        let (a, ax) = worker().await;
        let (b, bx) = worker().await;
        workers.push(a);
        workers.push(b);
        drop(workers);
        tokio::time::timeout(Duration::from_millis(100), async {
            ax.await.unwrap();
            bx.await.unwrap();
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn replacement_waits_for_old_worker_exit() {
        let mut workers = SenderWorkers::default();
        let (a, ax) = worker().await;
        workers.push(a);
        workers.stop().await;
        ax.await.unwrap();
        assert!(workers.0.is_empty());
    }
}
