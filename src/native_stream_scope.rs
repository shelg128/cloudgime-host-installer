//! Native control cancellation ownership. This records only directly-owned child
//! handles and connection generations. It is NOT a rental receipt, Sunshine
//! server termination proof, or a claim-release authority. No global/PID scan.
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
    time::Duration,
};
use tokio::{
    process::Child,
    sync::{Mutex, MutexGuard, watch},
    time::timeout,
};

const CAPACITY: usize = 4096;
const STOP_TIMEOUT: Duration = Duration::from_secs(8);
#[derive(Default)]
pub(crate) struct NativeStreamRegistry {
    sessions: Mutex<HashMap<String, Entry>>,
}
#[derive(Default)]
struct Entry {
    revoked: bool,
    connections: HashMap<u64, Arc<NativeStreamScope>>,
}
pub(crate) static NATIVE_STREAM_SCOPES: LazyLock<NativeStreamRegistry> =
    LazyLock::new(NativeStreamRegistry::default);

pub(crate) struct NativeStreamScope {
    cancelled: watch::Sender<bool>,
    children: Mutex<Vec<Arc<Mutex<Child>>>>,
}
pub(crate) struct NativeLaunchPermit<'a> {
    scope: &'a NativeStreamScope,
    children: MutexGuard<'a, Vec<Arc<Mutex<Child>>>>,
}
impl NativeStreamRegistry {
    pub(crate) async fn begin(
        &self,
        session_id: &str,
        generation: u64,
    ) -> Result<Arc<NativeStreamScope>, &'static str> {
        if session_id.is_empty() || session_id.len() > 256 || generation == 0 {
            return Err("native_scope_binding_invalid");
        }
        let mut sessions = self.sessions.lock().await;
        // Tombstones must not be evicted while an old task can still register a
        // child; bound memory by refusing new sessions, never accepting a revoked ID.
        if !sessions.contains_key(session_id) && sessions.len() >= CAPACITY {
            return Err("native_scope_capacity_reached");
        }
        let entry = sessions.entry(session_id.into()).or_default();
        if entry.revoked {
            return Err("native_scope_revoked");
        }
        if entry.connections.len() >= 16 || entry.connections.contains_key(&generation) {
            return Err("native_connection_conflict");
        }
        let (cancelled, _) = watch::channel(false);
        let scope = Arc::new(NativeStreamScope {
            cancelled,
            children: Mutex::new(Vec::new()),
        });
        entry.connections.insert(generation, scope.clone());
        Ok(scope)
    }
    pub(crate) async fn stop(&self, session_id: &str) -> Result<usize, String> {
        if session_id.is_empty() || session_id.len() > 256 {
            return Err("native_scope_binding_invalid".into());
        }
        let scopes = {
            let mut sessions = self.sessions.lock().await;
            if !sessions.contains_key(session_id) && sessions.len() >= CAPACITY {
                return Err("native_scope_capacity_reached".into());
            }
            // Even a revoke arriving before begin creates a deny tombstone.
            let entry = sessions.entry(session_id.into()).or_default();
            entry.revoked = true;
            for scope in entry.connections.values() {
                scope.cancelled.send_replace(true);
            }
            entry.connections.values().cloned().collect::<Vec<_>>()
        };
        if scopes.is_empty() {
            return Err("native_scope_no_registered_connection".into());
        }
        let mut count = 0;
        for scope in scopes {
            count += timeout(STOP_TIMEOUT, scope.stop_children())
                .await
                .map_err(|_| "native_child_exit_not_verified".to_string())??;
        }
        Ok(count)
    }
}
impl NativeStreamScope {
    pub(crate) fn is_cancelled(&self) -> bool {
        *self.cancelled.borrow()
    }
    pub(crate) async fn cancelled(&self) {
        let mut signal = self.cancelled.subscribe();
        if *signal.borrow() {
            return;
        }
        while signal.changed().await.is_ok() {
            if *signal.borrow() {
                return;
            }
        }
    }
    // Caller holds this permit from BEFORE spawning until ownership is attached.
    // stop() marks cancellation first, then waits for the permit before inspecting
    // children. A cancelled launch cannot be reported stopped while unregistered.
    pub(crate) async fn stop_owned_children(&self) -> Result<usize, String> {
        self.cancelled.send_replace(true);
        timeout(STOP_TIMEOUT, self.stop_children())
            .await
            .map_err(|_| "native_child_exit_not_verified".to_string())?
    }

    pub(crate) async fn launch_permit(&self) -> Result<NativeLaunchPermit<'_>, &'static str> {
        let children = self.children.lock().await;
        if self.is_cancelled() {
            return Err("native_scope_revoked");
        }
        if children.len() >= 16 {
            return Err("native_scope_child_limit");
        }
        Ok(NativeLaunchPermit {
            scope: self,
            children,
        })
    }
    async fn stop_children(&self) -> Result<usize, String> {
        let children = self.children.lock().await;
        for child in children.iter() {
            let mut child = child.lock().await;
            match child.try_wait() {
                Ok(Some(_)) => {}
                Ok(None) => {
                    // tokio Child retains OS process handle; never taskkill an
                    // arbitrary PID or process-name list. Direct child proof only.
                    if let Err(e) = child.start_kill() {
                        if child
                            .try_wait()
                            .map_err(|_| "native_child_inspection_failed")?
                            .is_none()
                        {
                            return Err(format!("native_child_stop_failed:{:?}", e.kind()));
                        }
                    }
                    child.wait().await.map_err(|_| "native_child_wait_failed")?;
                }
                Err(_) => return Err("native_child_inspection_failed".into()),
            }
        }
        Ok(children.len())
    }
}
impl NativeLaunchPermit<'_> {
    pub(crate) fn attach(&mut self, child: Arc<Mutex<Child>>) -> bool {
        self.children.push(child);
        !self.scope.is_cancelled()
    }
}
pub(crate) async fn scope_cancelled(scope: &Option<Arc<NativeStreamScope>>) {
    match scope {
        Some(scope) => scope.cancelled().await,
        None => std::future::pending::<()>().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;
    use tokio::process::Command;
    fn fixture_worker() -> Child {
        #[cfg(windows)]
        let mut command = {
            let mut c = Command::new("cmd.exe");
            c.args(["/D", "/C", "pause"]);
            c
        };
        #[cfg(not(windows))]
        let mut command = Command::new("cat");
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap()
    }
    async fn attach(scope: &NativeStreamScope) -> Arc<Mutex<Child>> {
        let mut permit = scope.launch_permit().await.unwrap();
        let child = Arc::new(Mutex::new(fixture_worker()));
        assert!(permit.attach(child.clone()));
        child
    }
    #[tokio::test]
    async fn cleanup_one_session_never_takes_other_sessions_child() {
        let registry = NativeStreamRegistry::default();
        let old = registry.begin("old", 1).await.unwrap();
        let new = registry.begin("new", 2).await.unwrap();
        let a = attach(&old).await;
        let b = attach(&new).await;
        assert!(registry.stop("unknown").await.is_err());
        assert!(registry.begin("unknown", 3).await.is_err());
        assert!(a.lock().await.try_wait().unwrap().is_none());
        assert!(b.lock().await.try_wait().unwrap().is_none());
        assert_eq!(registry.stop("old").await.unwrap(), 1);
        assert!(a.lock().await.try_wait().unwrap().is_some());
        assert!(b.lock().await.try_wait().unwrap().is_none());
        assert!(!new.is_cancelled());
        assert_eq!(registry.stop("old").await.unwrap(), 1);
        assert!(registry.begin("old", 4).await.is_err());
        assert!(b.lock().await.try_wait().unwrap().is_none());
        registry.stop("new").await.unwrap();
    }
    #[tokio::test]
    async fn revocation_cannot_overtake_spawn_registration() {
        let registry = Arc::new(NativeStreamRegistry::default());
        let scope = registry.begin("s", 1).await.unwrap();
        let mut permit = scope.launch_permit().await.unwrap();
        let child = Arc::new(Mutex::new(fixture_worker()));
        let cloned = registry.clone();
        let stopping = tokio::spawn(async move { cloned.stop("s").await });
        timeout(Duration::from_secs(2), scope.cancelled())
            .await
            .unwrap();
        assert!(!stopping.is_finished());
        assert!(!permit.attach(child.clone()));
        drop(permit);
        assert_eq!(stopping.await.unwrap().unwrap(), 1);
        assert!(child.lock().await.try_wait().unwrap().is_some());
        assert!(scope.launch_permit().await.is_err());
    }
    #[tokio::test]
    async fn cancelled_signal_is_retained_without_subscribers() {
        let registry = NativeStreamRegistry::default();
        let scope = registry.begin("s", 1).await.unwrap();
        registry.stop("s").await.unwrap();
        timeout(Duration::from_secs(1), scope.cancelled())
            .await
            .unwrap();
        assert!(scope.launch_permit().await.is_err());
    }
    #[tokio::test]
    async fn multiple_connections_are_all_cancelled_without_touching_new_session() {
        let registry = NativeStreamRegistry::default();
        let a = registry.begin("s", 1).await.unwrap();
        let b = registry.begin("s", 2).await.unwrap();
        assert!(registry.begin("s", 2).await.is_err());
        let x = attach(&a).await;
        let y = attach(&b).await;
        assert_eq!(registry.stop("s").await.unwrap(), 2);
        assert!(x.lock().await.try_wait().unwrap().is_some());
        assert!(y.lock().await.try_wait().unwrap().is_some());
    }
    #[tokio::test]
    async fn actual_host_cleanup_function_is_session_bound_and_fails_unknown() {
        // Isolate this test from the process-global registry by unique UUID IDs.
        let old = format!("old-{}", uuid::Uuid::new_v4());
        let new = format!("new-{}", uuid::Uuid::new_v4());
        let old_scope = NATIVE_STREAM_SCOPES.begin(&old, 1).await.unwrap();
        let new_scope = NATIVE_STREAM_SCOPES.begin(&new, 2).await.unwrap();
        let old_child = attach(&old_scope).await;
        let new_child = attach(&new_scope).await;
        assert!(
            crate::api::stream::cleanup_android_native_stream_session(
                "never-registered",
                "fixture"
            )
            .await
            .is_err()
        );
        assert_eq!(
            crate::api::stream::cleanup_android_native_stream_session(&old, "fixture")
                .await
                .unwrap(),
            1
        );
        assert!(old_child.lock().await.try_wait().unwrap().is_some());
        assert!(new_child.lock().await.try_wait().unwrap().is_none());
        assert!(!new_scope.is_cancelled());
        assert!(NATIVE_STREAM_SCOPES.begin(&old, 3).await.is_err());
        crate::api::stream::cleanup_android_native_stream_session(&new, "fixture")
            .await
            .unwrap();
    }
    #[test]
    fn native_stream_source_never_publishes_or_delayed_kills_global_children() {
        let source = include_str!("api/stream.rs");
        assert!(source.contains(
            "if native_scope.is_none() {\n            replace_active_child(&ACTIVE_STREAM_CHILD"
        ));
        assert!(source.contains("if native_scope.is_none() {\n                    replace_active_child(\n                        &ACTIVE_WINDOW_WATCH_CHILD"));
        assert!(
            source.contains(
                "if native_scope.is_none() {\n                let child = child.clone();"
            )
        );
        let cleanup = source
            .split("pub(crate) async fn cleanup_android_native_stream_session")
            .nth(1)
            .unwrap()
            .split("async fn restart_bundle_sunshine")
            .next()
            .unwrap();
        assert!(!cleanup.contains("ACTIVE_STREAM_CHILD"));
        assert!(!cleanup.contains("ACTIVE_WINDOW_WATCH_CHILD"));
        assert!(!cleanup.contains("cancel_host"));
        assert!(!cleanup.contains("note_host_lifecycle_phase(\"ready\""));
    }
}
