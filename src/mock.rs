/*!
Mock module to ease testing
    */
use async_channel::{bounded, Receiver};
use async_std::sync::Mutex;
use async_trait::async_trait;
use futures::future;

use crate::response::Response;
use crate::sender::{Sender, StopFuture};
use crate::transmission::Options;
use crate::Event;
use crate::Result;

/// Transmission mocker for use in tests (mostly in beeline-rust)
#[derive(Debug)]
pub struct TransmissionMock {
    started: usize,
    stopped: usize,
    events_called: usize,
    events: Mutex<Vec<Event>>,
    responses: Receiver<Response>,
    block_on_responses: bool,
}

#[async_trait]
impl Sender for TransmissionMock {
    // `send` queues up an event to be sent
    async fn send(&self, ev: Event) {
        self.events.lock().await.push(ev);
    }

    // `start` initializes any background processes necessary to send events
    fn start(&mut self) {
        self.started += 1;
    }

    // `stop` flushes any pending queues and blocks until everything in flight has
    // been sent
    async fn stop(&mut self) -> Result<StopFuture> {
        self.stopped += 1;
        Ok(Box::new(future::ready(Ok(()))))
    }

    // `responses` returns a channel that will contain a single Response for each
    // Event added. Note that they may not be in the same order as they came in
    fn responses(&self) -> Receiver<Response> {
        self.responses.clone()
    }
}

impl TransmissionMock {
    pub(crate) fn new(options: Options) -> Result<Self> {
        let (_, responses) = bounded(options.pending_work_capacity * 4);

        Ok(Self {
            started: 0,
            stopped: 0,
            events_called: 0,
            events: Mutex::new(Vec::new()),
            block_on_responses: false,
            responses,
        })
    }

    /// events
    pub async fn events(&mut self) -> Vec<Event> {
        self.events_called += 1;
        self.events.lock().await.clone()
    }
}
