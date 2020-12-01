use std::time::{Duration, Instant};

use surf::StatusCode;

use crate::event::Event;
use crate::response::Response;

pub(crate) type Events = Vec<Event>;

pub(crate) trait EventsResponse {
    fn to_response(
        &self,
        status: Option<StatusCode>,
        body: Option<String>,
        clock: Instant,
        error: Option<String>,
    ) -> Vec<Response>;
}

impl EventsResponse for Events {
    fn to_response(
        &self,
        status: Option<StatusCode>,
        body: Option<String>,
        clock: Instant,
        error: Option<String>,
    ) -> Vec<Response> {
        let total_events = if self.is_empty() {
            1
        } else {
            self.len() as u64
        };

        let spent = Duration::from_secs(clock.elapsed().as_secs() / total_events);
        self.iter()
            .map(|ev| Response {
                status_code: status,
                body: body.clone(),
                duration: spent,
                metadata: ev.metadata.clone(),
                error: error.clone(),
            })
            .collect()
    }
}
