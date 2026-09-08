// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Off-main-thread task helpers for GTK apps.
//!
//! Run blocking work (directory walks, subprocess spawns, network requests)
//! off the GTK main loop and deliver the result back on it, so the UI never
//! freezes. Shared so apps don't hand-roll `gio::spawn_blocking` +
//! `spawn_future_local` at every call site.
//!
//! # Lifetime binding
//!
//! [`spawn_blocking_result`] is fire-and-forget: its `on_result` runs on the
//! main loop even after the owning component has been torn down, which
//! stale-writes into freed model state. Two primitives close that gap:
//!
//! - [`BigTaskToken`] + [`spawn_result_bound`] — the model owns the token;
//!   dropping it (component teardown) makes the pending `on_result` a no-op.
//! - [`run_subprocess_bound`] / [`BigAsyncSubprocess`] + [`KillOnDrop`] —
//!   subprocess work bound to a component: teardown cancels delivery (via
//!   relm4's `drop_on_shutdown`) *and* kills the actual OS process.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use big_os_kit::subprocess::{
    BigSubprocessChild, BigSubprocessError, BigSubprocessOutput, BigSubprocessSpec,
};
use relm4::gtk;
use relm4::{Component, ComponentSender};

/// Run `task` off the GTK main thread, then deliver its result to `on_result`
/// back ON the main thread.
///
/// `task` must be `Send` (it crosses to a worker thread); `on_result` need not
/// be — it runs on the GLib main loop, so it can touch `!Send` GTK objects and
/// `Rc` UI state. Use a trailing-edge debouncer
/// ([`crate::input::debounce::BigDebouncedAction`]) before this for slider /
/// spin-button storms, and a generation/stale guard in `on_result` when rapid
/// input can supersede an in-flight task.
pub fn spawn_blocking_result<T, B, U>(task: B, on_result: U)
where
    T: Send + 'static,
    B: FnOnce() -> T + Send + 'static,
    U: FnOnce(T) + 'static,
{
    gtk::glib::spawn_future_local(async move {
        if let Ok(value) = gtk::gio::spawn_blocking(task).await {
            on_result(value);
        }
    });
}

/// Fire-and-forget variant of [`spawn_blocking_result`] — runs `task` off the
/// main thread and discards the result.
pub fn spawn_blocking_task<B>(task: B)
where
    B: FnOnce() + Send + 'static,
{
    spawn_blocking_result(task, |()| {});
}

/// Lifetime token that cancels a bound task when it is dropped.
///
/// The model that spawns a [`spawn_result_bound`] task owns the token. When
/// the component is torn down the model — and with it the token — is dropped,
/// which sets the shared cancelled flag. Any still-pending `on_result` then
/// becomes a no-op instead of stale-writing into freed model state. This is
/// the lifetime-bound counterpart to the unbound [`spawn_blocking_result`].
///
/// Single-threaded by design (`Rc<Cell<bool>>`): the token, the model, and
/// `on_result` all live on the GTK main loop. Only the blocking `task` crosses
/// to a worker thread, and it never touches the token.
///
/// # Examples
///
/// The flag is shared with in-flight tasks and flipped on drop:
///
/// ```
/// use big_relm4_components::task::BigTaskToken;
///
/// let token = BigTaskToken::new();
/// assert!(!token.is_cancelled());
/// drop(token); // component teardown cancels any bound task
/// ```
#[derive(Debug, Clone)]
pub struct BigTaskToken(Rc<Cell<bool>>);

impl BigTaskToken {
    /// Create a live (not-cancelled) token. Store it on the component model.
    #[must_use]
    pub fn new() -> Self {
        Self(Rc::new(Cell::new(false)))
    }

    /// `true` once the token has been cancelled (i.e. dropped, or an explicit
    /// [`Self::cancel`]). Bound tasks check this before delivering.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.get()
    }

    /// Cancel now, before teardown — e.g. when the model starts a fresh task
    /// that supersedes the in-flight one. Idempotent.
    pub fn cancel(&self) {
        self.0.set(true);
    }

    /// Clone of the shared cancelled flag, captured by a bound task so it can
    /// read the flag on the main loop after the owning token has dropped.
    fn shared_flag(&self) -> Rc<Cell<bool>> {
        Rc::clone(&self.0)
    }
}

impl Default for BigTaskToken {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for BigTaskToken {
    fn drop(&mut self) {
        // Teardown = cancellation. The clone held by any pending task keeps the
        // cell alive, so the delivery closure still reads `true` and skips.
        self.0.set(true);
    }
}

/// Lifetime-bound replacement for [`spawn_blocking_result`]: runs `task` off
/// the main thread, then delivers to `on_result` on the main loop **only if
/// `token` has not been cancelled** by then.
///
/// Prefer this whenever `on_result` writes into component/model state: binding
/// the delivery to a model-owned [`BigTaskToken`] guarantees a task that
/// finishes after the component is torn down cannot stale-write. Keep
/// [`spawn_blocking_result`] for genuinely unbound, fire-and-forget work whose
/// result touches no component state.
///
/// As with [`spawn_blocking_result`], `task` must be `Send`; `on_result` and
/// the token stay on the main loop and may touch `!Send` GTK / `Rc` state.
///
/// # Examples
///
/// ```ignore
/// // In a component's update():
/// use big_relm4_components::task::{BigTaskToken, spawn_result_bound};
///
/// // `self.load_token: BigTaskToken` lives on the model.
/// let sender = sender.clone();
/// spawn_result_bound(
///     &self.load_token,
///     || std::fs::read_to_string("/etc/os-release").unwrap_or_default(),
///     move |contents| sender.input(Msg::Loaded(contents)),
/// );
/// ```
pub fn spawn_result_bound<T, B, U>(token: &BigTaskToken, task: B, on_result: U)
where
    T: Send + 'static,
    B: FnOnce() -> T + Send + 'static,
    U: FnOnce(T) + 'static,
{
    let cancelled = token.shared_flag();
    gtk::glib::spawn_future_local(async move {
        if let Ok(value) = gtk::gio::spawn_blocking(task).await
            && !cancelled.get()
        {
            on_result(value);
        }
    });
}

/// RAII guard that kills (and reaps) a streaming child when dropped.
///
/// Hold this on a component model when it drives a long-lived child spawned
/// via [`BigSubprocessSpec::spawn`] (a tail, a player, a streaming decoder).
/// On teardown the guard drops and the child is `SIGKILL`ed immediately — no
/// poll latency, no reliance on a worker loop still running — then reaped so
/// no zombie leaks. This is the streaming-lane counterpart to
/// [`run_subprocess_bound`], which handles the run-to-completion lane.
///
/// For the run-to-completion lane use [`run_subprocess_bound`] instead: it
/// wires cancellation through [`BigSubprocessSpec::run_cancellable`].
pub struct KillOnDrop(
    /// The guarded child. Killed and reaped on drop.
    pub BigSubprocessChild,
);

impl KillOnDrop {
    /// Wrap a spawned child so teardown terminates it.
    #[must_use]
    pub fn new(child: BigSubprocessChild) -> Self {
        Self(child)
    }
}

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        // Best-effort on both calls: the child may have already exited.
        let _ = self.0.kill();
        // Reap the (now-terminated) child so it does not linger as a zombie.
        let _ = self.0.wait();
    }
}

/// Cancellation handle for a subprocess bound to a component lifetime by
/// [`run_subprocess_bound`].
///
/// The model owns this handle. Dropping it on teardown — or calling
/// [`Self::cancel`] — flips the shared cancel flag that the worker's
/// [`BigSubprocessSpec::run_cancellable`] loop polls, so the actual OS process
/// is killed within one poll tick. relm4's `drop_on_shutdown` independently
/// drops the result delivery, so a teardown-time result never lands in freed
/// state.
pub struct BigAsyncSubprocess {
    cancel: Arc<AtomicBool>,
}

impl BigAsyncSubprocess {
    /// Request cancellation now (e.g. a Cancel button), before teardown.
    /// Idempotent; the worker terminates the child within one poll tick.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// `true` once cancellation has been requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

impl Drop for BigAsyncSubprocess {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Run `spec` on a relm4 worker, bound to the calling component's lifetime,
/// and deliver its typed result as the component's `CommandOutput`.
///
/// The subprocess runs via [`BigSubprocessSpec::run_cancellable`] inside
/// `sender.spawn_oneshot_command`, so **two** teardown guarantees hold:
///
/// - relm4's `drop_on_shutdown` drops the *delivery* when the model drops, so
///   `to_output`'s message never reaches a torn-down component;
/// - dropping the returned [`BigAsyncSubprocess`] flips the cancel flag the
///   worker polls, so the actual *OS process* is killed (not merely detached).
///
/// Store the returned handle on the model. `to_output` maps the typed
/// `Result<BigSubprocessOutput, BigSubprocessError>` to the component's
/// `CommandOutput`, delivered by relm4's `update_cmd`.
///
/// # Examples
///
/// ```ignore
/// // `type CommandOutput = Result<BigSubprocessOutput, BigSubprocessError>;`
/// // and `self.probe: Option<BigAsyncSubprocess>` on the model.
/// use big_os_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};
/// use big_relm4_components::task::run_subprocess_bound;
/// use std::time::Duration;
///
/// let spec = BigSubprocessSpec::builder()
///     .program("ffprobe")
///     .arg("-of").arg("json").arg(&path)
///     .timeout(Duration::from_secs(10))
///     .stdout(BigSubprocessOutputMode::Capture)
///     .build();
/// self.probe = Some(run_subprocess_bound(&sender, spec, |result| result));
/// ```
#[must_use]
pub fn run_subprocess_bound<C, F>(
    sender: &ComponentSender<C>,
    spec: BigSubprocessSpec,
    to_output: F,
) -> BigAsyncSubprocess
where
    C: Component,
    F: FnOnce(Result<BigSubprocessOutput, BigSubprocessError>) -> C::CommandOutput + Send + 'static,
{
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel);
    sender.spawn_oneshot_command(move || {
        let result = spec.run_cancellable(&worker_cancel);
        to_output(result)
    });
    BigAsyncSubprocess { cancel }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn token_starts_uncancelled() {
        let token = BigTaskToken::new();
        assert!(!token.is_cancelled());
    }

    #[test]
    fn token_drop_sets_shared_cancelled_flag() {
        let token = BigTaskToken::new();
        let observer = token.shared_flag();
        assert!(!observer.get());
        drop(token);
        assert!(
            observer.get(),
            "dropping the token must cancel any bound task"
        );
    }

    #[test]
    fn token_explicit_cancel_flips_flag() {
        let token = BigTaskToken::new();
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn spawn_result_bound_skips_delivery_after_token_drop() {
        if gtk::init().is_err() {
            return;
        }
        let ran = Rc::new(Cell::new(false));
        let ran_in_cb = Rc::clone(&ran);

        let token = BigTaskToken::new();
        // Task sleeps so the token can be dropped before it completes.
        spawn_result_bound(
            &token,
            || {
                std::thread::sleep(Duration::from_millis(120));
                42u32
            },
            move |_value| ran_in_cb.set(true),
        );
        // Simulate component teardown before the worker finishes.
        drop(token);

        // Pump the main loop past the task duration; delivery must be skipped.
        let ctx = gtk::glib::MainContext::default();
        let deadline = Instant::now() + Duration::from_millis(700);
        while Instant::now() < deadline {
            while ctx.iteration(false) {}
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !ran.get(),
            "on_result must be skipped once the token is dropped"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn spawn_result_bound_delivers_while_token_alive() {
        if gtk::init().is_err() {
            return;
        }
        let got = Rc::new(Cell::new(0u32));
        let got_in_cb = Rc::clone(&got);

        let token = BigTaskToken::new();
        spawn_result_bound(&token, || 7u32, move |value| got_in_cb.set(value));

        let ctx = gtk::glib::MainContext::default();
        let deadline = Instant::now() + Duration::from_millis(700);
        while got.get() == 0 && Instant::now() < deadline {
            while ctx.iteration(false) {}
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(got.get(), 7, "on_result must run while the token is alive");
        drop(token);
    }

    #[test]
    // Miri does not implement `posix_spawn`; normal `cargo test` still checks
    // that dropping the guard kills a real long-lived child.
    #[cfg_attr(miri, ignore)]
    fn kill_on_drop_terminates_child_process() {
        let child = BigSubprocessSpec::builder()
            .program("sleep")
            .arg("30")
            .build()
            .spawn()
            .expect("spawn sleep");
        let pid = child.id();
        let proc_path = format!("/proc/{pid}");
        assert!(
            std::path::Path::new(&proc_path).exists(),
            "child should be alive before the guard drops"
        );

        {
            let _guard = KillOnDrop::new(child); // dropped here → kill + reap
        }

        // Kill is asynchronous at the OS level; poll for the pid to vanish.
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut gone = false;
        while Instant::now() < deadline {
            if !std::path::Path::new(&proc_path).exists() {
                gone = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            gone,
            "KillOnDrop must terminate the child (pid {pid} still present)"
        );
    }
}
