use async_channel::Receiver;
use async_trait::async_trait;
use futures::channel::oneshot;
use futures::Future;

use crate::errors::Result;
use crate::response::Response;
use crate::Event;

/// A Future that callers can await to determine when stop/flush has completed.
pub type StopFuture =
    Box<dyn Future<Output = std::result::Result<(), oneshot::Canceled>> + Send + Sync + Unpin>;

/// `Sender` is responsible for handling events after Send() is called.  Implementations
/// of `send()` must be safe for concurrent calls.
#[async_trait]
pub trait Sender {
    /// `send` queues up an event to be sent.
    ///
    /// If the work queue is full, this function blocks the current task until the
    /// event can be enqueued.
    async fn send(&self, ev: Event);

    /// `start` initializes any background processes necessary to send events
    fn start(&mut self) -> Result<()>;

    /// `stop` flushes any pending queues and blocks until everything in flight has been
    /// sent. The returned oneshot receiver is notified when all events have been flushed.
    ///
    /// If the work queue is full, this function blocks the current task until the stop
    /// event can be enqueued.
    async fn stop(&mut self) -> Result<StopFuture>;

    /// `responses` returns a channel that will contain a single Response for each Event
    /// added. Note that they may not be in the same order as they came in
    fn responses(&self) -> Receiver<Response>;
}
