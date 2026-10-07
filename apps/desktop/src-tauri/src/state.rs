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
//! 3. `PathGrants::paths`, the paths the user handed over, each under the
//!    purpose it was handed over for ([`GrantPurpose`]). Held for one lookup
//!    or insert.
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
//! other status change can come between the two writes. A lock made with no
//! `AppHandle` at hand is queued on the gate in the same order
//! (`WatchdogGate::announce_lock`). The opposite order
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
//! a panic may have interrupted a database operation: [`GatedVault::acquire`]
//! also locks the vault, which closes the connection, and has the watchdog
//! tell the UI.

use oikonomia_core::error::Error as CoreError;
use oikonomia_core::ledger::{DEFAULT_LOCK_TIMEOUT_SECS, get_lock_timeout_secs};
use oikonomia_core::vault::{Vault, VaultStatus};
use oikonomia_update::UpdateMachine;
use std::collections::HashSet;
use std::ops::{ControlFlow, Deref, DerefMut};
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How often the watchdog re-checks idle time while the vault is unlocked.
pub(crate) const AUTO_LOCK_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// The state shared by every Tauri command.
///
/// Tauri manages it from the end of a successful start (`start` in `lib.rs`); a command
/// borrows it for the length of the call. What a blocking task needs from it
/// is handed out as an owned handle ([`Self::vault`], [`Self::path_grants`],
/// [`Self::update_machine`]).
pub(crate) struct AppState {
    /// The vault, with the watchdog gate and timeout that follow its status.
    vault: Arc<GatedVault>,
    /// The app data directory: the vault files plus the plaintext preferences.
    data_dir: PathBuf,
    /// The directory that holds the bundled OCR models, `text-detection.rten`
    /// and `text-recognition.rten`.
    ocr_model_dir: PathBuf,
    /// Seconds since `UNIX_EPOCH` of the last command touching the vault.
    last_activity: Arc<AtomicU64>,
    /// The paths the user handed over through a native drop or a native file
    /// dialog; a path-taking IPC command accepts only those handed over for
    /// its own purpose.
    path_grants: PathGrants,
    /// Serializes a load-change-save of the plaintext preferences file.
    prefs_lock: Mutex<()>,
    /// The update check and install machine. Independent of the vault.
    update: Arc<Mutex<UpdateMachine>>,
}

impl AppState {
    /// Opens the vault in `data_dir`, without unlocking it. `ocr_model_dir`
    /// is where the bundled OCR models were found.
    ///
    /// # Errors
    ///
    /// Returns core's error when the directory cannot be created or read, or
    /// the vault header in it cannot be parsed.
    pub(crate) fn open_path(data_dir: PathBuf, ocr_model_dir: PathBuf) -> Result<Self, CoreError> {
        let vault = Vault::open_path(data_dir.clone())?;
        Ok(Self {
            vault: Arc::new(GatedVault::new(vault)),
            data_dir,
            ocr_model_dir,
            last_activity: Arc::new(AtomicU64::new(now_secs())),
            path_grants: PathGrants::default(),
            prefs_lock: Mutex::new(()),
            update: Arc::new(Mutex::new(UpdateMachine::new())),
        })
    }

    /// Returns the app data directory, where the vault files and the plaintext
    /// preferences live.
    #[must_use]
    pub(crate) fn data_dir(&self) -> &std::path::Path {
        &self.data_dir
    }

    /// Returns the directory the bundled OCR models were looked for in.
    #[must_use]
    pub(crate) fn ocr_model_dir(&self) -> &PathBuf {
        &self.ocr_model_dir
    }

    /// Returns a shared handle to the vault, for work on the blocking pool.
    #[must_use]
    pub(crate) fn vault(&self) -> Arc<GatedVault> {
        Arc::clone(&self.vault)
    }

    /// Returns a shared handle to the update machine.
    ///
    /// The machine is independent of the vault: an unlock never waits on a check
    /// or an install.
    #[must_use]
    pub(crate) fn update_machine(&self) -> Arc<Mutex<UpdateMachine>> {
        Arc::clone(&self.update)
    }

    /// Locks the preferences file for a load-change-save.
    ///
    /// Held across the three steps, so that a language change and a quick-add
    /// post cannot each load the old file and write over the other's change.
    /// Recovers from poisoning: the mutex guards no data, only the order of file
    /// operations.
    pub(crate) fn lock_prefs(&self) -> MutexGuard<'_, ()> {
        match self.prefs_lock.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                self.prefs_lock.clear_poison();
                poisoned.into_inner()
            }
        }
    }

    /// Records paths the user chose through a native drop or dialog, so a
    /// later command that takes a path for `purpose` may accept them
    /// ([`PathGrants::grant`]).
    pub(crate) fn grant_paths(
        &self,
        purpose: GrantPurpose,
        paths: impl IntoIterator<Item = PathBuf>,
    ) {
        self.path_grants.grant(purpose, paths);
    }

    /// Returns a handle to the granted paths that a blocking task can own.
    #[must_use]
    pub(crate) fn path_grants(&self) -> PathGrants {
        self.path_grants.clone()
    }

    /// Records command activity for the idle watchdog.
    pub(crate) fn touch(&self) {
        self.last_activity.store(now_secs(), Ordering::Relaxed);
    }

    /// Returns the handles the idle watchdog thread runs on.
    #[must_use]
    pub(crate) fn watchdog_handles(&self) -> WatchdogHandles {
        WatchdogHandles {
            vault: Arc::clone(&self.vault),
            last_activity: Arc::clone(&self.last_activity),
        }
    }
}

/// What the user handed a path over for.
///
/// Each place that grants a path names one purpose, and each command that
/// takes a path asks for its own, so a file the user picked for one thing is
/// not accepted for another: a statement picked for a CSV import, or a file
/// dropped on a window, never reaches the restore that replaces the vault.
///
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum GrantPurpose {
    /// A backup archive to restore the vault from. Granted by the open
    /// dialog of `vault_pick_backup` and `vault_restore`; accepted by
    /// `vault_restore`.
    Backup,
    /// A bank statement to preview for import. Granted by the open dialog of
    /// `csv_import_preview`, and accepted by that command when the webview
    /// passes the path back with a column mapping.
    Csv,
    /// A document to analyze or to store with an entry. Granted by a file
    /// drop on a window; accepted by `document_analyze_path` and
    /// `entry_post_simple_with_document_path`.
    Document,
}

/// The paths the user handed over through a native drop or a native file
/// dialog, each with the purpose it was handed over for.
///
/// A handle: clones share one set. The Tauri state is borrowed for the length
/// of a command, so a task on the blocking pool cannot hold it; it holds a
/// clone of this instead.
///
/// Both methods resolve a path through the filesystem, which can block on a
/// slow or remote volume. Call them on the blocking pool or the main thread,
/// not on an async worker.
#[derive(Debug, Clone, Default)]
pub(crate) struct PathGrants {
    /// The granted paths, each with its links resolved, under the purpose
    /// each was granted for. One path may be held under several purposes.
    paths: Arc<Mutex<HashSet<(GrantPurpose, PathBuf)>>>,
}

impl PathGrants {
    /// Records `paths` as handed over for `purpose`, so a later command that
    /// takes a path for that purpose may accept them.
    ///
    /// A path that cannot be resolved, such as one that no longer exists, is
    /// skipped: there is nothing a command could open under it.
    pub(crate) fn grant(&self, purpose: GrantPurpose, paths: impl IntoIterator<Item = PathBuf>) {
        // Resolved before the set is locked, so the lock is never held across
        // file I/O.
        let resolved: Vec<(GrantPurpose, PathBuf)> = paths
            .into_iter()
            .filter_map(|path| path.canonicalize().ok())
            .map(|path| (purpose, path))
            .collect();

        self.lock().extend(resolved);
    }

    /// Returns the path granted for `purpose` that `path` resolves to, or
    /// `None` if it resolves to nothing the user handed over for that
    /// purpose. A grant for another purpose does not count.
    ///
    /// The result is the resolved path that was compared, and it is the one
    /// to open. Opening `path` itself would resolve its links a second time,
    /// and a link changed in between would lead to a file that was never
    /// checked. The resolved path is still opened by name, so this does not
    /// cover a directory on it being replaced after the check.
    #[must_use]
    pub(crate) fn resolve(&self, purpose: GrantPurpose, path: &Path) -> Option<PathBuf> {
        let wanted = (purpose, path.canonicalize().ok()?);
        let granted = self.lock();

        granted.contains(&wanted).then_some(wanted.1)
    }

    /// Locks the set, recovering from poisoning: it holds plain paths that
    /// are valid whichever statement a panic interrupted.
    fn lock(&self) -> MutexGuard<'_, HashSet<(GrantPurpose, PathBuf)>> {
        match self.paths.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                self.paths.clear_poison();
                poisoned.into_inner()
            }
        }
    }
}

/// Locks the update machine, recovering from poisoning.
///
/// The machine holds plain values that are valid whichever statement a panic
/// interrupted, so the guard is handed out and the flag cleared.
pub(crate) fn lock_update(machine: &Mutex<UpdateMachine>) -> MutexGuard<'_, UpdateMachine> {
    match machine.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            machine.clear_poison();
            poisoned.into_inner()
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
/// The gate and the cached timeout are functions of the vault. Outside the
/// tests they are set only with the vault mutex held: whether the watchdog
/// polls and the timeout by [`VaultGuard`], the queued `vault-locked`
/// announcement by the three places that lock the vault with no `AppHandle`
/// at hand ([`Self::acquire`] after poisoning, `lock_after_panic` and
/// `lock_if_idle`). So no caller can change the vault's status and forget
/// them, and two status changes cannot apply their gate updates in the
/// opposite order. The writes made without the vault are the watchdog's own
/// bookkeeping of a queued announcement, under the gate state alone: taking
/// an attempt at it, dropping it, and clearing it once it is made.
pub(crate) struct GatedVault {
    /// The vault. Locked only through [`Self::acquire`].
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
    /// the watchdog emits `vault-locked`, as no command is there to do it,
    /// unless the caller unlocks the vault again under the guard it gets.
    ///
    /// Blocks while another thread holds the vault, which a rekey does for
    /// seconds: call it on the blocking pool, not on an async worker.
    pub(crate) fn acquire(&self) -> VaultGuard<'_> {
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

    /// Reads the stored idle timeout into the cache after an unlock.
    fn refresh_lock_timeout_cache(&self, vault: &Vault) {
        // A failed read falls back to the default, but never silently.
        let timeout_secs = match vault.connection().and_then(get_lock_timeout_secs) {
            Ok(timeout_secs) => timeout_secs,
            Err(err) => {
                log::warn!("could not read lock timeout after unlock, using default: {err}");
                DEFAULT_LOCK_TIMEOUT_SECS
            }
        };
        self.lock_timeout_secs
            .store(timeout_secs, Ordering::Relaxed);
    }
}

/// Exclusive access to the vault.
///
/// Dropping it brings the watchdog in line with the status the vault was left
/// in, before the vault mutex is released.
#[must_use = "the vault mutex is released as soon as the guard is dropped"]
pub(crate) struct VaultGuard<'a> {
    /// The locked vault.
    vault: MutexGuard<'a, Vault>,
    /// Owner of the gate and the timeout cache this guard keeps in step.
    gated: &'a GatedVault,
    /// Status when the guard was taken, to tell an unlock from a vault that
    /// was already open.
    status_at_lock: VaultStatus,
}

impl VaultGuard<'_> {
    /// Caches the idle timeout so the watchdog needs no database access.
    ///
    /// On the guard so that the cache is written with the vault held, in the
    /// same order as the stored values it mirrors.
    pub(crate) fn set_lock_timeout_cache(&self, timeout_secs: u64) {
        self.gated
            .lock_timeout_secs
            .store(timeout_secs, Ordering::Relaxed);
    }
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

        // Runs before the `vault` field is dropped, so the vault mutex is
        // still held: lock order vault, then gate state. First, so that a
        // panic in the read below cannot leave an unlocked vault with a
        // parked watchdog.
        self.gated.gate.follow(status);

        // Not while unwinding: the next `GatedVault::acquire` locks the vault
        // again, and a query that panicked here would abort the process. The
        // watchdog cannot read the timeout before this, as it reads it with
        // the vault held.
        if unlocked_under_this_guard && !std::thread::panicking() {
            self.gated.refresh_lock_timeout_cache(&self.vault);
        }
    }
}

/// Shared handles for [`spawn_auto_lock`].
pub(crate) struct WatchdogHandles {
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
pub(crate) struct WatchdogGate {
    /// What the watchdog should be doing.
    state: Mutex<GateState>,
    /// Notified whenever `state` changes.
    cond: Condvar,
    /// Completed unlocked poll intervals. Stays zero while the thread is parked.
    #[cfg(test)]
    ticks: AtomicU64,
    /// Calls of [`Self::follow`], counted on entry, before it takes `state`.
    /// A test that holds `state` reads from it that a guard's drop has
    /// reached its gate update.
    #[cfg(test)]
    follow_entries: AtomicU64,
    /// Times the watchdog started the wait it parks in, counted with `state`
    /// held, just before the wait releases it.
    #[cfg(test)]
    parked_waits: AtomicU64,
    /// Times the watchdog started a poll-interval wait, counted the same way.
    #[cfg(test)]
    poll_waits: AtomicU64,
}

impl WatchdogGate {
    /// Creates a parked gate: the process starts with the vault locked or
    /// missing.
    fn new() -> Self {
        Self {
            state: Mutex::new(GateState {
                phase: GatePhase::Parked,
                lock_to_announce: None,
                locks_queued: 0,
            }),
            cond: Condvar::new(),
            #[cfg(test)]
            ticks: AtomicU64::new(0),
            #[cfg(test)]
            follow_entries: AtomicU64::new(0),
            #[cfg(test)]
            parked_waits: AtomicU64::new(0),
            #[cfg(test)]
            poll_waits: AtomicU64::new(0),
        }
    }

    /// Locks the gate state, recovering from poisoning: it holds plain values
    /// that are valid whichever statement a panic interrupted.
    fn lock_state(&self) -> MutexGuard<'_, GateState> {
        match self.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                self.state.clear_poison();
                poisoned.into_inner()
            }
        }
    }

    /// Waits on the condition variable until notified, recovering from poisoning
    /// as [`Self::lock_state`] does. May also return without a notification, as
    /// any condition-variable wait may; callers re-check the state.
    fn wait<'a>(&self, guard: MutexGuard<'a, GateState>) -> MutexGuard<'a, GateState> {
        match self.cond.wait(guard) {
            Ok(guard) => guard,
            Err(poisoned) => {
                self.state.clear_poison();
                poisoned.into_inner()
            }
        }
    }

    /// Waits on the condition variable for at most `interval`, recovering from
    /// poisoning as [`Self::lock_state`] does.
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
        #[cfg(test)]
        self.follow_entries.fetch_add(1, Ordering::SeqCst);

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

    /// Queues a `vault-locked` for the watchdog to emit: for a lock made
    /// where no `AppHandle` is at hand, and for the watchdog's own idle lock.
    ///
    /// The caller holds the vault mutex and has just locked the vault. The
    /// gate is parked here, not left for the caller's guard to park as it
    /// drops: a queued lock is dropped when the gate shows the vault
    /// unlocked ([`GateState::next_announcement`]), and until that guard
    /// drops the gate would still show the session that was just ended.
    ///
    /// The lock gets a fresh [`MAX_LOCK_ANNOUNCE_ATTEMPTS`]. It replaces a
    /// lock still queued: the event carries nothing, so one emission tells
    /// the UI of both.
    fn announce_lock(&self) {
        let mut state = self.lock_state();
        if state.phase == GatePhase::Running {
            state.phase = GatePhase::Parked;
        }
        state.locks_queued = state.locks_queued.wrapping_add(1);
        state.lock_to_announce = Some(LockAnnouncement {
            lock: state.locks_queued,
            attempts_left: MAX_LOCK_ANNOUNCE_ATTEMPTS,
        });
        self.cond.notify_all();
    }

    /// Records that the announcement of `lock` reached the emitter and came
    /// back, so it is not retried.
    ///
    /// A lock queued while that announcement was being emitted is a later
    /// one, which the UI may not have been told of, and stays queued.
    fn lock_announced(&self, lock: u64) {
        let mut state = self.lock_state();
        if state
            .lock_to_announce
            .is_some_and(|queued| queued.lock == lock)
        {
            state.lock_to_announce = None;
        }
    }

    /// Wakes any waiter and ends the loop.
    ///
    /// Only tests end the loop; in the app the thread runs until the process
    /// exits.
    #[cfg(test)]
    fn shutdown(&self) {
        let mut state = self.lock_state();
        state.phase = GatePhase::Shutdown;
        self.cond.notify_all();
    }

    /// Waits while the gate is parked with nothing to announce, and returns why
    /// it stopped waiting.
    fn wait_for_work(&self) -> Wake {
        let mut state = self.lock_state();
        loop {
            if state.phase == GatePhase::Shutdown {
                return Wake::Shutdown;
            }
            if let Some(lock) = state.next_announcement() {
                return Wake::AnnounceLock(lock);
            }
            if state.phase == GatePhase::Running {
                return Wake::Poll;
            }
            #[cfg(test)]
            self.parked_waits.fetch_add(1, Ordering::SeqCst);

            state = self.wait(state);
        }
    }

    /// Waits one poll interval, or less if the gate changes, and returns the
    /// phase to act on.
    ///
    /// A lock queued during the wait ends it early, as any notification
    /// does. The idle check that follows is then merely early, and the next
    /// round decides about the announcement.
    fn wait_poll_interval(&self, interval: Duration) -> GatePhase {
        let state = self.lock_state();
        if state.phase != GatePhase::Running {
            return state.phase;
        }
        #[cfg(test)]
        self.poll_waits.fetch_add(1, Ordering::SeqCst);

        let state = self.wait_timeout(state, interval);
        state.phase
    }

    /// Returns whether the gate has the watchdog polling.
    #[cfg(test)]
    fn is_running(&self) -> bool {
        self.lock_state().phase == GatePhase::Running
    }

    /// Counts one completed poll interval.
    #[cfg(test)]
    fn record_tick(&self) {
        self.ticks.fetch_add(1, Ordering::Relaxed);
    }

    /// Returns the completed poll intervals. Zero for a watchdog that has only
    /// ever been parked.
    #[cfg(test)]
    fn tick_count(&self) -> u64 {
        self.ticks.load(Ordering::Relaxed)
    }

    /// Returns how many times the watchdog has started the wait it parks in.
    ///
    /// Once a test has read a count, the watchdog is inside that wait or about
    /// to release `state` into it, so a change the test then makes to the
    /// gate cannot be missed.
    #[cfg(test)]
    fn parked_waits(&self) -> u64 {
        self.parked_waits.load(Ordering::SeqCst)
    }

    /// Returns how many poll-interval waits the watchdog has started. The
    /// same holds for a count read here as for [`Self::parked_waits`].
    #[cfg(test)]
    fn poll_waits(&self) -> u64 {
        self.poll_waits.load(Ordering::SeqCst)
    }
}

/// How many times the watchdog tries to announce one lock to the UI.
///
/// An announcement that panics is tried again on the next round, because
/// until it gets through the UI shows an open session over a locked vault.
/// The number is bounded because a round that follows a panic starts at
/// once: the vault is locked, so there is no poll interval to wait out, and
/// an emitter that panics every time would otherwise keep the thread
/// spinning for the rest of the process. Three is a first try and two more;
/// nothing was measured to choose it.
const MAX_LOCK_ANNOUNCE_ATTEMPTS: u8 = 3;

/// What the gate tells the watchdog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GateState {
    /// Whether the watchdog polls or waits.
    phase: GatePhase,
    /// A lock the watchdog still has to report to the UI: its own idle lock,
    /// or one forced by a recovery from a panic, which happens where no
    /// `AppHandle` is at hand.
    lock_to_announce: Option<LockAnnouncement>,
    /// How many locks have been queued for announcement, which numbers them.
    /// Wraps; only equality of two numbers close in time is ever tested.
    locks_queued: u64,
}

impl GateState {
    /// Takes one attempt at the queued announcement and returns the number
    /// of its lock, or `None` when there is nothing to announce now.
    ///
    /// Two queued announcements are dropped here instead of being returned:
    ///
    /// - one whose vault is unlocked again. The UI unlocked it, so it shows
    ///   an open session over an open vault, and the event would drop it to
    ///   the unlock screen. The phase stands for the vault's status: it is
    ///   written with the vault mutex held, when a vault operation ends
    ///   ([`VaultGuard`]) and when a lock is queued
    ///   ([`WatchdogGate::announce_lock`]).
    /// - one that has used up [`MAX_LOCK_ANNOUNCE_ATTEMPTS`].
    fn next_announcement(&mut self) -> Option<u64> {
        let queued = self.lock_to_announce.as_mut()?;

        if self.phase == GatePhase::Running || queued.attempts_left == 0 {
            self.lock_to_announce = None;
            return None;
        }
        queued.attempts_left -= 1;

        Some(queued.lock)
    }
}

/// A lock the UI has not been told of yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LockAnnouncement {
    /// The number of the lock ([`GateState::locks_queued`] when it was
    /// queued), which tells it from a lock queued later.
    lock: u64,
    /// How many more times the watchdog may try to announce it.
    attempts_left: u8,
}

/// What the watchdog loop does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GatePhase {
    /// The vault is not unlocked; nothing to poll for.
    Parked,
    /// The vault is unlocked; check idle time every poll interval.
    Running,
    /// The loop should end.
    Shutdown,
}

/// Why [`WatchdogGate::wait_for_work`] returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wake {
    /// The vault is unlocked: start polling.
    Poll,
    /// The vault was locked behind the UI's back: emit `vault-locked` for
    /// the lock of this number, and report it with
    /// [`WatchdogGate::lock_announced`] once the emitter has returned.
    AnnounceLock(u64),
    /// The loop should end.
    Shutdown,
}

/// Returns whether the watchdog should lock the vault now: it is unlocked
/// and has been idle for the timeout.
///
/// `idle_secs` is `None` when the idle time is unknown because the clock was
/// set back ([`idle_secs`]); an unlocked vault is then locked at once.
///
/// A pure predicate, so its cases are tested without a watchdog thread or an
/// `AppHandle`.
#[must_use]
pub(crate) const fn should_auto_lock(
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

/// Starts the thread that locks the vault when it has been idle, whatever
/// the webview is doing.
///
/// A timer in the webview cannot be what closes the vault: the webview may
/// throttle timers, stall, or be reloaded. The frontend timer is only a
/// fast-path duplicate; this thread guarantees the lock. It emits
/// `vault-locked` so the UI can drop to the unlock screen. An emission that
/// panics is tried again, [`MAX_LOCK_ANNOUNCE_ATTEMPTS`] times in all, and
/// one whose vault has been unlocked again by then is not made
/// ([`run_auto_lock_loop`]). An emission that returns an error is not
/// retried: the error is discarded.
///
/// While the vault is not unlocked the thread parks on [`WatchdogGate`]
/// instead of waking every poll interval, so a locked session can stay
/// open (or hidden to tray) without a periodic wakeup. Unlock notifies;
/// lock notifies so an in-flight poll wait parks immediately.
///
/// The thread is named `auto-lock` and runs until the process exits, so its
/// handle is not kept: there is no point at which it could be joined.
///
/// # Errors
///
/// Returns the operating system's error when the thread cannot be started.
/// The app must not run without the watchdog, so the caller fails startup.
pub(crate) fn spawn_auto_lock(
    app: tauri::AppHandle,
    handles: WatchdogHandles,
) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("auto-lock".into())
        .spawn(move || {
            run_auto_lock_loop(&handles, AUTO_LOCK_POLL_INTERVAL, || {
                use tauri::Emitter;
                let _ = app.emit("vault-locked", ());
            });
        })?;

    Ok(())
}

/// Runs the idle watchdog until the gate shuts it down.
///
/// `on_locked` tells the UI of a lock and runs with no mutex held. Every
/// lock the UI was not told of goes through the gate's queue
/// ([`WatchdogGate::announce_lock`]): the watchdog's own idle lock, and a
/// lock forced by a recovery from a panic. A round takes the queued lock and
/// calls `on_locked` for it, unless the vault has been unlocked again in the
/// meantime ([`GateState::next_announcement`]).
///
/// A round that panics is contained: the vault is locked
/// ([`lock_after_panic`]) and the loop goes on. When it was `on_locked` that
/// panicked, the lock stays queued and the next round calls `on_locked` for
/// it again, [`MAX_LOCK_ANNOUNCE_ATTEMPTS`] times in all.
///
/// The decision to announce and the call are two steps. An unlock that
/// completes between them is still followed by the event, which this loop
/// cannot prevent.
fn run_auto_lock_loop(
    handles: &WatchdogHandles,
    poll_interval: Duration,
    mut on_locked: impl FnMut(),
) {
    loop {
        // A panic that ended this thread would end auto-lock for the rest of
        // the process, with nothing to show for it. Each round is therefore
        // contained and the loop goes on.
        //
        // `AssertUnwindSafe`: the handles are mutexes that recover from
        // poisoning and atomics, and `on_locked` is called again only to
        // announce a lock: the one it panicked on, or a later one.
        let round = std::panic::catch_unwind(AssertUnwindSafe(|| {
            watch_one_round(handles, poll_interval, &mut on_locked)
        }));

        match round {
            Ok(ControlFlow::Break(())) => return,
            Ok(ControlFlow::Continue(())) => {}
            Err(_panic) => lock_after_panic(&handles.vault),
        }
    }
}

/// Runs one pass of the watchdog. `Break` ends the loop.
///
/// A pass either announces a queued lock, or waits until the vault is
/// unlocked, waits one poll interval, and locks if idle. An idle lock is
/// queued, not announced in the same pass, so that it is retried and
/// dropped by the same rules as every other lock.
fn watch_one_round(
    handles: &WatchdogHandles,
    poll_interval: Duration,
    on_locked: &mut impl FnMut(),
) -> ControlFlow<()> {
    let WatchdogHandles {
        vault,
        last_activity,
    } = handles;

    match vault.gate.wait_for_work() {
        Wake::Shutdown => return ControlFlow::Break(()),
        Wake::AnnounceLock(lock) => {
            on_locked();
            // Not reached when `on_locked` panics, which leaves the lock
            // queued for the next round.
            vault.gate.lock_announced(lock);
            return ControlFlow::Continue(());
        }
        Wake::Poll => {}
    }

    match vault.gate.wait_poll_interval(poll_interval) {
        GatePhase::Shutdown => return ControlFlow::Break(()),
        GatePhase::Parked => return ControlFlow::Continue(()),
        GatePhase::Running => {}
    }

    #[cfg(test)]
    vault.gate.record_tick();

    lock_if_idle(vault, last_activity);
    ControlFlow::Continue(())
}

/// Locks the vault after a watchdog round panicked.
///
/// How far the round got is unknown, including whether the idle check ran, so
/// the vault is locked rather than left open on a guess. That also parks the
/// gate, which keeps a round that panics every time from spinning: the next
/// one waits for an unlock, once any queued lock has used up its attempts.
///
/// A lock made here is queued and announced by the next round, inside the
/// containment. If the vault is already locked nothing new is queued. A lock
/// still queued then, which is the case when it was its announcement that
/// panicked, keeps the attempts it has left; it is not given new ones here,
/// or an emitter that panics every time would be called without end.
fn lock_after_panic(vault: &GatedVault) {
    // The panic message itself went to the panic hook.
    log::error!("auto-lock watchdog round panicked; locking the vault");

    let mut guard = vault.acquire();
    if guard.status() == VaultStatus::Unlocked {
        guard.lock();
        vault.gate.announce_lock();
    }
}

/// Locks the vault if it is unlocked and has been idle for the timeout, and
/// queues the lock for the next round to announce.
fn lock_if_idle(vault: &GatedVault, last_activity: &AtomicU64) {
    let mut guard = vault.acquire();

    // Sampled with the vault held, so the decision uses the activity and the
    // timeout as they are once every earlier vault operation has finished,
    // not as they were before this thread waited for the mutex.
    let idle = idle_secs(now_secs(), last_activity.load(Ordering::Relaxed));
    let timeout = vault.lock_timeout_secs.load(Ordering::Relaxed);

    if should_auto_lock(idle, timeout, guard.status()) {
        guard.lock();
        vault.gate.announce_lock();
    }
}

/// Returns the directory that holds the OCR models, for a packaged build and
/// for one run from the source tree.
///
/// When no candidate holds the models, returns the source-tree path, so the
/// caller always gets a path; the analyzer then reports the models missing.
#[must_use]
pub(crate) fn resolve_ocr_model_dir(resource_dir: Option<PathBuf>) -> PathBuf {
    // The source-tree layout: apps/desktop/src-tauri/resources/ocr.
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/ocr");
    let beside_executable = std::env::current_exe()
        .ok()
        .and_then(|executable| executable.parent().map(|dir| dir.join("resources/ocr")));

    let candidates = ocr_model_dir_candidates(resource_dir, &dev, beside_executable);
    first_dir_with_models(candidates).unwrap_or(dev)
}

/// Returns the places the OCR models may be, most specific first.
///
/// The Tauri resource directory of a packaged build comes first. The bundle
/// keeps the `resources/ocr/*` path from `tauri.conf.json`, so on macOS the
/// models sit in `Contents/Resources/resources/ocr`; that entry is what
/// finds them there, since the compile-time source-tree path exists only on
/// the machine that built the app. Some bundles put resources under `ocr` or
/// flat.
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

/// Returns the first directory that holds the text-detection model file.
fn first_dir_with_models(candidates: Vec<PathBuf>) -> Option<PathBuf> {
    candidates
        .into_iter()
        .find(|dir| dir.join("text-detection.rten").is_file())
}

#[cfg(test)]
mod tests {
    use super::{
        AUTO_LOCK_POLL_INTERVAL, AppState, GatePhase, GateState, GatedVault, GrantPurpose,
        LockAnnouncement, MAX_LOCK_ANNOUNCE_ATTEMPTS, WatchdogHandles, idle_secs, now_secs,
        resolve_ocr_model_dir, run_auto_lock_loop, should_auto_lock,
    };
    use oikonomia_core::ledger::set_lock_timeout_secs;
    use oikonomia_core::vault::VaultStatus;
    use std::fs;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;
    use std::sync::{Arc, TryLockError};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    /// A master password long enough for core to accept.
    const TEST_PASSWORD: &str = "correct horse battery staple";
    /// A poll interval short enough that a test can wait for an idle check.
    const SHORT_POLL: Duration = Duration::from_millis(20);
    /// A poll interval no test waits out, so a watchdog that leaves its poll
    /// wait was woken and did not time out.
    const NEVER_ELAPSES: Duration = Duration::from_hours(1);
    /// How long a test waits for the watchdog thread before it fails.
    ///
    /// It only turns an event that never comes into a failure instead of a
    /// hung test. No wait bounded by it covers a key derivation, which can
    /// take many seconds on a loaded runner: each unlock is finished, on the
    /// test's own thread or behind a signal of its own, before such a wait
    /// starts. What remains is a thread being scheduled and a few
    /// [`SHORT_POLL`] intervals.
    const GIVE_UP_AFTER: Duration = Duration::from_mins(1);
    /// How long a test waits for a step that derives a key on another thread.
    ///
    /// Long enough that only a deadlock reaches it, never a slow runner.
    const DEADLOCK_AFTER: Duration = Duration::from_mins(5);

    #[test]
    fn packaged_ocr_models_resolve_from_the_bundle_resources() {
        // tauri.conf.json bundles `resources/ocr/*`, which keeps that relative
        // path: on macOS the models land in Contents/Resources/resources/ocr.
        let resource_dir = std::env::temp_dir().join(format!(
            "oiko-ocr-bundle-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since_epoch| since_epoch.as_nanos())
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

        let grants = state.path_grants();
        let csv = GrantPurpose::Csv;
        assert_eq!(grants.resolve(csv, &picked), None, "nothing granted yet");

        state.grant_paths(csv, [picked.clone()]);

        let resolved = picked.canonicalize().expect("canonical");
        assert_eq!(grants.resolve(csv, &picked), Some(resolved.clone()));
        assert_eq!(
            grants.resolve(csv, &indirect),
            Some(resolved),
            "same file through .."
        );
        assert_eq!(grants.resolve(csv, &dir.join("other.csv")), None);
        let _ = fs::remove_dir_all(&dir);
    }

    /// Every purpose, so that a purpose added later is tested as well: the
    /// `match` stops compiling until it is listed.
    fn every_purpose() -> [GrantPurpose; 3] {
        let all = [
            GrantPurpose::Backup,
            GrantPurpose::Csv,
            GrantPurpose::Document,
        ];
        for purpose in all {
            match purpose {
                GrantPurpose::Backup | GrantPurpose::Csv | GrantPurpose::Document => {}
            }
        }
        all
    }

    #[test]
    fn a_path_granted_for_one_purpose_is_accepted_for_that_purpose_only() {
        for granted_for in every_purpose() {
            let (state, dir) = test_state("grant-purpose");
            let picked = dir.join("picked.bin");
            fs::write(&picked, b"bytes").expect("write");
            let resolved = picked.canonicalize().expect("canonical");

            state.grant_paths(granted_for, [picked.clone()]);

            for asked_for in every_purpose() {
                let expected = (asked_for == granted_for).then(|| resolved.clone());
                assert_eq!(
                    state.path_grants().resolve(asked_for, &picked),
                    expected,
                    "granted for {granted_for:?}, asked for {asked_for:?}"
                );
            }
            let _ = fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn a_path_may_be_granted_for_two_purposes_and_each_grant_stands_alone() {
        let (state, dir) = test_state("grant-twice");
        let picked = dir.join("picked.csv");
        fs::write(&picked, b"date,amount\n").expect("write");
        let resolved = picked.canonicalize().expect("canonical");
        let grants = state.path_grants();

        state.grant_paths(GrantPurpose::Csv, [picked.clone()]);
        state.grant_paths(GrantPurpose::Document, [picked.clone()]);

        assert_eq!(
            grants.resolve(GrantPurpose::Csv, &picked),
            Some(resolved.clone())
        );
        assert_eq!(
            grants.resolve(GrantPurpose::Document, &picked),
            Some(resolved)
        );
        assert_eq!(grants.resolve(GrantPurpose::Backup, &picked), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn auto_lock_when_unlocked_and_idle_meets_timeout() {
        assert!(should_auto_lock(Some(60), 60, VaultStatus::Unlocked));
        assert!(should_auto_lock(Some(61), 60, VaultStatus::Unlocked));
    }

    #[test]
    fn no_auto_lock_when_idle_below_timeout() {
        assert!(!should_auto_lock(Some(59), 60, VaultStatus::Unlocked));
    }

    #[test]
    fn no_auto_lock_after_activity_resets_idle() {
        assert!(!should_auto_lock(Some(0), 60, VaultStatus::Unlocked));
    }

    #[test]
    fn no_auto_lock_when_already_locked() {
        assert!(!should_auto_lock(Some(120), 60, VaultStatus::Locked));
        assert!(!should_auto_lock(Some(120), 60, VaultStatus::Uninitialized));
        assert!(!should_auto_lock(None, 60, VaultStatus::Locked));
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
        vault.acquire().unlock(TEST_PASSWORD).expect("unlock");

        // The bound only fails the test if no lock ever comes.
        let locked = emits.recv_timeout(GIVE_UP_AFTER);
        shutdown_watchdog(&vault, join);
        locked.expect("activity in the future must lock at once");
        assert_eq!(vault.acquire().status(), VaultStatus::Locked);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn watchdog_keeps_locking_after_a_round_panicked() {
        let (state, dir) = test_state("panic");
        init_locked_vault(&state);
        let handles = state.watchdog_handles();
        let vault = Arc::clone(&handles.vault);
        let last_activity = Arc::clone(&handles.last_activity);

        // The first announcement panics, as a failing emitter would.
        let (join, announcements) = spawn_counting_watchdog(handles, SHORT_POLL, |call| call == 1);

        // The first session's lock is announced twice: the call that panics
        // and the one that repeats it. The second session's lock is the
        // third call, which only a watchdog that is still running makes.
        for calls_expected in [vec![1, 2], vec![3]] {
            vault.acquire().unlock(TEST_PASSWORD).expect("unlock");
            last_activity.store(0, Ordering::Relaxed);

            for call in calls_expected {
                // The bound only fails the test if the watchdog is gone.
                let got = announcements.recv_timeout(GIVE_UP_AFTER);
                assert_eq!(got.ok(), Some(call), "announcement {call}");
            }
            assert_eq!(vault.acquire().status(), VaultStatus::Locked);
        }

        shutdown_watchdog(&vault, join);
        let _ = fs::remove_dir_all(&dir);
    }

    /// Checks that a lock whose announcement panicked is announced again.
    ///
    /// No unlock follows the idle lock, so the panic recovery finds the
    /// vault locked and queues nothing: only the retry can tell the UI, which
    /// otherwise shows an open session over a locked vault.
    #[test]
    fn a_lock_announcement_that_panics_is_made_again_until_it_gets_through() {
        let (state, dir) = test_state("retry");
        init_locked_vault(&state);
        let handles = state.watchdog_handles();
        let vault = Arc::clone(&handles.vault);
        handles.last_activity.store(0, Ordering::Relaxed);
        const {
            assert!(
                MAX_LOCK_ANNOUNCE_ATTEMPTS >= 3,
                "the emitter below panics twice, so the test needs two retries"
            );
        }

        let (join, announcements) = spawn_counting_watchdog(handles, SHORT_POLL, |call| call < 3);
        wait_until("the watchdog parks on the locked vault", || {
            vault.gate.parked_waits() >= 1
        });
        let parks_before_unlock = vault.gate.parked_waits();

        vault.acquire().unlock(TEST_PASSWORD).expect("unlock");

        for call in 1..=3_u32 {
            let got = announcements.recv_timeout(GIVE_UP_AFTER);
            assert_eq!(got.ok(), Some(call), "announcement {call}");
        }
        assert_eq!(vault.acquire().status(), VaultStatus::Locked);

        // The third call returned, so the lock is announced and the watchdog
        // parks. Parked is a wait only a notification ends, so no fourth
        // call can follow it.
        wait_until("the watchdog parks after the announcement", || {
            vault.gate.parked_waits() > parks_before_unlock
        });
        assert_eq!(announcements.try_recv().ok(), None, "announced once more");

        shutdown_watchdog(&vault, join);
        let _ = fs::remove_dir_all(&dir);
    }

    /// Checks the bound on the retries, and that giving up on one lock does
    /// not stop the watchdog from locking and announcing the next session.
    #[test]
    fn a_lock_announcement_that_always_panics_is_given_up_after_the_bound() {
        let (state, dir) = test_state("retry-bound");
        init_locked_vault(&state);
        let handles = state.watchdog_handles();
        let vault = Arc::clone(&handles.vault);
        let last_activity = Arc::clone(&handles.last_activity);
        last_activity.store(0, Ordering::Relaxed);
        let bound = u32::from(MAX_LOCK_ANNOUNCE_ATTEMPTS);

        let (join, announcements) =
            spawn_counting_watchdog(handles, SHORT_POLL, move |call| call <= bound);
        wait_until("the watchdog parks on the locked vault", || {
            vault.gate.parked_waits() >= 1
        });
        let parks_before_unlock = vault.gate.parked_waits();

        vault.acquire().unlock(TEST_PASSWORD).expect("unlock");

        for call in 1..=bound {
            let got = announcements.recv_timeout(GIVE_UP_AFTER);
            assert_eq!(got.ok(), Some(call), "announcement {call}");
        }
        wait_until("the watchdog gives up and parks", || {
            vault.gate.parked_waits() > parks_before_unlock
        });
        assert_eq!(announcements.try_recv().ok(), None, "tried past the bound");
        assert_eq!(vault.acquire().status(), VaultStatus::Locked);

        vault.acquire().unlock(TEST_PASSWORD).expect("unlock");
        last_activity.store(0, Ordering::Relaxed);

        let next = announcements.recv_timeout(GIVE_UP_AFTER);
        assert_eq!(next.ok(), Some(bound + 1), "the next session's lock");

        shutdown_watchdog(&vault, join);
        let _ = fs::remove_dir_all(&dir);
    }

    /// Checks that a queued lock is not announced once the vault is open
    /// again.
    ///
    /// A command panics with the vault open. The next command is an unlock:
    /// taking the vault it finds the mutex poisoned, so the recovery locks
    /// the session and queues that lock, and then the command unlocks the
    /// vault under the same guard. The UI asked for that unlock and shows an
    /// open session; `vault-locked` would drop it to the unlock screen over
    /// an open vault.
    ///
    /// The watchdog is started only once all of that has happened, so the
    /// order of the unlock and the watchdog's round is not left to the
    /// scheduler.
    #[test]
    fn a_queued_lock_is_not_announced_once_the_vault_is_unlocked_again() {
        let (state, dir) = test_state("requeue");
        init_locked_vault(&state);
        let vault = state.vault();
        vault.acquire().unlock(TEST_PASSWORD).expect("unlock");
        state.touch();
        vault.acquire().set_lock_timeout_cache(15 * 60);

        let poisoner = Arc::clone(&vault);
        let panicked = std::thread::spawn(move || {
            let _guard = poisoner.acquire();
            std::panic::resume_unwind(Box::new("a command panicked"));
        })
        .join();
        assert!(panicked.is_err());

        vault.acquire().unlock(TEST_PASSWORD).expect("unlock again");
        assert!(
            vault.gate.lock_state().lock_to_announce.is_some(),
            "the recovery did not queue the lock it made"
        );
        assert!(vault.gate.is_running());

        // The interval does not elapse: a watchdog inside its poll wait has
        // passed the point where it announces a queued lock.
        let (join, emits) = spawn_watchdog(&state, NEVER_ELAPSES);
        wait_until("the watchdog is inside its poll wait", || {
            vault.gate.poll_waits() >= 1
        });

        assert_eq!(emits.try_recv().ok(), None, "announced over an open vault");
        assert_eq!(vault.gate.lock_state().lock_to_announce, None);
        assert_eq!(vault.acquire().status(), VaultStatus::Unlocked);

        shutdown_watchdog(&vault, join);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_queued_lock_is_taken_while_parked_and_dropped_once_running_or_spent() {
        let parked = |lock_to_announce| GateState {
            phase: GatePhase::Parked,
            lock_to_announce,
            locks_queued: 7,
        };
        let queued = |attempts_left| {
            Some(LockAnnouncement {
                lock: 7,
                attempts_left,
            })
        };

        let mut nothing = parked(None);
        assert_eq!(nothing.next_announcement(), None);

        let mut waiting = parked(queued(2));
        assert_eq!(waiting.next_announcement(), Some(7));
        assert_eq!(waiting.lock_to_announce, queued(1));
        assert_eq!(waiting.next_announcement(), Some(7));
        assert_eq!(waiting.next_announcement(), None, "no attempt left");
        assert_eq!(waiting.lock_to_announce, None);

        let mut unlocked_again = GateState {
            phase: GatePhase::Running,
            ..parked(queued(MAX_LOCK_ANNOUNCE_ATTEMPTS))
        };
        assert_eq!(unlocked_again.next_announcement(), None);
        assert_eq!(unlocked_again.lock_to_announce, None);
    }

    /// The gate has to show the vault locked from the moment a lock is
    /// queued. The caller's guard parks it only when it drops, and a round
    /// that ran in between would take the queued lock for one made before an
    /// unlock and drop it.
    #[test]
    fn queueing_a_lock_parks_the_gate_and_only_its_own_announcement_clears_it() {
        let (state, dir) = test_state("queue");
        let vault = state.vault();
        vault.acquire().init(TEST_PASSWORD).expect("init");
        assert!(vault.gate.is_running());

        vault.gate.announce_lock();
        assert!(!vault.gate.is_running());
        let first = vault.gate.lock_state().next_announcement();

        // A second lock is queued while the first is being announced.
        vault.gate.announce_lock();
        vault
            .gate
            .lock_announced(first.expect("the first lock is queued"));

        let second = vault.gate.lock_state().next_announcement();
        assert!(
            second.is_some() && second != first,
            "the later lock was lost"
        );
        vault.gate.lock_announced(second.expect("checked above"));
        assert_eq!(vault.gate.lock_state().next_announcement(), None);
        let _ = fs::remove_dir_all(&dir);
    }

    /// Checks that a session the panic recovery locks is announced to the UI.
    ///
    /// The emitter panics on the idle lock, and before the watchdog's recovery
    /// runs the user has unlocked again. The recovery cannot tell how far the
    /// round got, so it locks that new session, and the UI, which shows it
    /// unlocked, has to be told.
    ///
    /// The new session is not idle, so the only thing that can lock it, and
    /// produce the second announcement, is the recovery.
    #[test]
    fn a_session_locked_by_the_panic_recovery_is_announced() {
        let (state, dir) = test_state("recovery");
        init_locked_vault(&state);
        let handles = state.watchdog_handles();
        let vault = Arc::clone(&handles.vault);
        let last_activity = Arc::clone(&handles.last_activity);

        let (announced, announcements) = mpsc::channel();
        let (unlocked_again, unlocks) = mpsc::channel();
        let unlocker = Arc::clone(&vault);
        let activity = Arc::clone(&last_activity);
        let mut calls = 0_u32;
        let join = std::thread::spawn(move || {
            run_auto_lock_loop(&handles, SHORT_POLL, move || {
                calls += 1;
                if calls == 1 {
                    unlocker.acquire().unlock(TEST_PASSWORD).expect("unlock");
                    activity.store(now_secs(), Ordering::Relaxed);
                    let _ = unlocked_again.send(());
                    std::panic::resume_unwind(Box::new("emit failed"));
                }
                let _ = announced.send(calls);
            });
        });

        last_activity.store(0, Ordering::Relaxed);
        vault.acquire().unlock(TEST_PASSWORD).expect("unlock");

        // The second unlock derives a key on the watchdog's thread, so this
        // wait is bounded only against a deadlock. What follows it, the
        // recovery's lock and its announcement, derives none.
        unlocks
            .recv_timeout(DEADLOCK_AFTER)
            .expect("the emitter is called for the idle lock and unlocks again");

        let got = announcements.recv_timeout(GIVE_UP_AFTER);
        assert_eq!(got.ok(), Some(2), "the recovery's lock was not announced");
        assert_eq!(vault.acquire().status(), VaultStatus::Locked);

        shutdown_watchdog(&vault, join);
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
        let (join, emits) = spawn_watchdog(&state, SHORT_POLL);

        // Parked is a wait that only a notification ends, so once the
        // watchdog is in it no amount of further waiting could show a poll.
        wait_until("the watchdog parks on the locked vault", || {
            vault.gate.parked_waits() >= 1
        });
        assert_eq!(
            vault.gate.tick_count(),
            0,
            "the watchdog polled a vault that was never unlocked"
        );
        assert_eq!(vault.acquire().status(), VaultStatus::Locked);
        let parks_before_unlock = vault.gate.parked_waits();

        last_activity.store(0, Ordering::Relaxed);
        vault.acquire().unlock(TEST_PASSWORD).expect("unlock");

        emits
            .recv_timeout(GIVE_UP_AFTER)
            .expect("an idle unlocked session is locked and vault-locked emitted");
        assert!(
            vault.gate.tick_count() >= 1,
            "unlock must unpark and run an idle check"
        );
        assert_eq!(vault.acquire().status(), VaultStatus::Locked);

        // A watchdog that kept polling the locked vault would never start
        // this wait again.
        wait_until("the watchdog parks again after locking", || {
            vault.gate.parked_waits() > parks_before_unlock
        });
        assert!(!vault.gate.is_running());

        shutdown_watchdog(&vault, join);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn watchdog_lock_parks_before_poll_wait_finishes() {
        let (state, dir) = test_state("inflight");
        init_locked_vault(&state);
        let vault = state.vault();
        vault.acquire().unlock(TEST_PASSWORD).expect("unlock");
        state.touch();
        vault.acquire().set_lock_timeout_cache(15 * 60);

        // The interval does not elapse, so only the lock's notification can
        // end the poll wait.
        let (join, _emits) = spawn_watchdog(&state, NEVER_ELAPSES);
        wait_until("the watchdog is inside its poll wait", || {
            vault.gate.poll_waits() >= 1
        });

        vault.acquire().lock();

        wait_until("the lock ends the poll wait and the watchdog parks", || {
            vault.gate.parked_waits() >= 1
        });
        assert_eq!(
            vault.gate.tick_count(),
            0,
            "a poll wait cut short by a lock must not run an idle check"
        );

        shutdown_watchdog(&vault, join);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_gate_runs_exactly_while_the_vault_is_unlocked() {
        let (state, dir) = test_state("follow");
        let vault = state.vault();
        assert!(!vault.gate.is_running(), "no vault yet");

        vault.acquire().init(TEST_PASSWORD).expect("init");
        assert!(vault.gate.is_running(), "init leaves the vault unlocked");

        vault.acquire().lock();
        assert!(!vault.gate.is_running());

        let wrong = vault.acquire().unlock("not the password at all");
        assert!(wrong.is_err());
        assert!(!vault.gate.is_running(), "a refused unlock changes nothing");

        vault.acquire().unlock(TEST_PASSWORD).expect("unlock");
        assert!(vault.gate.is_running());
        let _ = fs::remove_dir_all(&dir);
    }

    /// Checks that a guard updates the gate before it releases the vault.
    ///
    /// The race this guards against: a status change and its watchdog update
    /// are two writes, and another status change gets in between them, so the
    /// updates land in the wrong order and an unlocked vault is left with a
    /// parked watchdog.
    ///
    /// The test holds the gate state, which stops the unlock's guard inside
    /// its gate update, and checks that the vault is still held right then.
    #[test]
    fn a_guard_still_holds_the_vault_while_it_updates_the_gate() {
        let (state, dir) = test_state("order");
        init_locked_vault(&state);
        let vault = state.vault();
        let shared: &GatedVault = &vault;
        let entries_before = shared.gate.follow_entries.load(Ordering::SeqCst);

        std::thread::scope(|scope| {
            let gate_state = shared.gate.lock_state();
            let unlock = scope.spawn(move || shared.acquire().unlock(TEST_PASSWORD));

            // Every guard's drop calls `follow`, which then blocks on the
            // gate state this thread holds, so the count must go up before
            // the unlock thread can end. If it ends first, the unlock never
            // reached its gate update.
            wait_while_running(
                "the unlock's guard reaches its gate update",
                &unlock,
                || shared.gate.follow_entries.load(Ordering::SeqCst) > entries_before,
            );
            // The vault is unlocked and its gate update is pending. Anyone
            // who could take the vault now could get their update in first.
            let vault_is_held = matches!(shared.vault.try_lock(), Err(TryLockError::WouldBlock));

            drop(gate_state);
            unlock.join().expect("unlock thread").expect("unlock");
            assert!(
                vault_is_held,
                "the vault was released before the gate was updated"
            );
        });

        assert_eq!(vault.acquire().status(), VaultStatus::Unlocked);
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
        vault.acquire().unlock(TEST_PASSWORD).expect("unlock");
        state.touch();
        let (join, emits) = spawn_watchdog(&state, AUTO_LOCK_POLL_INTERVAL);

        let poisoner = Arc::clone(&vault);
        let panicked = std::thread::spawn(move || {
            let _guard = poisoner.acquire();
            std::panic::resume_unwind(Box::new("a command panicked"));
        })
        .join();
        assert!(panicked.is_err());

        assert_eq!(vault.acquire().status(), VaultStatus::Locked);
        assert!(
            !vault.gate.is_running(),
            "vault force-locked but watchdog running"
        );
        emits
            .recv_timeout(GIVE_UP_AFTER)
            .expect("the forced lock emits vault-locked");

        shutdown_watchdog(&vault, join);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_password_change_leaves_the_gate_and_timeout_matching_the_vault() {
        let (state, dir) = test_state("rekey");
        let vault = state.vault();
        {
            let mut guard = vault.acquire();
            guard.init(TEST_PASSWORD).expect("init");
            set_lock_timeout_secs(guard.connection().expect("connection"), 120).expect("store");
            guard.lock();
        }
        vault.acquire().set_lock_timeout_cache(7);

        vault
            .acquire()
            .change_password(TEST_PASSWORD, "another long password")
            .expect("change");

        // Whether a change on a locked vault unlocks it is core's decision,
        // so the status is not asserted here: the watchdog has to agree with
        // the outcome either way. The timeout read after an unlock has its
        // own test below.
        let status = vault.acquire().status();
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
            let mut guard = vault.acquire();
            guard.init(TEST_PASSWORD).expect("init");
            set_lock_timeout_secs(guard.connection().expect("connection"), 120).expect("store");
            guard.lock();
        }
        vault.acquire().set_lock_timeout_cache(7);

        vault.acquire().unlock(TEST_PASSWORD).expect("unlock");

        assert_eq!(vault.lock_timeout_secs.load(Ordering::Relaxed), 120);
        let _ = fs::remove_dir_all(&dir);
    }

    /// Waits until `condition`, which `worker` makes true, holds.
    ///
    /// The outcome does not depend on how fast `worker` runs. The unlock
    /// it waits on derives a key, which can take many seconds on a loaded
    /// CI runner, so a short deadline failed a correct run. Instead, `worker`
    /// ending first is the failure. The long deadline is only there so a
    /// deadlock fails the test instead of hanging it. Sleeping between
    /// checks leaves the CPU to `worker` rather than spinning against it.
    fn wait_while_running<T>(
        what: &str,
        worker: &std::thread::ScopedJoinHandle<'_, T>,
        condition: impl Fn() -> bool,
    ) {
        let hang = Instant::now() + Duration::from_secs(300);

        while !condition() {
            assert!(!worker.is_finished(), "the worker ended before {what}");
            assert!(Instant::now() < hang, "deadlocked waiting until {what}");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Waits until `condition`, which the watchdog thread makes true, holds.
    ///
    /// Only for an event the watchdog reaches without deriving a key: it
    /// parks, starts a poll wait or locks. [`GIVE_UP_AFTER`] then only turns
    /// an event that never comes into a failure instead of a hung test.
    /// Sleeping between checks leaves the CPU to the watchdog.
    fn wait_until(what: &str, condition: impl Fn() -> bool) {
        let deadline = Instant::now() + GIVE_UP_AFTER;

        while !condition() {
            assert!(Instant::now() < deadline, "timed out waiting until {what}");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Opens a state on a fresh directory and returns both.
    fn test_state(label: &str) -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "oiko-watchdog-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since_epoch| since_epoch.as_nanos())
        ));
        fs::create_dir_all(&dir).expect("tmpdir");
        let state = AppState::open_path(dir.clone(), dir.clone()).expect("state");
        (state, dir)
    }

    /// Creates a vault under [`TEST_PASSWORD`] and leaves it locked.
    fn init_locked_vault(state: &AppState) {
        let vault = state.vault();
        let mut guard = vault.acquire();
        guard.init(TEST_PASSWORD).expect("vault init");
        guard.lock();
    }

    /// Starts the watchdog loop on its own thread with `poll` as the poll
    /// interval. Each `vault-locked` it would emit arrives on the receiver.
    fn spawn_watchdog(
        state: &AppState,
        poll: Duration,
    ) -> (std::thread::JoinHandle<()>, mpsc::Receiver<()>) {
        let (sender, receiver) = mpsc::channel();
        let handles = state.watchdog_handles();
        let join = std::thread::spawn(move || {
            run_auto_lock_loop(&handles, poll, move || {
                let _ = sender.send(());
            });
        });
        (join, receiver)
    }

    /// Starts the watchdog loop with an emitter that numbers its calls from
    /// one, sends each number, and then panics, as a failing emitter would,
    /// when `panics_on` says so for that number.
    fn spawn_counting_watchdog(
        handles: WatchdogHandles,
        poll: Duration,
        panics_on: impl Fn(u32) -> bool + Send + 'static,
    ) -> (std::thread::JoinHandle<()>, mpsc::Receiver<u32>) {
        let (sender, receiver) = mpsc::channel();
        let mut calls = 0_u32;
        let join = std::thread::spawn(move || {
            run_auto_lock_loop(&handles, poll, move || {
                calls += 1;
                let _ = sender.send(calls);
                if panics_on(calls) {
                    std::panic::resume_unwind(Box::new("emit failed"));
                }
            });
        });
        (join, receiver)
    }

    /// Ends the watchdog loop and fails the test if its thread does not exit.
    fn shutdown_watchdog(vault: &GatedVault, join: std::thread::JoinHandle<()>) {
        vault.gate.shutdown();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = join.join();
            let _ = sender.send(());
        });
        receiver
            .recv_timeout(GIVE_UP_AFTER)
            .expect("watchdog thread should exit after shutdown");
    }
}
