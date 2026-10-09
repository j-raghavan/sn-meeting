//! ADR D7: one multi-thread tokio runtime per process, created by `nativeInit`.
use std::future::Future;
use std::sync::OnceLock;
use tokio::runtime::{Builder, Runtime};

static RUNTIME: OnceLock<Result<Runtime, String>> = OnceLock::new();

/// The process runtime, built on first use (idempotent; `nativeInit` calls it first).
pub fn get() -> Result<&'static Runtime, String> {
    RUNTIME
        .get_or_init(|| {
            Builder::new_multi_thread()
                .worker_threads(2)
                .thread_name("meeting-rt")
                .enable_all()
                .build()
                .map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Block the calling (non-runtime) thread on `fut`.
pub fn block_on<F: Future>(fut: F) -> Result<F::Output, String> {
    Ok(get()?.block_on(fut))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_runtime_per_process() {
        let a = get().unwrap() as *const Runtime;
        let b = get().unwrap() as *const Runtime;
        assert_eq!(a, b);
        assert_eq!(block_on(async { 7 }), Ok(7));
    }

    #[test]
    fn runtime_has_timers() {
        let slept = block_on(async {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            true
        });
        assert_eq!(slept, Ok(true));
    }
}
