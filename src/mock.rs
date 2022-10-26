/*!
Mock module to ease testing
    */

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
}

impl TransmissionMock {
    pub(crate) fn new(_options: Options) -> Result<Self> {
        Ok(Self {
            started: 0,
            stopped: 0,
            events_called: 0,
            events: Vec::new(),
            block_on_responses: false,
        })
    }

    /// events
    pub fn events(&mut self) -> Vec<Event> {
        self.events_called += 1;
        self.events.clone()
    }
}
