//! Process-wide application state: vault handle, OCR model paths, idle lock.
//!
//! # Locks
//!
//! The state holds five mutexes:
//!
//! 1. `GatedVault::vault`, the vault. Held for one vault operation (a query,
//!    an unlock with its key derivation, a rekey).
//! 2. `WatchdogGate::state`, whether the idle watchdog polls or parks. Held
//!    for a field update, or handed to a condition-variable wait.
//! 3. `AppState::granted_paths`, the paths the user handed over. Held for one
//!    lookup or insert.
//! 4. `AppState::prefs_lock`, which serializes a load-change-save of the
//!    plaintext preferences file. Held across that file I/O.
//! 5. `AppState::update`, the update machine. Held for a status change, and
//!    by an install for its whole download.
//!
//! ## Order
//!
//! Only one pair is ever held together: the vault first, then the gate state.
//! [`VaultGuard`] updates the gate as it is dropped, while it still holds the
//! vault, so the gate always shows the status the vault was left in and no
//! other status change can come between the two writes. The opposite order
//! does not occur: the watchdog takes the gate state only inside
//! [`WatchdogGate`] methods, each of which releases it before returning, so
//! it never holds the gate state while it waits for the vault. Every other
//! mutex is taken with no other mutex held.
//!
//! ## Poisoning
//!
//! No mutex stays poisoned. Each lock helper clears the flag and hands out
//! the guard, so one panicking command cannot disable the rest of the
//! session. For the gate state, the granted paths, the preferences lock and
//! the update machine that is all: each holds plain values that are valid
//! whichever statement the panic interrupted. The vault is different, because
//! a panic may have interrupted a database operation: [`GatedVault::lock`]
//! also locks the vault, which closes the connection, and has the watchdog
//! tell the UI.

use std::collections::HashSet;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use oikonomia_core::error::Error as CoreError;
use oikonomia_core::ledger::{DEFAULT_LOCK_TIMEOUT_SECS, get_lock_timeout_secs};
use oikonomia_core::vault::{Vault, VaultStatus, default_data_dir};
use oikonomia_update::UpdateMachine;

/// Shared state behind Tauri commands.
pub struct AppState {
    /// The vault, with the watchdog gate and timeout that follow its status.
    vault: Arc<GatedVault>,
    /// App data directory: vault files plus the plaintext UI prefs.
    data_dir: PathBuf,
    /// Directory containing bundled `text-detection.rten` + `text-recognition.rten`.
    ocr_model_dir: PathBuf,
    /// Seconds since `UNIX_EPOCH` of the last command touching the vault.
    last_activity: Arc<AtomicU64>,
    /// Canonical paths the user handed over through a native drop or a native
    /// file dialog; path-taking IPC commands accept only these.
    granted_paths: Mutex<HashSet<PathBuf>>,
    /// Serializes plaintext prefs load-mutate-save.
    prefs_lock: Mutex<()>,
    /// Unlock-screen update check / install machine. Independent of the vault.
    update: Arc<Mutex<UpdateMachine>>,
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
            vault: Arc::new(GatedVault::new(vault)),
            data_dir,
            ocr_model_dir,
            last_activity: Arc::new(AtomicU64::new(now_secs())),
            granted_paths: Mutex::new(HashSet::new()),
            prefs_lock: Mutex::new(()),
            update: Arc::new(Mutex::new(UpdateMachine::new())),
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
    pub fn vault(&self) -> Arc<GatedVault> {
        Arc::clone(&self.vault)
    }

    /// Shared update machine. Never awaited from `vault_unlock`.
    #[must_use]
    pub fn update_machine(&self) -> Arc<Mutex<UpdateMachine>> {
        Arc::clone(&self.update)
    }

    /// Hold across a prefs load-mutate-save so locale and tray last-used cannot clobber.
    pub fn lock_prefs(&self) -> std::sync::MutexGuard<'_, ()> {
        match self.prefs_lock.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                self.prefs_lock.clear_poison();
                poisoned.into_inner()
            }
        }
    }

    /// Record paths the user chose through a native drop or dialog, so a
    /// later path-taking command may accept them.
    pub fn grant_paths(&self, paths: impl IntoIterator<Item = PathBuf>) {
        let mut granted = self.lock_granted_paths();
        for path in paths {
            if let Ok(canonical) = path.canonicalize() {
                granted.insert(canonical);
            }
        }
    }

    /// True when `path` resolves to a granted path.
    #[must_use]
    pub fn path_is_granted(&self, path: &Path) -> bool {
        let granted = self.lock_granted_paths();
        path.canonicalize()
            .is_ok_and(|canonical| granted.contains(&canonical))
    }

    fn lock_granted_paths(&self) -> std::sync::MutexGuard<'_, HashSet<PathBuf>> {
        match self.granted_paths.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                self.granted_paths.clear_poison();
                poisoned.into_inner()
            }
        }
    }

    /// Record command activity for the idle watchdog.
    pub fn touch(&self) {
        self.last_activity.store(now_secs(), Ordering::Relaxed);
    }

    /// Cache the idle timeout so the watchdog needs no database access.
    pub fn set_lock_timeout_cache(&self, secs: u64) {
        self.vault.set_lock_timeout_cache(secs);
    }

    /// Handles for the idle watchdog thread.
    #[must_use]
    pub fn watchdog_handles(&self) -> WatchdogHandles {
        WatchdogHandles {
            vault: Arc::clone(&self.vault),
            last_activity: Arc::clone(&self.last_activity),
        }
    }
}

/// Seconds since `UNIX_EPOCH` by the wall clock; zero for a clock set before it.
///
/// Idle time is measured on the wall clock on purpose. A monotonic clock does
/// not count the time the machine sleeps (`std::time::Instant` is
/// `CLOCK_MONOTONIC` on Linux and `CLOCK_UPTIME_RAW` on macOS), and a session
/// left open over a sleep has to lock when the machine wakes. The price is
/// that the wall clock can be set back, which [`idle_secs`] accounts for.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since_epoch| since_epoch.as_secs())
}

/// Seconds since the last activity, or `None` when the clock reads earlier
/// than that activity.
///
/// `None` means the clock was set back and the real idle time is unknown.
/// Clamping it to zero would restart the idle period at every backward step
/// and postpone the lock, so callers treat it as idle for long enough.
const fn idle_secs(now_secs: u64, last_activity_secs: u64) -> Option<u64> {
    now_secs.checked_sub(last_activity_secs)
}

/// The vault behind its mutex, with the watchdog state derived from it.
///
/// The gate and the cached timeout are functions of the vault: they are
/// written only by [`VaultGuard`] while it holds the vault, so no caller can
/// change the vault's status and forget them, and two status changes cannot
/// apply their gate updates in the opposite order.
pub struct GatedVault {
    /// The vault. Locked only through [`Self::lock`].
    vault: Mutex<Vault>,
    /// Parks the idle watchdog while the vault is not unlocked.
    gate: WatchdogGate,
    /// Cached idle timeout in seconds; the persisted value lives in the vault.
    lock_timeout_secs: AtomicU64,
}

impl GatedVault {
    /// Wraps a vault that is not unlocked, as every vault is when the process starts.
    fn new(vault: Vault) -> Self {
        Self {
            vault: Mutex::new(vault),
            gate: WatchdogGate::new(),
            lock_timeout_secs: AtomicU64::new(DEFAULT_LOCK_TIMEOUT_SECS),
        }
    }

    /// Locks the vault mutex, recovering from poisoning.
    ///
    /// A panic inside a command closure must not brick the session or disable
    /// the watchdog. The guard is recovered and the poison flag cleared, and
    /// the vault is forced into the locked state, because the panic may have
    /// interrupted a database operation. If that ended an unlocked session
    /// the watchdog emits `vault-locked`, as no command is there to do it.
    ///
    /// Blocks while another thread holds the vault, which a rekey does for
    /// seconds: call it on the blocking pool, not on an async worker.
    pub fn lock(&self) -> VaultGuard<'_> {
        let vault = match self.vault.lock() {
            Ok(vault) => vault,
            Err(poisoned) => {
                self.vault.clear_poison();
                let mut vault = poisoned.into_inner();

                let was_unlocked = vault.status() == VaultStatus::Unlocked;
                vault.lock();
                if was_unlocked {
                    self.gate.announce_lock();
                }
                vault
            }
        };

        VaultGuard {
            status_at_lock: vault.status(),
            vault,
            gated: self,
        }
    }

    /// Caches the idle timeout so the watchdog needs no database access.
    pub fn set_lock_timeout_cache(&self, secs: u64) {
        self.lock_timeout_secs.store(secs, Ordering::Relaxed);
    }

    /// Reads the stored idle timeout into the cache after an unlock.
    fn refresh_lock_timeout_cache(&self, vault: &Vault) {
        // A failed read falls back to the default, but never silently.
        let secs = match vault.connection().and_then(get_lock_timeout_secs) {
            Ok(secs) => secs,
            Err(err) => {
                log::warn!("could not read lock timeout after unlock, using default: {err}");
                DEFAULT_LOCK_TIMEOUT_SECS
            }
        };
        self.set_lock_timeout_cache(secs);
    }
}

/// Exclusive access to the vault.
///
/// Dropping it brings the watchdog in line with the status the vault was left
/// in, before the vault mutex is released.
#[must_use = "the vault unlocks for other threads as soon as the guard is dropped"]
pub struct VaultGuard<'a> {
    /// The locked vault.
    vault: MutexGuard<'a, Vault>,
    /// Owner of the gate and the timeout cache this guard keeps in step.
    gated: &'a GatedVault,
    /// Status when the guard was taken, to tell an unlock from a vault that
    /// was already open.
    status_at_lock: VaultStatus,
}

impl Deref for VaultGuard<'_> {
    type Target = Vault;

    fn deref(&self) -> &Vault {
        &self.vault
    }
}

impl DerefMut for VaultGuard<'_> {
    fn deref_mut(&mut self) -> &mut Vault {
        &mut self.vault
    }
}

impl Drop for VaultGuard<'_> {
    fn drop(&mut self) {
        let status = self.vault.status();
        let unlocked_under_this_guard =
            status == VaultStatus::Unlocked && self.status_at_lock != VaultStatus::Unlocked;

        // Not while unwinding: the next `GatedVault::lock` locks the vault
        // again, and a query that panicked here would abort the process.
        if unlocked_under_this_guard && !std::thread::panicking() {
            self.gated.refresh_lock_timeout_cache(&self.vault);
        }

        // Runs before the `vault` field is dropped, so the vault mutex is
        // still held: lock order vault, then gate state.
        self.gated.gate.follow(status);
    }
}

/// How often the watchdog re-checks idle time while the vault is unlocked.
pub const AUTO_LOCK_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Shared handles for [`spawn_auto_lock`].
pub struct WatchdogHandles {
    /// The vault, which the watchdog locks only for a status check / idle
    /// lock, and whose gate it waits on.
    pub vault: Arc<GatedVault>,
    /// Seconds since `UNIX_EPOCH` of the last command touching the vault.
    pub last_activity: Arc<AtomicU64>,
}

/// Gate that parks the idle watchdog while the vault is not unlocked.
///
/// [`VaultGuard`] moves the gate to match the vault and notifies on a change:
/// an unlock wakes the parked thread, a lock ends an in-flight poll wait at
/// once. The watchdog never holds the vault mutex across a wait.
pub struct WatchdogGate {
    /// What the watchdog should be doing.
    state: Mutex<GateState>,
    /// Notified whenever `state` changes.
    cond: Condvar,
    /// Completed unlocked poll intervals. Stays zero while the thread is parked.
    #[cfg(test)]
    ticks: AtomicU64,
}

/// What the gate tells the watchdog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GateState {
    /// Whether the watchdog polls or waits.
    phase: GatePhase,
    /// A lock the watchdog still has to report to the UI: the one forced by
    /// poison recovery, which happens where no `AppHandle` is at hand.
    lock_to_announce: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GatePhase {
    /// The vault is not unlocked; nothing to poll for.
    Parked,
    /// The vault is unlocked; check idle time every poll interval.
    Running,
    /// The loop should end.
    Shutdown,
}

/// Why [`WatchdogGate::wait_for_work`] returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wake {
    /// The vault is unlocked: start polling.
    Poll,
    /// The vault was locked behind the UI's back: emit `vault-locked`.
    AnnounceLock,
    /// The loop should end.
    Shutdown,
}

impl WatchdogGate {
    /// Start parked: the process launches with the vault locked or missing.
    fn new() -> Self {
        Self {
            state: Mutex::new(GateState {
                phase: GatePhase::Parked,
                lock_to_announce: false,
            }),
            cond: Condvar::new(),
            #[cfg(test)]
            ticks: AtomicU64::new(0),
        }
    }

    fn lock_state(&self) -> MutexGuard<'_, GateState> {
        match self.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                self.state.clear_poison();
                poisoned.into_inner()
            }
        }
    }

    fn wait<'a>(&self, guard: MutexGuard<'a, GateState>) -> MutexGuard<'a, GateState> {
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
        guard: MutexGuard<'a, GateState>,
        interval: Duration,
    ) -> MutexGuard<'a, GateState> {
        match self.cond.wait_timeout(guard, interval) {
            Ok((guard, _)) => guard,
            Err(poisoned) => {
                self.state.clear_poison();
                poisoned.into_inner().0
            }
        }
    }

    /// Runs the watchdog for an unlocked vault and parks it for any other.
    ///
    /// Called by [`VaultGuard`] with the vault mutex held. Notifies only on a
    /// change, so ordinary commands do not cut the poll wait short.
    fn follow(&self, status: VaultStatus) {
        let phase = if status == VaultStatus::Unlocked {
            GatePhase::Running
        } else {
            GatePhase::Parked
        };

        let mut state = self.lock_state();
        if state.phase == GatePhase::Shutdown || state.phase == phase {
            return;
        }
        state.phase = phase;
        self.cond.notify_all();
    }

    /// Has the watchdog emit `vault-locked` once.
    fn announce_lock(&self) {
        let mut state = self.lock_state();
        state.lock_to_announce = true;
        self.cond.notify_all();
    }

    /// Wake any waiter and stop the loop. Process exit also ends the thread.
    #[cfg(test)]
    fn shutdown(&self) {
        let mut state = self.lock_state();
        state.phase = GatePhase::Shutdown;
        self.cond.notify_all();
    }

    /// Waits while parked with nothing to announce.
    fn wait_for_work(&self) -> Wake {
        let mut state = self.lock_state();
        loop {
            if state.phase == GatePhase::Shutdown {
                return Wake::Shutdown;
            }
            if state.lock_to_announce {
                state.lock_to_announce = false;
                return Wake::AnnounceLock;
            }
            if state.phase == GatePhase::Running {
                return Wake::Poll;
            }
            state = self.wait(state);
        }
    }

    /// Waits one poll interval, or less if the gate changes, and returns the
    /// phase to act on.
    fn wait_poll_interval(&self, interval: Duration) -> GatePhase {
        let state = self.lock_state();
        if state.phase != GatePhase::Running || state.lock_to_announce {
            return state.phase;
        }
        let state = self.wait_timeout(state, interval);
        state.phase
    }

    #[cfg(test)]
    fn is_running(&self) -> bool {
        self.lock_state().phase == GatePhase::Running
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

/// Lock the update machine, recovering from poisoning.
pub fn lock_update(machine: &Mutex<UpdateMachine>) -> std::sync::MutexGuard<'_, UpdateMachine> {
    match machine.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            machine.clear_poison();
            poisoned.into_inner()
        }
    }
}

/// Lock iff the vault is unlocked and idle time has reached the timeout.
///
/// `idle_secs` is `None` when the idle time is unknown because the clock was
/// set back ([`idle_secs`]); an unlocked vault is then locked at once.
///
/// Pure predicate for the idle watchdog. Tests cover the four launch cases
/// without sleeping the 5s loop or constructing an `AppHandle`.
#[must_use]
pub const fn should_auto_lock(
    idle_secs: Option<u64>,
    timeout_secs: u64,
    status: VaultStatus,
) -> bool {
    // `matches!` is const; derived `PartialEq` on `VaultStatus` is not.
    if !matches!(status, VaultStatus::Unlocked) {
        return false;
    }

    match idle_secs {
        Some(idle_secs) => idle_secs >= timeout_secs,
        None => true,
    }
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

/// Idle watchdog loop. `on_locked` runs with no mutex held, each time the
/// watchdog locked the vault or has a forced lock to announce.
fn run_auto_lock_loop(
    handles: WatchdogHandles,
    poll_interval: Duration,
    mut on_locked: impl FnMut(),
) {
    let WatchdogHandles {
        vault,
        last_activity,
    } = handles;

    loop {
        match vault.gate.wait_for_work() {
            Wake::Shutdown => return,
            Wake::AnnounceLock => {
                on_locked();
                continue;
            }
            Wake::Poll => {}
        }

        match vault.gate.wait_poll_interval(poll_interval) {
            GatePhase::Shutdown => return,
            GatePhase::Parked => continue,
            GatePhase::Running => {}
        }

        #[cfg(test)]
        vault.gate.record_tick();

        let locked_now = lock_if_idle(&vault, &last_activity);

        if should_emit_vault_locked(locked_now) {
            on_locked();
        }
    }
}

/// Locks the vault if it is unlocked and has been idle for the timeout.
/// Returns whether it did. The guard parks the gate as it drops.
fn lock_if_idle(vault: &GatedVault, last_activity: &AtomicU64) -> bool {
    let mut guard = vault.lock();

    // Sampled with the vault held, so the decision uses the activity and the
    // timeout as they are once every earlier vault operation has finished,
    // not as they were before this thread waited for the mutex.
    let idle = idle_secs(now_secs(), last_activity.load(Ordering::Relaxed));
    let timeout = vault.lock_timeout_secs.load(Ordering::Relaxed);

    let lock_now = should_auto_lock(idle, timeout, guard.status());
    if lock_now {
        guard.lock();
    }
    lock_now
}

/// Resolve OCR model directory for dev and packaged builds.
#[must_use]
pub fn resolve_ocr_model_dir(resource_dir: Option<PathBuf>) -> PathBuf {
    // Dev layout: apps/desktop/src-tauri/resources/ocr. Also the answer when
    // no candidate holds the models, so the caller always gets a path.
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/ocr");
    let beside_executable = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("resources/ocr")));

    let candidates = ocr_model_dir_candidates(resource_dir, &dev, beside_executable);
    first_dir_with_models(candidates).unwrap_or(dev)
}

/// Where the OCR models may be, most specific first.
///
/// The Tauri resource directory (packaged) comes first. The bundle keeps the
/// `resources/ocr/*` path from tauri.conf.json, so on macOS the models sit
/// in Contents/Resources/resources/ocr; without that entry the lookup fell
/// through to the compile-time dev path, which exists only on the machine
/// that built the app. Some bundles put resources under `ocr` or flat.
fn ocr_model_dir_candidates(
    resource_dir: Option<PathBuf>,
    dev: &Path,
    beside_executable: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(dir) = resource_dir {
        candidates.push(dir.join("resources").join("ocr"));
        candidates.push(dir.join("ocr"));
        candidates.push(dir);
    }
    candidates.push(dev.to_path_buf());
    candidates.extend(beside_executable);

    candidates
}

/// The first directory that holds the text-detection model.
fn first_dir_with_models(candidates: Vec<PathBuf>) -> Option<PathBuf> {
    candidates
        .into_iter()
        .find(|dir| dir.join("text-detection.rten").is_file())
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
mod tests {
    use super::{
        AUTO_LOCK_POLL_INTERVAL, AppState, GatedVault, idle_secs, resolve_ocr_model_dir,
        run_auto_lock_loop, should_auto_lock, should_emit_vault_locked,
    };
    use oikonomia_core::ledger::set_lock_timeout_secs;
    use oikonomia_core::vault::VaultStatus;
    use std::fs;
    use std::sync::Arc;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    const TEST_PASSWORD: &str = "correct horse battery staple";
    /// Longer than [`AUTO_LOCK_POLL_INTERVAL`] so a still-polling thread would tick.
    const PAST_POLL: Duration = Duration::from_millis(5_500);

    #[test]
    fn packaged_ocr_models_resolve_from_the_bundle_resources() {
        // tauri.conf.json bundles `resources/ocr/*`, which keeps that relative
        // path: on macOS the models land in Contents/Resources/resources/ocr.
        let resource_dir = std::env::temp_dir().join(format!(
            "oiko-ocr-bundle-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        let models = resource_dir.join("resources").join("ocr");
        fs::create_dir_all(&models).expect("models dir");
        fs::write(models.join("text-detection.rten"), b"model").expect("write model");

        assert_eq!(resolve_ocr_model_dir(Some(resource_dir.clone())), models);
        let _ = fs::remove_dir_all(&resource_dir);
    }

    #[test]
    fn ocr_model_candidates_put_the_packaged_layouts_before_the_dev_tree() {
        let resources = std::path::PathBuf::from("bundle");
        let dev = std::path::PathBuf::from("dev");
        let beside = std::path::PathBuf::from("beside");

        assert_eq!(
            super::ocr_model_dir_candidates(Some(resources.clone()), &dev, Some(beside.clone())),
            [
                resources.join("resources").join("ocr"),
                resources.join("ocr"),
                resources,
                dev.clone(),
                beside,
            ]
        );
        assert_eq!(
            super::ocr_model_dir_candidates(None, &dev, None),
            std::slice::from_ref(&dev)
        );
    }

    #[test]
    fn the_first_directory_holding_the_model_wins() {
        let (_state, dir) = test_state("ocr-pick");
        let empty = dir.join("empty");
        let first = dir.join("first");
        let second = dir.join("second");
        for holder in [&first, &second] {
            fs::create_dir_all(holder).expect("dir");
            fs::write(holder.join("text-detection.rten"), b"model").expect("model");
        }
        fs::create_dir_all(&empty).expect("dir");
        // A directory named like the model is not the model.
        let decoy = dir.join("decoy");
        fs::create_dir_all(decoy.join("text-detection.rten")).expect("decoy");

        let picked =
            super::first_dir_with_models(vec![empty.clone(), decoy.clone(), first.clone(), second]);

        assert_eq!(picked, Some(first));
        assert_eq!(super::first_dir_with_models(vec![empty, decoy]), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn packaged_ocr_models_under_ocr_or_flat_are_found() {
        for layout in ["ocr", ""] {
            let (_state, resource_dir) = test_state("ocr-layout");
            let models = resource_dir.join(layout);
            fs::create_dir_all(&models).expect("models dir");
            fs::write(models.join("text-detection.rten"), b"model").expect("write model");

            assert_eq!(
                resolve_ocr_model_dir(Some(resource_dir.clone())),
                models,
                "layout {layout:?}"
            );
            let _ = fs::remove_dir_all(&resource_dir);
        }
    }

    #[test]
    fn granted_paths_match_after_canonicalization() {
        let (state, dir) = test_state("grants");
        let picked = dir.join("picked.csv");
        fs::write(&picked, b"date,amount\n").expect("write");
        fs::create_dir_all(dir.join("sub")).expect("subdir");
        let indirect = dir.join("sub").join("..").join("picked.csv");

        assert!(!state.path_is_granted(&picked), "nothing granted yet");

        state.grant_paths([picked.clone()]);

        assert!(state.path_is_granted(&picked));
        assert!(state.path_is_granted(&indirect), "same file through ..");
        assert!(!state.path_is_granted(&dir.join("other.csv")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn auto_lock_when_unlocked_and_idle_meets_timeout() {
        assert!(should_auto_lock(Some(60), 60, VaultStatus::Unlocked));
        assert!(should_auto_lock(Some(61), 60, VaultStatus::Unlocked));
        assert!(
            should_emit_vault_locked(true),
            "a successful idle lock would emit vault-locked"
        );
    }

    #[test]
    fn no_auto_lock_when_idle_below_timeout() {
        assert!(!should_auto_lock(Some(59), 60, VaultStatus::Unlocked));
        assert!(!should_emit_vault_locked(false));
    }

    #[test]
    fn no_auto_lock_after_activity_resets_idle() {
        assert!(!should_auto_lock(Some(0), 60, VaultStatus::Unlocked));
        assert!(!should_emit_vault_locked(false));
    }

    #[test]
    fn no_auto_lock_when_already_locked() {
        assert!(!should_auto_lock(Some(120), 60, VaultStatus::Locked));
        assert!(!should_auto_lock(Some(120), 60, VaultStatus::Uninitialized));
        assert!(!should_auto_lock(None, 60, VaultStatus::Locked));
        assert!(
            !should_emit_vault_locked(false),
            "already locked must not emit vault-locked"
        );
    }

    #[test]
    fn a_clock_set_back_before_the_last_activity_locks_at_once() {
        assert_eq!(idle_secs(100, 40), Some(60));
        assert_eq!(idle_secs(100, 100), Some(0));
        assert_eq!(idle_secs(99, 100), None);
        // A clock before the epoch reads as zero.
        assert_eq!(idle_secs(0, 1_700_000_000), None);

        assert!(should_auto_lock(None, 60, VaultStatus::Unlocked));
        assert!(should_auto_lock(None, u64::MAX, VaultStatus::Unlocked));
    }

    #[test]
    fn watchdog_locks_an_unlocked_vault_whose_last_activity_is_in_the_future() {
        let (state, dir) = test_state("clock");
        init_locked_vault(&state);
        let handles = state.watchdog_handles();
        let vault = Arc::clone(&handles.vault);
        let (join, emits) = spawn_watchdog(&state, Duration::from_millis(20));

        // What the watchdog sees after the clock is set back: the last
        // activity lies ahead of now.
        handles.last_activity.store(u64::MAX, Ordering::Relaxed);
        vault.lock().unlock(TEST_PASSWORD).expect("unlock");

        // The bound only fails the test if no lock ever comes.
        let locked = emits.recv_timeout(Duration::from_secs(5));
        shutdown_watchdog(&vault, join);
        locked.expect("activity in the future must lock at once");
        assert_eq!(vault.lock().status(), VaultStatus::Locked);
        let _ = fs::remove_dir_all(&dir);
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
        let vault = Arc::clone(&handles.vault);
        let last_activity = Arc::clone(&handles.last_activity);
        let (join, emits) = spawn_watchdog(&state, AUTO_LOCK_POLL_INTERVAL);

        std::thread::sleep(PAST_POLL);
        assert_eq!(
            vault.gate.tick_count(),
            0,
            "locked watchdog must not complete a 5s poll tick"
        );
        assert_eq!(vault.lock().status(), VaultStatus::Locked);

        vault.lock().unlock(TEST_PASSWORD).expect("unlock");
        last_activity.store(0, Ordering::Relaxed);
        state.set_lock_timeout_cache(60);

        std::thread::sleep(PAST_POLL);
        assert!(
            vault.gate.tick_count() >= 1,
            "unlock must unpark and run an idle check"
        );
        assert_eq!(
            vault.lock().status(),
            VaultStatus::Locked,
            "idle unlocked session must still auto-lock"
        );
        emits
            .recv_timeout(Duration::from_millis(200))
            .expect("auto-lock emits vault-locked");

        let ticks_after_lock = vault.gate.tick_count();
        std::thread::sleep(PAST_POLL);
        assert_eq!(
            vault.gate.tick_count(),
            ticks_after_lock,
            "auto-lock must park; no further 5s ticks"
        );

        shutdown_watchdog(&vault, join);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn watchdog_lock_parks_before_poll_wait_finishes() {
        let (state, dir) = test_state("inflight");
        init_locked_vault(&state);
        let vault = state.vault();
        vault.lock().unlock(TEST_PASSWORD).expect("unlock");
        state.touch();
        state.set_lock_timeout_cache(15 * 60);

        let (join, _emits) = spawn_watchdog(&state, AUTO_LOCK_POLL_INTERVAL);

        std::thread::sleep(Duration::from_millis(200));
        vault.lock().lock();

        std::thread::sleep(PAST_POLL);
        assert_eq!(
            vault.gate.tick_count(),
            0,
            "lock must park before the unlocked poll wait completes"
        );

        shutdown_watchdog(&vault, join);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_gate_runs_exactly_while_the_vault_is_unlocked() {
        let (state, dir) = test_state("follow");
        let vault = state.vault();
        assert!(!vault.gate.is_running(), "no vault yet");

        vault.lock().init(TEST_PASSWORD).expect("init");
        assert!(vault.gate.is_running(), "init leaves the vault unlocked");

        vault.lock().lock();
        assert!(!vault.gate.is_running());

        let wrong = vault.lock().unlock("not the password at all");
        assert!(wrong.is_err());
        assert!(!vault.gate.is_running(), "a refused unlock changes nothing");

        vault.lock().unlock(TEST_PASSWORD).expect("unlock");
        assert!(vault.gate.is_running());
        let _ = fs::remove_dir_all(&dir);
    }

    /// The race this guards against: a lock and an unlock overlap, and the
    /// lock's watchdog update is applied after the unlock's, parking the
    /// watchdog on an unlocked vault. With the gate written under the vault
    /// guard, the unlock cannot start before the lock's update is done.
    #[test]
    fn a_lock_that_overlaps_a_later_unlock_cannot_park_the_watchdog_after_it() {
        let (state, dir) = test_state("overlap");
        init_locked_vault(&state);
        let vault = state.vault();
        vault.lock().unlock(TEST_PASSWORD).expect("unlock");
        let (holding, is_holding) = mpsc::channel();
        let (release, released) = mpsc::channel::<()>();
        let shared: &GatedVault = &vault;

        std::thread::scope(|scope| {
            scope.spawn(move || {
                let mut guard = shared.lock();
                guard.lock();
                let _ = holding.send(());
                // Keep the vault until the unlock has been started.
                let _ = released.recv();
            });
            is_holding.recv().expect("the lock holds the vault");

            let unlock = scope.spawn(move || shared.lock().unlock(TEST_PASSWORD));
            release.send(()).expect("the lock is still waiting");

            unlock.join().expect("unlock thread").expect("unlock");
        });

        assert_eq!(vault.lock().status(), VaultStatus::Unlocked);
        assert!(
            vault.gate.is_running(),
            "vault unlocked but watchdog parked"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_poisoned_vault_is_locked_parked_and_announced_to_the_ui() {
        let (state, dir) = test_state("poison");
        init_locked_vault(&state);
        let vault = state.vault();
        vault.lock().unlock(TEST_PASSWORD).expect("unlock");
        state.touch();
        let (join, emits) = spawn_watchdog(&state, AUTO_LOCK_POLL_INTERVAL);

        let poisoner = Arc::clone(&vault);
        let panicked = std::thread::spawn(move || {
            let _guard = poisoner.lock();
            std::panic::resume_unwind(Box::new("a command panicked"));
        })
        .join();
        assert!(panicked.is_err());

        assert_eq!(vault.lock().status(), VaultStatus::Locked);
        assert!(
            !vault.gate.is_running(),
            "vault force-locked but watchdog running"
        );
        emits
            .recv_timeout(Duration::from_secs(5))
            .expect("the forced lock emits vault-locked");

        shutdown_watchdog(&vault, join);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_password_change_leaves_the_gate_and_timeout_matching_the_vault() {
        let (state, dir) = test_state("rekey");
        let vault = state.vault();
        {
            let mut guard = vault.lock();
            guard.init(TEST_PASSWORD).expect("init");
            set_lock_timeout_secs(guard.connection().expect("connection"), 120).expect("store");
            guard.lock();
        }
        state.set_lock_timeout_cache(7);

        vault
            .lock()
            .change_password(TEST_PASSWORD, "another long password")
            .expect("change");

        // Whether a change on a locked vault unlocks it is core's decision;
        // the watchdog has to agree with the outcome either way.
        let status = vault.lock().status();
        assert_eq!(
            vault.gate.is_running(),
            status == VaultStatus::Unlocked,
            "{status:?}"
        );
        if status == VaultStatus::Unlocked {
            assert_eq!(vault.lock_timeout_secs.load(Ordering::Relaxed), 120);
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unlock_reads_the_stored_timeout_into_the_cache() {
        let (state, dir) = test_state("timeout");
        let vault = state.vault();
        {
            let mut guard = vault.lock();
            guard.init(TEST_PASSWORD).expect("init");
            set_lock_timeout_secs(guard.connection().expect("connection"), 120).expect("store");
            guard.lock();
        }
        state.set_lock_timeout_cache(7);

        vault.lock().unlock(TEST_PASSWORD).expect("unlock");

        assert_eq!(vault.lock_timeout_secs.load(Ordering::Relaxed), 120);
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
        let vault = state.vault();
        let mut guard = vault.lock();
        guard.init(TEST_PASSWORD).expect("vault init");
        guard.lock();
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

    fn shutdown_watchdog(vault: &GatedVault, join: std::thread::JoinHandle<()>) {
        vault.gate.shutdown();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = join.join();
            let _ = tx.send(());
        });
        rx.recv_timeout(Duration::from_secs(2))
            .expect("watchdog thread should exit after shutdown");
    }
}
