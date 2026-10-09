//! F5-FR7: the attempt policy for provider calls (whole-call budgets, one retry, cancel) and the
//! registry of jobs in flight (ADR D7). No rig or reqwest here: calls arrive as futures that
//! already map their failures to `CallError` (R8).
use crate::providers::{CallError, CallFail};
use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use tokio::sync::Notify;

/// reqwest's connect phase; the whole call is bounded by `race`'s budget.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(60);
pub const SUMMARISE_TIMEOUT: Duration = Duration::from_secs(180);
/// The wait before the one retry when the reply has no `Retry-After`.
pub const RETRY_DEFAULT: Duration = Duration::from_secs(5);
/// The longest `Retry-After` honoured.
pub const RETRY_CAP: Duration = Duration::from_secs(30);
/// A segment of unknown length (legacy, unmeasured) is budgeted as a full 15-minute one.
const UNKNOWN_SEGMENT_MS: u64 = 15 * 60 * 1000;

/// `60 s + duration / 4`: a 15-minute segment allows 4 min 45 s.
pub fn transcribe_timeout(segment_ms: Option<u64>) -> Duration {
    Budgets::default().transcribe(segment_ms)
}

/// A job's whole-call budgets. Fixed on the device; tests shorten them to reach a timeout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budgets {
    /// Added to a quarter of the segment's duration.
    pub transcribe_base: Duration,
    pub summarise: Duration,
}

impl Default for Budgets {
    fn default() -> Self {
        Budgets {
            transcribe_base: Duration::from_secs(60),
            summarise: SUMMARISE_TIMEOUT,
        }
    }
}

impl Budgets {
    pub fn transcribe(&self, segment_ms: Option<u64>) -> Duration {
        let ms = segment_ms.unwrap_or(UNKNOWN_SEGMENT_MS);
        self.transcribe_base + Duration::from_millis(ms / 4)
    }
}

/// The wait before the retry: `Retry-After` (5 s when absent), at most 30 s.
pub fn backoff(retry_after: Option<Duration>) -> Duration {
    retry_after.unwrap_or(RETRY_DEFAULT).min(RETRY_CAP)
}

/// One meeting's cancel flag. Sticky: once cancelled it stays cancelled, and every waiter
/// (the in-flight request, a backoff sleep) wakes at once.
#[derive(Clone, Debug, Default)]
pub struct Cancel(Arc<(AtomicBool, Notify)>);

impl Cancel {
    pub fn cancel(&self) {
        self.0.0.store(true, Ordering::SeqCst);
        self.0.1.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.0.load(Ordering::SeqCst)
    }

    /// Resolves once `cancel` has been called (immediately if it already was).
    pub async fn cancelled(&self) {
        loop {
            // Register before checking, so a cancel between the two is not missed.
            let notified = self.0.1.notified();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}

fn cancelled() -> CallError {
    CallError::new(CallFail::Cancelled, "Cancelled")
}

fn timed_out() -> CallError {
    CallError::new(CallFail::TimedOut, "Timed out")
}

/// One attempt within `budget`, raced against `cancel`. The losing future is dropped, which
/// aborts its HTTP request (reqwest cancels on drop).
pub async fn race<T>(
    cancel: &Cancel,
    budget: Duration,
    fut: impl Future<Output = Result<T, CallError>>,
) -> Result<T, CallError> {
    if cancel.is_cancelled() {
        return Err(cancelled());
    }
    tokio::select! {
        biased;
        () = cancel.cancelled() => Err(cancelled()),
        r = tokio::time::timeout(budget, fut) => r.unwrap_or_else(|_| Err(timed_out())),
    }
}

/// At most one automatic retry, only for `Retryable` (408/425/429/5xx), after `backoff`. The
/// wait races `cancel` too. Every other failure, and a second failure, is returned as is.
pub async fn with_retry<T, F, Fut>(
    cancel: &Cancel,
    budget: Duration,
    mut attempt: F,
) -> Result<T, CallError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, CallError>>,
{
    match race(cancel, budget, attempt()).await {
        Err(e) if e.kind == CallFail::Retryable => {
            tokio::select! {
                biased;
                () = cancel.cancelled() => return Err(cancelled()),
                () = tokio::time::sleep(backoff(e.retry_after)) => {}
            }
            race(cancel, budget, attempt()).await
        }
        r => r,
    }
}

/// Jobs in flight in this process, id → cancel flag. `App` reads `running()` into
/// `Liveness.jobs`, so recovery never fails a running job and delete refuses it (F3-FR5).
#[derive(Debug, Default)]
pub struct Registry(Mutex<HashMap<String, Cancel>>);

/// A running job's slot. Dropping it (on every path, a panic included) frees the meeting.
#[derive(Debug)]
pub struct JobGuard<'a> {
    reg: &'a Registry,
    id: String,
    pub cancel: Cancel,
}

impl Drop for JobGuard<'_> {
    fn drop(&mut self) {
        self.reg.map().remove(&self.id);
    }
}

impl Registry {
    fn map(&self) -> MutexGuard<'_, HashMap<String, Cancel>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Claim `id` for a job; `None` when one is already running on it ("meeting is busy").
    pub fn start(&self, id: &str) -> Option<JobGuard<'_>> {
        let mut m = self.map();
        if m.contains_key(id) {
            return None;
        }
        let cancel = Cancel::default();
        m.insert(id.to_owned(), cancel.clone());
        Some(JobGuard {
            reg: self,
            id: id.to_owned(),
            cancel,
        })
    }

    /// `nativeCancel`: `true` when a job was running on `id`. Never blocks on the job.
    pub fn cancel(&self, id: &str) -> bool {
        self.map().get(id).map(Cancel::cancel).is_some()
    }

    /// The ids with a job in flight.
    pub fn running(&self) -> Vec<String> {
        self.map().keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Instant;

    #[test]
    fn budgets_and_backoff() {
        assert_eq!(transcribe_timeout(Some(900_000)), Duration::from_secs(285));
        assert_eq!(transcribe_timeout(None), Duration::from_secs(285));
        assert_eq!(transcribe_timeout(Some(0)), Duration::from_secs(60));
        assert_eq!(backoff(None), Duration::from_secs(5));
        assert_eq!(
            backoff(Some(Duration::from_secs(2))),
            Duration::from_secs(2)
        );
        assert_eq!(backoff(Some(Duration::from_secs(31))), RETRY_CAP);
    }

    #[test]
    fn the_registry_is_exclusive_and_the_guard_frees_the_meeting() {
        let r = Registry::default();
        let g = r.start("m").unwrap();
        assert!(r.start("m").is_none(), "busy");
        assert_eq!(r.running(), ["m"]);
        assert!(r.cancel("m") && g.cancel.is_cancelled());
        drop(g);
        assert!(r.running().is_empty());
        assert!(!r.cancel("m"), "nothing to cancel");
        assert!(r.start("m").is_some());
    }

    fn err(kind: CallFail, retry_after: Option<u64>) -> CallError {
        CallError {
            retry_after: retry_after.map(Duration::from_secs),
            ..CallError::new(kind, "x")
        }
    }

    /// `with_retry` over scripted outcomes; returns the result and the attempts made.
    async fn scripted(outcomes: Vec<Result<u8, CallError>>) -> (Result<u8, CallError>, usize) {
        let n = AtomicUsize::new(0);
        let r = with_retry(&Cancel::default(), Duration::from_secs(5), || {
            let i = n.fetch_add(1, Ordering::SeqCst);
            let o = outcomes[i].clone();
            async move { o }
        })
        .await;
        (r, n.load(Ordering::SeqCst))
    }

    #[tokio::test(start_paused = true)]
    async fn one_retry_only_for_retryable_failures() {
        let (r, n) = scripted(vec![Err(err(CallFail::Retryable, Some(1))), Ok(7)]).await;
        assert_eq!((r.unwrap(), n), (7, 2));
        let (r, n) = scripted(vec![
            Err(err(CallFail::Retryable, None)),
            Err(err(CallFail::Retryable, None)),
        ])
        .await;
        assert_eq!((r.unwrap_err().kind, n), (CallFail::Retryable, 2));
        for kind in [CallFail::TooLarge, CallFail::KeyRejected, CallFail::Http] {
            let (r, n) = scripted(vec![Err(err(kind, Some(0)))]).await;
            assert_eq!((r.unwrap_err().kind, n), (kind, 1));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn the_retry_waits_for_retry_after() {
        let t0 = tokio::time::Instant::now();
        let (r, _) = scripted(vec![Err(err(CallFail::Retryable, Some(12))), Ok(1)]).await;
        assert!(r.is_ok());
        assert_eq!(t0.elapsed().as_secs(), 12);
        let t0 = tokio::time::Instant::now();
        let (r, _) = scripted(vec![Err(err(CallFail::Retryable, Some(600))), Ok(1)]).await;
        assert!(r.is_ok());
        assert_eq!(t0.elapsed(), RETRY_CAP, "capped");
    }

    #[tokio::test]
    async fn cancel_wins_over_a_slow_attempt_and_a_backoff() {
        let c = Cancel::default();
        let c2 = c.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            c2.cancel();
        });
        let t0 = Instant::now();
        let slow = || async {
            tokio::time::sleep(Duration::from_secs(10)).await;
            Ok::<u8, CallError>(1)
        };
        let e = with_retry(&c, Duration::from_secs(30), slow)
            .await
            .unwrap_err();
        assert_eq!(
            (e.kind, e.detail.as_str()),
            (CallFail::Cancelled, "Cancelled")
        );
        assert!(t0.elapsed() < Duration::from_secs(2));
        // Already cancelled: nothing is attempted.
        let e = race(&c, Duration::from_secs(1), async { Ok::<u8, CallError>(1) }).await;
        assert_eq!(e.unwrap_err().kind, CallFail::Cancelled);
        c.cancelled().await; // sticky: resolves at once

        // Cancelled during the backoff wait.
        let c = Cancel::default();
        let c2 = c.clone();
        let first = || async { Err::<u8, _>(err(CallFail::Retryable, Some(20))) };
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            c2.cancel();
        });
        let t0 = Instant::now();
        let e = with_retry(&c, Duration::from_secs(30), first)
            .await
            .unwrap_err();
        assert_eq!(e.kind, CallFail::Cancelled);
        assert!(t0.elapsed() < Duration::from_secs(2));
    }

    #[tokio::test(start_paused = true)]
    async fn the_budget_bounds_the_whole_attempt() {
        let slow = async {
            tokio::time::sleep(Duration::from_secs(10)).await;
            Ok::<u8, CallError>(1)
        };
        let e = race(&Cancel::default(), Duration::from_millis(200), slow).await;
        let e = e.unwrap_err();
        assert_eq!(
            (e.kind, e.detail.as_str()),
            (CallFail::TimedOut, "Timed out")
        );
    }
}
