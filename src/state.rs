//! Shared application state: config, store, AI settings, and per-project
//! runtime handles (event broadcast, cancellation, live progress).

use crate::config::Config;
use crate::settings::AiSettings;
use crate::store::Store;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::{broadcast, watch, Mutex as AsyncMutex};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Serialize, Debug)]
pub struct LiveStage {
    pub stage: String,
    pub progress: f32,
    pub detail: Option<String>,
}

pub struct ProjectHandle {
    pub events: broadcast::Sender<String>,
    pub operation: AsyncMutex<()>,
    run: Mutex<RunState>,
    /// In-memory progress for the currently executing stage. Durable state
    /// transitions live in project.json; this fills the gaps between them.
    pub live: Mutex<Option<LiveStage>>,
}

struct RunState {
    next_generation: u64,
    active: Option<ActiveRun>,
}

struct ActiveRun {
    generation: u64,
    token: CancellationToken,
    done: watch::Sender<bool>,
}

pub struct RunLease {
    pub token: CancellationToken,
    pub(crate) generation: u64,
}

impl ProjectHandle {
    fn new() -> Arc<ProjectHandle> {
        let (tx, _) = broadcast::channel(512);
        Arc::new(ProjectHandle {
            events: tx,
            operation: AsyncMutex::new(()),
            run: Mutex::new(RunState {
                next_generation: 0,
                active: None,
            }),
            live: Mutex::new(None),
        })
    }

    pub fn emit(&self, value: serde_json::Value) {
        let _ = self.events.send(value.to_string());
    }

    pub fn set_live(&self, stage: &str, progress: f32, detail: Option<String>) {
        *self.live.lock().unwrap() = Some(LiveStage {
            stage: stage.to_string(),
            progress,
            detail,
        });
    }

    pub fn clear_live(&self) {
        *self.live.lock().unwrap() = None;
    }

    pub fn is_running(&self) -> bool {
        self.run.lock().unwrap().active.is_some()
    }

    pub fn try_start(&self) -> Option<RunLease> {
        let mut run = self.run.lock().unwrap();
        if run.active.is_some() {
            return None;
        }
        run.next_generation = run.next_generation.saturating_add(1);
        let token = CancellationToken::new();
        let (done, _) = watch::channel(false);
        let generation = run.next_generation;
        run.active = Some(ActiveRun {
            generation,
            token: token.clone(),
            done,
        });
        Some(RunLease { token, generation })
    }

    pub fn request_cancel(&self) -> Option<watch::Receiver<bool>> {
        let run = self.run.lock().unwrap();
        let active = run.active.as_ref()?;
        let done = active.done.subscribe();
        active.token.cancel();
        Some(done)
    }

    pub fn finish(&self, generation: u64) {
        let done = {
            let mut run = self.run.lock().unwrap();
            if run.active.as_ref().map(|active| active.generation) != Some(generation) {
                return;
            }
            run.active.take().map(|active| active.done)
        };
        if let Some(done) = done {
            let _ = done.send(true);
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub store: Store,
    pub settings: Arc<RwLock<AiSettings>>,
    handles: Arc<Mutex<HashMap<String, Arc<ProjectHandle>>>>,
    /// `project-id/clip-id` keys with a caption restyle currently running.
    restyling: Arc<Mutex<HashSet<String>>>,
    /// Serializes settings validation and persistence so concurrent updates
    /// cannot overwrite a newer value with an older request.
    pub settings_update: Arc<AsyncMutex<()>>,
    /// Bytes currently reserved by uploads that have not finished or failed.
    upload_bytes: Arc<Mutex<u64>>,
}

impl AppState {
    pub fn new(cfg: Config) -> AppState {
        let store = Store::new(&cfg.data_dir);
        let settings = crate::settings::load(&cfg.data_dir);
        AppState {
            cfg: Arc::new(cfg),
            store,
            settings: Arc::new(RwLock::new(settings)),
            handles: Arc::new(Mutex::new(HashMap::new())),
            restyling: Arc::new(Mutex::new(HashSet::new())),
            settings_update: Arc::new(AsyncMutex::new(())),
            upload_bytes: Arc::new(Mutex::new(0)),
        }
    }

    pub fn handle(&self, id: &str) -> Arc<ProjectHandle> {
        let mut map = self.handles.lock().unwrap();
        map.entry(id.to_string())
            .or_insert_with(ProjectHandle::new)
            .clone()
    }

    /// Claim the restyle lock for one clip. Returns false when a restyle for
    /// the same clip is already running.
    pub fn try_begin_restyle(&self, key: &str) -> bool {
        self.restyling.lock().unwrap().insert(key.to_string())
    }

    pub fn end_restyle(&self, key: &str) {
        self.restyling.lock().unwrap().remove(key);
    }

    /// Reserve additional bytes against a capacity snapshot. The shared
    /// counter prevents concurrent uploads from each spending the same free
    /// disk space reported by `df`.
    pub fn reserve_upload_bytes(&self, additional: u64, capacity: u64) -> bool {
        let mut reserved = self.upload_bytes.lock().unwrap();
        let Some(total) = reserved.checked_add(additional) else {
            return false;
        };
        if total > capacity {
            return false;
        }
        *reserved = total;
        true
    }

    pub fn release_upload_bytes(&self, bytes: u64) {
        let mut reserved = self.upload_bytes.lock().unwrap();
        *reserved = reserved.saturating_sub(bytes);
    }

    #[cfg(test)]
    fn reserved_upload_bytes(&self) -> u64 {
        *self.upload_bytes.lock().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_without_an_active_run_does_not_poison_the_next_run() {
        let handle = ProjectHandle::new();

        assert!(handle.request_cancel().is_none());

        let lease = handle.try_start().expect("run should start");
        assert!(!lease.token.is_cancelled());
        handle.finish(lease.generation);
    }

    #[test]
    fn upload_reservations_share_one_capacity_budget() {
        let state = AppState::new(Config::resolve());
        assert!(state.reserve_upload_bytes(60, 100));
        assert!(!state.reserve_upload_bytes(50, 100));
        assert_eq!(state.reserved_upload_bytes(), 60);
        state.release_upload_bytes(60);
        assert!(state.reserve_upload_bytes(50, 100));
        state.release_upload_bytes(50);
    }

    #[test]
    fn duplicate_cancel_is_idempotent_and_a_stale_finish_cannot_end_new_run() {
        let handle = ProjectHandle::new();
        let first = handle.try_start().expect("first run should start");
        let first_done = handle.request_cancel().expect("first run is cancellable");
        let second_done = handle.request_cancel().expect("duplicate cancel is safe");
        assert!(first.token.is_cancelled());

        handle.finish(first.generation);
        assert!(*first_done.borrow());
        assert!(*second_done.borrow());

        let second = handle.try_start().expect("next run should start");
        handle.finish(first.generation);
        assert!(handle.is_running(), "stale completion ended the new run");
        handle.finish(second.generation);
        assert!(!handle.is_running());
    }
}
