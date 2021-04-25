use crate::errors::Result;
use crate::Event;

/// `Sender` is responsible for handling events after Send() is called.  Implementations
/// of `send()` must be safe for concurrent calls.
pub trait Sender {
    /// `send` queues up an event to be sent
    fn send(&mut self, ev: Event);

    /// `start` initializes any background processes necessary to send events
    fn start(&mut self);

    /// `stop` flushes any pending queues and blocks until everything in flight has been
    /// sent
    fn stop(&mut self) -> Result<()>;
}
