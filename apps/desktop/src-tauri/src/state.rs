//! Process-wide application state: vault handle, OCR model paths, idle lock.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use oikonomia_core::error::Error as CoreError;
use oikonomia_core::ledger::DEFAULT_LOCK_TIMEOUT_SECS;
use oikonomia_core::vault::{Vault, VaultStatus, default_data_dir};

/// Shared state behind Tauri commands.
pub struct AppState {
    vault: Arc<Mutex<Vault>>,
    /// App data directory: vault files plus the plaintext UI prefs.
    data_dir: PathBuf,
    /// Directory containing bundled `text-detection.rten` + `text-recognition.rten`.
    ocr_model_dir: PathBuf,
    /// Seconds since `UNIX_EPOCH` of the last command touching the vault.
    last_activity: Arc<AtomicU64>,
    /// Cached idle timeout for the watchdog; the persisted value lives in the vault.
    lock_timeout_secs: Arc<AtomicU64>,
    /// Parks the idle watchdog while the vault is not unlocked.
    gate: Arc<WatchdogGate>,
    /// Canonical paths from native drag-drop; path IPC commands accept only these.
    allowed_drop_paths: Mutex<HashSet<PathBuf>>,
    /// Serializes plaintext prefs load-mutate-save.
    prefs_lock: Mutex<()>,
}

impl AppState {
    /// Open the vault path; resolve OCR models next to the binary / resources.
    ///
    /// # Errors
    ///
    /// Propagates vault I/O errors from the default data directory.
    pub fn new(ocr_model_dir: PathBuf) -> Result<Self, CoreError> {
        Self::open_path(default_data_dir()?, ocr_model_dir)
    }

    /// Open a vault in `data_dir` (tests and [`Self::new`]).
    pub(crate) fn open_path(data_dir: PathBuf, ocr_model_dir: PathBuf) -> Result<Self, CoreError> {
        let vault = Vault::open_path(data_dir.clone())?;
        Ok(Self {
            vault: Arc::new(Mutex::new(vault)),
            data_dir,
            ocr_model_dir,
            last_activity: Arc::new(AtomicU64::new(now_secs())),
            lock_timeout_secs: Arc::new(AtomicU64::new(DEFAULT_LOCK_TIMEOUT_SECS)),
            gate: Arc::new(WatchdogGate::new()),
            allowed_drop_paths: Mutex::new(HashSet::new()),
            prefs_lock: Mutex::new(()),
        })
    }

    /// App data directory (UI prefs live here as plaintext).
    #[must_use]
    pub fn data_dir(&self) -> &std::path::Path {
        &self.data_dir
    }

    /// Path to bundled OCR models.
    #[must_use]
    pub fn ocr_model_dir(&self) -> &PathBuf {
        &self.ocr_model_dir
    }

    /// Shared vault handle for blocking work off the command thread.
    #[must_use]
    pub fn vault(&self) -> Arc<Mutex<Vault>> {
        Arc::clone(&self.vault)
    }

    /// Hold across a prefs load-mutate-save so theme and tray last-used cannot clobber.
    pub fn lock_prefs(&self) -> std::sync::MutexGuard<'_, ()> {
        match self.prefs_lock.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                self.prefs_lock.clear_poison();
                poisoned.into_inner()
            }
        }
    }

    /// Record native drop paths for later path-based analyze/post commands.
    pub fn remember_drop_paths(&self, paths: impl IntoIterator<Item = PathBuf>) {
        let mut allowed = match self.allowed_drop_paths.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                self.allowed_drop_paths.clear_poison();
                poisoned.into_inner()
            }
        };
        for path in paths {
            if let Ok(canonical) = path.canonicalize() {
                allowed.insert(canonical);
            }
        }
    }

    /// True when `path` was recorded from a native drop (after canonicalize).
    #[must_use]
    pub fn drop_path_allowed(&self, path: &Path) -> bool {
        let allowed = match self.allowed_drop_paths.lock() {
            Ok(set) => set,
            Err(poisoned) => {
                self.allowed_drop_paths.clear_poison();
                poisoned.into_inner()
            }
        };
        path_is_allowed_set(&allowed, path)
    }

    /// Record command activity for the idle watchdog.
    pub fn touch(&self) {
        self.last_activity.store(now_secs(), Ordering::Relaxed);
    }

    /// Cache the idle timeout so the watchdog needs no vault access.
    pub fn set_lock_timeout_cache(&self, secs: u64) {
        self.lock_timeout_secs.store(secs, Ordering::Relaxed);
    }

    /// Handles for the idle watchdog thread.
    #[must_use]
    pub fn watchdog_handles(&self) -> WatchdogHandles {
        WatchdogHandles {
            vault: Arc::clone(&self.vault),
            last_activity: Arc::clone(&self.last_activity),
            lock_timeout_secs: Arc::clone(&self.lock_timeout_secs),
            gate: Arc::clone(&self.gate),
        }
    }

    /// Park or run the idle watchdog after a vault lock-state change.
    ///
    /// Call after releasing the vault mutex. Unlock notifies the parked
    /// thread; lock notifies so an in-flight poll wait returns and parks.
    pub fn sync_watchdog_gate(&self, status: VaultStatus) {
        if matches!(status, VaultStatus::Unlocked) {
            self.gate.set_running();
        } else {
            self.gate.set_parked();
        }
    }
}

/// Compare a candidate path against a set of already-canonicalized drop paths.
#[must_use]
pub fn path_is_allowed_set(allowed: &HashSet<PathBuf>, path: &Path) -> bool {
    path.canonicalize()
        .ok()
        .is_some_and(|canonical| allowed.contains(&canonical))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// How often the watchdog re-checks idle time while the vault is unlocked.
pub const AUTO_LOCK_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Shared handles for [`spawn_auto_lock`].
pub struct WatchdogHandles {
    /// Vault mutex the watchdog locks only for a status check / idle lock.
    pub vault: Arc<Mutex<Vault>>,
    /// Seconds since `UNIX_EPOCH` of the last command touching the vault.
    pub last_activity: Arc<AtomicU64>,
    /// Cached idle timeout; the persisted value lives in the vault.
    pub lock_timeout_secs: Arc<AtomicU64>,
    /// Park while locked; poll while unlocked.
    pub gate: Arc<WatchdogGate>,
}

/// Gate that parks the idle watchdog while the vault is not unlocked.
///
/// Unlock sets the gate to running and notifies. Lock parks and notifies so
/// an in-flight poll wait returns immediately. The vault mutex is never
/// held across a wait.
pub struct WatchdogGate {
    state: Mutex<GateState>,
    cond: Condvar,
    /// Completed unlocked poll intervals. Stays zero while the thread is parked.
    #[cfg(test)]
    ticks: AtomicU64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GateState {
    Parked,
    Running,
    Shutdown,
}

impl WatchdogGate {
    /// Start parked: the process launches with the vault locked or missing.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Mutex::new(GateState::Parked),
            cond: Condvar::new(),
            #[cfg(test)]
            ticks: AtomicU64::new(0),
        }
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, GateState> {
        match self.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                self.state.clear_poison();
                poisoned.into_inner()
            }
        }
    }

    fn wait<'a>(
        &self,
        guard: std::sync::MutexGuard<'a, GateState>,
    ) -> std::sync::MutexGuard<'a, GateState> {
        match self.cond.wait(guard) {
            Ok(guard) => guard,
            Err(poisoned) => {
                self.state.clear_poison();
                poisoned.into_inner()
            }
        }
    }

    fn wait_timeout<'a>(
        &self,
        guard: std::sync::MutexGuard<'a, GateState>,
        interval: Duration,
    ) -> std::sync::MutexGuard<'a, GateState> {
        match self.cond.wait_timeout(guard, interval) {
            Ok((guard, _)) => guard,
            Err(poisoned) => {
                self.state.clear_poison();
                poisoned.into_inner().0
            }
        }
    }

    /// Wake the watchdog so it resumes the unlocked poll interval.
    pub fn set_running(&self) {
        let mut state = self.lock_state();
        if *state == GateState::Shutdown {
            return;
        }
        *state = GateState::Running;
        self.cond.notify_all();
    }

    /// Park the watchdog; notifies so a poll wait does not run to completion.
    pub fn set_parked(&self) {
        let mut state = self.lock_state();
        if *state == GateState::Shutdown {
            return;
        }
        *state = GateState::Parked;
        self.cond.notify_all();
    }

    /// Wake any waiter and stop the loop. Process exit also ends the thread.
    #[cfg(test)]
    pub fn shutdown(&self) {
        let mut state = self.lock_state();
        *state = GateState::Shutdown;
        self.cond.notify_all();
    }

    fn wait_until_running(&self) -> GateState {
        let mut state = self.lock_state();
        while *state == GateState::Parked {
            state = self.wait(state);
        }
        *state
    }

    fn wait_poll_interval(&self, interval: Duration) -> GateState {
        let state = self.lock_state();
        if *state != GateState::Running {
            return *state;
        }
        let state = self.wait_timeout(state, interval);
        *state
    }

    #[cfg(test)]
    fn record_tick(&self) {
        self.ticks.fetch_add(1, Ordering::Relaxed);
    }

    /// Unlocked poll completions. Zero while parked.
    #[cfg(test)]
    fn tick_count(&self) -> u64 {
        self.ticks.load(Ordering::Relaxed)
    }
}

impl Default for WatchdogGate {
    fn default() -> Self {
        Self::new()
    }
}

/// Lock the vault mutex, recovering from poisoning.
///
/// A panic inside a command closure must not brick the session or disable the
/// watchdog: recover the guard, force the vault into the safe locked state,
/// and clear the poison flag so later locks are clean.
pub fn lock_vault(vault: &Mutex<Vault>) -> std::sync::MutexGuard<'_, Vault> {
    match vault.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            vault.clear_poison();
            let mut guard = poisoned.into_inner();
            guard.lock();
            guard
        }
    }
}

/// Lock iff the vault is unlocked and idle time has reached the timeout.
///
/// Pure predicate for the idle watchdog. Tests cover the four launch cases
/// without sleeping the 5s loop or constructing an `AppHandle`.
#[must_use]
pub const fn should_auto_lock(idle_secs: u64, timeout_secs: u64, status: VaultStatus) -> bool {
    // `matches!` is const; derived `PartialEq` on `VaultStatus` is not.
    matches!(status, VaultStatus::Unlocked) && idle_secs >= timeout_secs
}

/// Whether a watchdog tick that just locked should emit `vault-locked`.
///
/// Emit needs `AppHandle`; this returns the decision so tests can assert the
/// emit path without the Tauri runtime.
#[must_use]
pub const fn should_emit_vault_locked(did_lock: bool) -> bool {
    did_lock
}

/// Lock the vault from Rust when idle, regardless of webview state (F5).
///
/// The frontend timer is only a fast-path duplicate; this thread guarantees
/// the vault locks even if the webview throttles timers or stalls. Emits
/// `vault-locked` so the UI can drop to the unlock screen.
///
/// While the vault is not unlocked the thread parks on [`WatchdogGate`]
/// instead of waking every poll interval, so a locked session can stay
/// open (or hidden to tray) without a periodic wakeup. Unlock notifies;
/// lock notifies so an in-flight poll wait parks immediately.
pub fn spawn_auto_lock(app: tauri::AppHandle, handles: WatchdogHandles) {
    std::thread::spawn(move || {
        run_auto_lock_loop(handles, AUTO_LOCK_POLL_INTERVAL, || {
            use tauri::Emitter;
            let _ = app.emit("vault-locked", ());
        });
    });
}

/// Idle watchdog loop. `on_auto_locked` runs after the vault mutex is released.
fn run_auto_lock_loop(
    handles: WatchdogHandles,
    poll_interval: Duration,
    mut on_auto_locked: impl FnMut(),
) {
    let WatchdogHandles {
        vault,
        last_activity,
        lock_timeout_secs,
        gate,
    } = handles;

    loop {
        match gate.wait_until_running() {
            GateState::Shutdown => return,
            GateState::Parked | GateState::Running => {}
        }

        match gate.wait_poll_interval(poll_interval) {
            GateState::Shutdown => return,
            GateState::Parked => continue,
            GateState::Running => {}
        }

        #[cfg(test)]
        gate.record_tick();

        let idle = now_secs().saturating_sub(last_activity.load(Ordering::Relaxed));
        let timeout = lock_timeout_secs.load(Ordering::Relaxed);

        let mut locked_now = false;
        {
            let mut vault = lock_vault(&vault);
            if should_auto_lock(idle, timeout, vault.status()) {
                vault.lock();
                locked_now = true;
            }
        }

        if locked_now {
            gate.set_parked();
        }

        if should_emit_vault_locked(locked_now) {
            on_auto_locked();
        }
    }
}

/// Resolve OCR model directory for dev and packaged builds.
#[must_use]
pub fn resolve_ocr_model_dir(resource_dir: Option<PathBuf>) -> PathBuf {
    // 1) Tauri resource dir (packaged): …/resources/ocr
    if let Some(dir) = resource_dir {
        let candidate = dir.join("ocr");
        if candidate.join("text-detection.rten").is_file() {
            return candidate;
        }
        // Sometimes resources land flat under resource_dir
        if dir.join("text-detection.rten").is_file() {
            return dir;
        }
    }

    // 2) Dev layout: apps/desktop/src-tauri/resources/ocr
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/ocr");
    if dev.join("text-detection.rten").is_file() {
        return dev;
    }

    // 3) Fallback next to executable
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        let near = parent.join("resources/ocr");
        if near.join("text-detection.rten").is_file() {
            return near;
        }
    }

    dev
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
mod tests {
    use super::{
        AUTO_LOCK_POLL_INTERVAL, AppState, WatchdogGate, lock_vault, path_is_allowed_set,
        run_auto_lock_loop, should_auto_lock, should_emit_vault_locked,
    };
    use oikonomia_core::vault::VaultStatus;
    use std::collections::HashSet;
    use std::fs;
    use std::sync::Arc;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    const TEST_PASSWORD: &str = "correct horse battery staple";
    /// Longer than [`AUTO_LOCK_POLL_INTERVAL`] so a still-polling thread would tick.
    const PAST_POLL: Duration = Duration::from_millis(5_500);

    #[test]
    fn path_is_allowed_requires_canonical_membership() {
        let dir = std::env::temp_dir().join(format!("oiko-allow-{}", std::process::id()));
        if fs::create_dir_all(&dir).is_err() {
            return;
        }
        let file = dir.join("drop.pdf");
        if fs::write(&file, b"%PDF").is_err() {
            return;
        }
        let Ok(canonical) = file.canonicalize() else {
            return;
        };
        let mut allowed = HashSet::new();
        allowed.insert(canonical);

        assert!(path_is_allowed_set(&allowed, &file));
        assert!(!path_is_allowed_set(&allowed, &dir.join("other.pdf")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn auto_lock_when_unlocked_and_idle_meets_timeout() {
        assert!(should_auto_lock(60, 60, VaultStatus::Unlocked));
        assert!(should_auto_lock(61, 60, VaultStatus::Unlocked));
        assert!(
            should_emit_vault_locked(true),
            "a successful idle lock would emit vault-locked"
        );
    }

    #[test]
    fn no_auto_lock_when_idle_below_timeout() {
        assert!(!should_auto_lock(59, 60, VaultStatus::Unlocked));
        assert!(!should_emit_vault_locked(false));
    }

    #[test]
    fn no_auto_lock_after_activity_resets_idle() {
        assert!(!should_auto_lock(0, 60, VaultStatus::Unlocked));
        assert!(!should_emit_vault_locked(false));
    }

    #[test]
    fn no_auto_lock_when_already_locked() {
        assert!(!should_auto_lock(120, 60, VaultStatus::Locked));
        assert!(!should_auto_lock(120, 60, VaultStatus::Uninitialized));
        assert!(
            !should_emit_vault_locked(false),
            "already locked must not emit vault-locked"
        );
    }

    #[test]
    fn auto_lock_poll_interval_is_five_seconds() {
        assert_eq!(AUTO_LOCK_POLL_INTERVAL, Duration::from_secs(5));
    }

    #[test]
    fn watchdog_parks_while_locked_and_still_auto_locks() {
        let (state, dir) = test_state("park");
        init_locked_vault(&state);
        let handles = state.watchdog_handles();
        let gate = Arc::clone(&handles.gate);
        let vault = Arc::clone(&handles.vault);
        let last_activity = Arc::clone(&handles.last_activity);
        let (join, emits) = spawn_watchdog(&state, AUTO_LOCK_POLL_INTERVAL);

        std::thread::sleep(PAST_POLL);
        assert_eq!(
            gate.tick_count(),
            0,
            "locked watchdog must not complete a 5s poll tick"
        );
        assert_eq!(lock_vault(&vault).status(), VaultStatus::Locked);

        {
            let mut guard = lock_vault(&vault);
            guard.unlock(TEST_PASSWORD).expect("unlock");
        }
        last_activity.store(0, Ordering::Relaxed);
        state.set_lock_timeout_cache(60);
        state.sync_watchdog_gate(VaultStatus::Unlocked);

        std::thread::sleep(PAST_POLL);
        assert!(
            gate.tick_count() >= 1,
            "unlock must unpark and run an idle check"
        );
        assert_eq!(
            lock_vault(&vault).status(),
            VaultStatus::Locked,
            "idle unlocked session must still auto-lock"
        );
        emits
            .recv_timeout(Duration::from_millis(200))
            .expect("auto-lock emits vault-locked");

        let ticks_after_lock = gate.tick_count();
        std::thread::sleep(PAST_POLL);
        assert_eq!(
            gate.tick_count(),
            ticks_after_lock,
            "auto-lock must park; no further 5s ticks"
        );

        shutdown_watchdog(&gate, join);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn watchdog_lock_parks_before_poll_wait_finishes() {
        let (state, dir) = test_state("inflight");
        init_locked_vault(&state);
        {
            let vault = state.vault();
            let mut guard = lock_vault(&vault);
            guard.unlock(TEST_PASSWORD).expect("unlock");
        }
        state.touch();
        state.set_lock_timeout_cache(15 * 60);
        state.sync_watchdog_gate(VaultStatus::Unlocked);

        let gate = Arc::clone(&state.watchdog_handles().gate);
        let (join, _emits) = spawn_watchdog(&state, AUTO_LOCK_POLL_INTERVAL);

        std::thread::sleep(Duration::from_millis(200));
        {
            let vault = state.vault();
            lock_vault(&vault).lock();
        }
        state.sync_watchdog_gate(VaultStatus::Locked);

        std::thread::sleep(PAST_POLL);
        assert_eq!(
            gate.tick_count(),
            0,
            "lock must park before the unlocked poll wait completes"
        );

        shutdown_watchdog(&gate, join);
        let _ = fs::remove_dir_all(&dir);
    }

    fn test_state(label: &str) -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "oiko-watchdog-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        fs::create_dir_all(&dir).expect("tmpdir");
        let state = AppState::open_path(dir.clone(), dir.clone()).expect("state");
        (state, dir)
    }

    fn init_locked_vault(state: &AppState) {
        {
            let vault = state.vault();
            let mut guard = lock_vault(&vault);
            guard.init(TEST_PASSWORD).expect("vault init");
            guard.lock();
        }
        state.sync_watchdog_gate(VaultStatus::Locked);
    }

    fn spawn_watchdog(
        state: &AppState,
        poll: Duration,
    ) -> (std::thread::JoinHandle<()>, mpsc::Receiver<()>) {
        let (tx, rx) = mpsc::channel();
        let handles = state.watchdog_handles();
        let join = std::thread::spawn(move || {
            run_auto_lock_loop(handles, poll, move || {
                let _ = tx.send(());
            });
        });
        (join, rx)
    }

    fn shutdown_watchdog(gate: &WatchdogGate, join: std::thread::JoinHandle<()>) {
        gate.shutdown();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = join.join();
            let _ = tx.send(());
        });
        rx.recv_timeout(Duration::from_secs(2))
            .expect("watchdog thread should exit after shutdown");
    }
}
