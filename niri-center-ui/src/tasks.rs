//! Work that runs beside the UI: what blocks (files, niri's socket, `niri
//! validate`, a pick that waits for a click) on a thread of its own, and
//! messages that come after a wait.

use std::time::Duration;

use iced::Task;

/// Run `work` on tokio's blocking pool and turn its result into a message,
/// so the executor's threads stay free meanwhile. The browser has no
/// threads and nothing there blocks, so there it runs at once.
pub fn blocking<T, M>(
    work: impl FnOnce() -> T + Send + 'static,
    message: impl Fn(T) -> M + Send + Sync + 'static,
) -> Task<M>
where
    T: Send + 'static,
    M: Send + 'static,
{
    // spawned once the task runs, inside iced's runtime: tests make tasks
    // they never run, with no runtime about
    #[cfg(not(target_arch = "wasm32"))]
    return Task::future(async move { tokio::task::spawn_blocking(work).await }).then(
        move |done| match done {
            Ok(value) => Task::done(message(value)),
            // the work panicked: nothing to report it with
            Err(_) => Task::none(),
        },
    );
    #[cfg(target_arch = "wasm32")]
    Task::perform(async move { work() }, message)
}

/// `message`, once `delay` is over.
pub fn after<M: Clone + Send + 'static>(delay: Duration, message: M) -> Task<M> {
    // the timer starts once the task runs, as with [`blocking`]
    Task::future(async move {
        #[cfg(not(target_arch = "wasm32"))]
        tokio::time::sleep(delay).await;
        #[cfg(target_arch = "wasm32")]
        wasmtimer::tokio::sleep(delay).await;
    })
    .map(move |()| message.clone())
}
