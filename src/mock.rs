/*!
Mock module to ease testing
    */
use crossbeam_channel::{bounded, Receiver};

use crate::response::Response;
use crate::sender::Sender;
use crate::transmission::Options;
use crate::Event;
use crate::Result;

/// Transmission mocker for use in tests (mostly in beeline-rust)
#[derive(Debug, Clone)]
pub struct TransmissionMock {
    started: usize,
    stopped: usize,
    events_called: usize,
    events: Vec<Event>,
    responses: Receiver<Response>,
    block_on_responses: bool,
}

impl Sender for TransmissionMock {
    // `send` queues up an event to be sent
    fn send(&mut self, ev: Event) {
        self.events.push(ev);
    }

    // `start` initializes any background processes necessary to send events
    fn start(&mut self) {
        self.started += 1;
    }

    // `stop` flushes any pending queues and blocks until everything in flight has
    // been sent
    fn stop(&mut self) -> Result<()> {
        self.stopped += 1;
        Ok(())
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
            events: Vec::new(),
            block_on_responses: false,
            responses,
        })
    }

    /// events
    pub fn events(&mut self) -> Vec<Event> {
        self.events_called += 1;
        self.events.clone()
    }
}
