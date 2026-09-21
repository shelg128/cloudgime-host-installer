//! Native control cancellation ownership. This records only directly-owned child
//! handles and connection generations. It is NOT a rental receipt, Sunshine
//! server termination proof, or a claim-release authority. No global/PID scan.
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Weak},
    time::Duration,
};
use tokio::{
    process::Child,
    sync::{Mutex, MutexGuard, OwnedMutexGuard, watch},
    time::timeout,
};

const CAPACITY: usize = 4096;
const STOP_TIMEOUT: Duration = Duration::from_secs(8);
#[derive(Default)]
pub(crate) struct NativeStreamRegistry {
    sessions: Arc<Mutex<HashMap<String, Entry>>>,
}
#[derive(Default)]
struct Entry {
    revoked: bool,
    registered: bool,
    verified_child_exits: usize,
    connections: HashMap<u64, Arc<NativeStreamScope>>,
    // Sunshine accepts concurrent sessions but its encrypted RTSP setup is not
    // safe when two clients handshake at the exact same time. Serialize only
    // that startup phase; established streams do not retain this gate.
    startup_gate: Arc<Mutex<()>>,
}
pub(crate) static NATIVE_STREAM_SCOPES: LazyLock<NativeStreamRegistry> =
    LazyLock::new(NativeStreamRegistry::default);

pub(crate) struct NativeStreamScope {
    registry: Weak<Mutex<HashMap<String, Entry>>>,
    session_id: String,
    generation: u64,
    primary: bool,
    startup_gate: Arc<Mutex<()>>,
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
        let primary = entry.connections.is_empty();
        let (cancelled, _) = watch::channel(false);
        let scope = Arc::new(NativeStreamScope {
            registry: Arc::downgrade(&self.sessions),
            session_id: session_id.into(),
            generation,
            primary,
            startup_gate: entry.startup_gate.clone(),
            cancelled,
            children: Mutex::new(Vec::new()),
        });
        entry.registered = true;
        entry.connections.insert(generation, scope.clone());
        Ok(scope)
    }
    /// Access revocation may arrive after this process restarted. A tombstone is
    /// allowed only if no unverified local connection already owns that ID. The
    /// check + insert are atomic under the registry lock. This is NOT a receipt.
    pub(crate) async fn tombstone_if_unregistered(
        &self,
        session_id: &str,
    ) -> Result<usize, String> {
        if session_id.is_empty() || session_id.len() > 256 {
            return Err("native_scope_binding_invalid".into());
        }
        let mut sessions = self.sessions.lock().await;
        if !sessions.contains_key(session_id) && sessions.len() >= CAPACITY {
            return Err("native_scope_capacity_reached".into());
        }
        let entry = sessions.entry(session_id.into()).or_default();
        if !entry.connections.is_empty() {
            return Err("native_scope_unverified_connection_exists".into());
        }
        entry.revoked = true;
        Ok(0)
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
            if !entry.registered {
                return Err("native_scope_no_registered_connection".into());
            }
            for scope in entry.connections.values() {
                scope.cancelled.send_replace(true);
            }
            entry.connections.values().cloned().collect::<Vec<_>>()
        };
        // Closing one device can retire its scope concurrently. Each successful
        // stop is recorded once under the registry lock, so a repeated root
        // revoke returns stable evidence without counting a child twice.
        for scope in scopes {
            scope.stop_owned_children().await?;
        }
        let sessions = self.sessions.lock().await;
        let entry = sessions.get(session_id).ok_or("native_scope_missing")?;
        if !entry.connections.is_empty() {
            return Err("native_child_exit_not_verified".into());
        }
        Ok(entry.verified_child_exits)
    }
}
// Lifetime of the owning connection future, including early startup failure.
// Cancellation is synchronous; OS termination is bounded and uses retained handles.
pub(crate) struct NativeConnectionGuard(Arc<NativeStreamScope>);
impl Drop for NativeConnectionGuard {
    fn drop(&mut self) {
        self.0.cancelled.send_replace(true);
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let scope = self.0.clone();
            runtime.spawn(async move {
                if let Err(error) = scope.stop_owned_children().await {
                    log::warn!("[Scoped connection]: cleanup unverified: {error}");
                }
            });
        }
    }
}

impl NativeStreamScope {
    pub(crate) fn connection_guard(self: &Arc<Self>) -> NativeConnectionGuard {
        NativeConnectionGuard(self.clone())
    }
    pub(crate) fn is_primary(&self) -> bool {
        self.primary
    }
    pub(crate) fn is_cancelled(&self) -> bool {
        *self.cancelled.borrow()
    }
    pub(crate) async fn startup_permit(&self) -> Result<OwnedMutexGuard<()>, &'static str> {
        let mut signal = self.cancelled.subscribe();
        if *signal.borrow() {
            return Err("native_scope_revoked");
        }
        tokio::select! {
            guard = self.startup_gate.clone().lock_owned() => {
                if self.is_cancelled() { Err("native_scope_revoked") } else { Ok(guard) }
            }
            _ = signal.changed() => Err("native_scope_revoked"),
        }
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
    /// Gives owned children a short grace window after an authenticated Stop
    /// command. Ownership is retained throughout; timeout does not authorize
    /// release and the caller must still invoke `stop_owned_children`.
    pub(crate) async fn wait_owned_children_exit(&self, max_wait: Duration) -> bool {
        let started = tokio::time::Instant::now();
        loop {
            let children = self.children.lock().await;
            let mut all_exited = true;
            for child in children.iter() {
                match child.lock().await.try_wait() {
                    Ok(Some(_)) => {}
                    Ok(None) | Err(_) => all_exited = false,
                }
            }
            drop(children);
            if all_exited {
                return true;
            }
            if started.elapsed() >= max_wait {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    pub(crate) async fn stop_owned_children(&self) -> Result<usize, String> {
        self.cancelled.send_replace(true);
        let count = timeout(STOP_TIMEOUT, self.stop_children())
            .await
            .map_err(|_| "native_child_exit_not_verified".to_string())??;
        // Never discard an unverified child on error/timeout. A cancelled scope
        // still consumes capacity until every retained OS handle has exited.
        if let Some(registry) = self.registry.upgrade() {
            let mut sessions = registry.lock().await;
            if let Some(entry) = sessions.get_mut(&self.session_id) {
                let same_scope = entry
                    .connections
                    .get(&self.generation)
                    .is_some_and(|current| std::ptr::eq(Arc::as_ptr(current), self));
                if same_scope {
                    entry.connections.remove(&self.generation);
                    entry.verified_child_exits = entry.verified_child_exits.saturating_add(count);
                }
            }
        }
        Ok(count)
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
    async fn graceful_wait_observes_exit_without_releasing_scope() {
        let registry = NativeStreamRegistry::default();
        let scope = registry.begin("grace", 1).await.unwrap();
        let child = attach(&scope).await;
        assert!(
            !scope
                .wait_owned_children_exit(Duration::from_millis(30))
                .await
        );
        child.lock().await.start_kill().unwrap();
        child.lock().await.wait().await.unwrap();
        assert!(scope.wait_owned_children_exit(Duration::from_secs(1)).await);
        assert_eq!(scope.stop_owned_children().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn connection_failure_cancels_only_its_owned_children() {
        let registry = NativeStreamRegistry::default();
        let failed = registry.begin("failed-start", 1).await.unwrap();
        let other = registry.begin("other", 2).await.unwrap();
        let guard = failed.connection_guard();
        let a = attach(&failed).await;
        let b = attach(&other).await;
        drop(guard);
        assert!(failed.is_cancelled());
        timeout(Duration::from_secs(10), async {
            loop {
                if a.lock().await.try_wait().unwrap().is_some() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(b.lock().await.try_wait().unwrap().is_none());
        assert!(!other.is_cancelled());
        registry.stop("other").await.unwrap();
    }

    #[tokio::test]
    async fn disconnect_one_device_preserves_sibling_and_releases_capacity() {
        let registry = NativeStreamRegistry::default();
        let phone = registry.begin("rental-root", 1).await.unwrap();
        let phone_child = attach(&phone).await;
        for generation in 2..=25 {
            let pc = registry.begin("rental-root", generation).await.unwrap();
            let pc_child = attach(&pc).await;
            let guard = pc.connection_guard();
            drop(guard);
            timeout(Duration::from_secs(10), async {
                loop {
                    let retired = !registry.sessions.lock().await["rental-root"]
                        .connections
                        .contains_key(&generation);
                    if retired {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            assert!(pc_child.lock().await.try_wait().unwrap().is_some());
            assert!(!phone.is_cancelled());
            assert!(phone_child.lock().await.try_wait().unwrap().is_none());
        }
        let entry_count = registry.sessions.lock().await["rental-root"]
            .connections
            .len();
        assert_eq!(
            entry_count, 1,
            "completed PC connections must not consume slots"
        );
        assert_eq!(registry.stop("rental-root").await.unwrap(), 25);
        assert!(phone_child.lock().await.try_wait().unwrap().is_some());
        assert_eq!(registry.stop("rental-root").await.unwrap(), 25);
        assert!(registry.begin("rental-root", 26).await.is_err());
    }

    #[tokio::test]
    async fn sibling_startup_is_serial_until_permit_release() {
        let registry = NativeStreamRegistry::default();
        let pc = registry.begin("root", 1).await.unwrap();
        let hp = registry.begin("root", 2).await.unwrap();
        let pc_permit = pc.startup_permit().await.unwrap();
        let hp_wait = hp.startup_permit();
        tokio::pin!(hp_wait);
        assert!(
            timeout(Duration::from_millis(50), &mut hp_wait)
                .await
                .is_err()
        );
        drop(pc_permit);
        assert!(timeout(Duration::from_secs(1), &mut hp_wait).await.is_ok());
        registry.stop("root").await.unwrap();
    }

    #[tokio::test]
    async fn revoke_unblocks_waiting_startup_without_waiting_for_holder() {
        let registry = NativeStreamRegistry::default();
        let pc = registry.begin("root-cancel", 1).await.unwrap();
        let hp = registry.begin("root-cancel", 2).await.unwrap();
        let _held = pc.startup_permit().await.unwrap();
        let waiting = hp.startup_permit();
        tokio::pin!(waiting);
        assert!(
            timeout(Duration::from_millis(20), &mut waiting)
                .await
                .is_err()
        );
        timeout(Duration::from_secs(1), registry.stop("root-cancel"))
            .await
            .unwrap()
            .unwrap();
        assert!(
            timeout(Duration::from_secs(1), &mut waiting)
                .await
                .unwrap()
                .is_err()
        );
        assert!(hp.startup_permit().await.is_err());
    }

    #[tokio::test]
    async fn independent_roots_do_not_share_startup_gate() {
        let registry = NativeStreamRegistry::default();
        let pc = registry.begin("root-a", 1).await.unwrap();
        let other = registry.begin("root-b", 2).await.unwrap();
        let _held = pc.startup_permit().await.unwrap();
        let _other = timeout(Duration::from_secs(1), other.startup_permit())
            .await
            .unwrap()
            .unwrap();
        registry.stop("root-a").await.unwrap();
        registry.stop("root-b").await.unwrap();
    }

    #[tokio::test]
    async fn delayed_cleanup_of_old_generation_does_not_remove_reused_slot() {
        let registry = NativeStreamRegistry::default();
        let old = registry.begin("root", 7).await.unwrap();
        let old_child = attach(&old).await;
        assert_eq!(old.stop_owned_children().await.unwrap(), 1);
        let current = registry.begin("root", 7).await.unwrap();
        let current_child = attach(&current).await;
        // A delayed duplicate stop/guard drop must not retire the new scope.
        assert_eq!(old.stop_owned_children().await.unwrap(), 1);
        assert!(old_child.lock().await.try_wait().unwrap().is_some());
        let sessions = registry.sessions.lock().await;
        assert!(Arc::ptr_eq(&sessions["root"].connections[&7], &current));
        assert_eq!(sessions["root"].verified_child_exits, 1);
        drop(sessions);
        assert!(current_child.lock().await.try_wait().unwrap().is_none());
        assert_eq!(registry.stop("root").await.unwrap(), 2);
    }

    #[tokio::test]
    async fn concurrent_root_revokes_count_each_retired_child_once() {
        let registry = NativeStreamRegistry::default();
        let pc = registry.begin("root", 1).await.unwrap();
        let hp = registry.begin("root", 2).await.unwrap();
        let x = attach(&pc).await;
        let y = attach(&hp).await;
        let (a, b, closing) = tokio::join!(
            registry.stop("root"),
            registry.stop("root"),
            pc.stop_owned_children()
        );
        assert_eq!(a.unwrap(), 2);
        assert_eq!(b.unwrap(), 2);
        assert_eq!(closing.unwrap(), 1);
        assert_eq!(
            registry.sessions.lock().await["root"].verified_child_exits,
            2
        );
        assert!(x.lock().await.try_wait().unwrap().is_some());
        assert!(y.lock().await.try_wait().unwrap().is_some());
    }

    #[tokio::test]
    async fn cancelled_stop_retains_scope_until_child_exit_is_verified() {
        let registry = Arc::new(NativeStreamRegistry::default());
        let pc = registry.begin("root", 1).await.unwrap();
        let child = attach(&pc).await;
        let held_child_lock = child.lock().await;
        let revoked = registry.clone();
        let stop = tokio::spawn(async move { revoked.stop("root").await });
        timeout(Duration::from_secs(2), pc.cancelled())
            .await
            .unwrap();
        stop.abort();
        let _ = stop.await;
        let sessions = registry.sessions.lock().await;
        assert!(sessions["root"].connections.contains_key(&1));
        assert_eq!(sessions["root"].verified_child_exits, 0);
        drop(sessions);
        assert!(registry.begin("root", 2).await.is_err());
        drop(held_child_lock);
        assert_eq!(registry.stop("root").await.unwrap(), 1);
        assert!(child.lock().await.try_wait().unwrap().is_some());
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
        let normalized = source.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            normalized
                .contains("if native_scope.is_none() { replace_active_child(&ACTIVE_STREAM_CHILD")
        );
        assert!(normalized.contains(
            "if native_scope.is_none() { replace_active_child( &ACTIVE_WINDOW_WATCH_CHILD"
        ));
        assert!(normalized.contains("if native_scope.is_none() { let child = child.clone();"));
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
