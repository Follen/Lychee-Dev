//! Lexical ownership of the synchronous source pipeline used by the Engine.
use lycheedev_core::Result;

pub(crate) fn run<T: Send>(work: impl FnOnce() -> Result<T> + Send) -> Result<T> {
    // Source preparation is a synchronous, checked ownership scope. Its HTTP
    // operations each own a runtime, so it must execute outside the Engine's
    // Tokio context (including Tokio's blocking pool). Keep admission, reads,
    // verification and close together, and join before the request can return.
    // Agent ingress has its own thread and can cancel the original control
    // while this serial request waits; no work survives a dropped future.
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("data-source-io".into())
            .spawn_scoped(scope, work)?
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}
