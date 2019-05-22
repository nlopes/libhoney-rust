use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use crossbeam_channel::{bounded, Receiver, Sender};
use futures::future::{lazy, Future, IntoFuture};
use reqwest::{header, StatusCode};
use tokio::runtime::{Builder, Runtime};
use tokio_timer::clock::Clock;

use crate::event::Event;
use crate::eventdata::EventData;
use crate::response::{HoneyResponse, Response};

type Events = Vec<Event>;

const BATCH_ENDPOINT: &str = "/1/batch/";

const DEFAULT_NAME_PREFIX: &str = "libhoney-rust";
// DEFAULT_MAX_BATCH_SIZE how many events to collect in a batch
const DEFAULT_MAX_BATCH_SIZE: usize = 50;
// DEFAULT_MAX_CONCURRENT_BATCHES how many batches to maintain in parallel
const DEFAULT_MAX_CONCURRENT_BATCHES: usize = 80;
// DEFAULT_BATCH_TIMEOUT how frequently to send unfilled batches
const DEFAULT_BATCH_TIMEOUT: Duration = Duration::from_millis(100);
// DEFAULT_PENDING_WORK_CAPACITY how many events to queue up for busy batches
const DEFAULT_PENDING_WORK_CAPACITY: usize = 10_000;

/// TransmissionOptions includes various options to tweak the behavious of the sender.
#[derive(Debug, Clone)]
pub struct TransmissionOptions {
    /// how many events to collect into a batch before sending. Overrides
    /// DEFAULT_MAX_BATCH_SIZE.
    pub max_batch_size: usize,

    /// how many batches can be inflight simultaneously. Overrides
    /// DEFAULT_MAX_CONCURRENT_BATCHES.
    pub max_concurrent_batches: usize,

    /// how often to send off batches. Overrides DEFAULT_BATCH_TIMEOUT. (TODO(nlopes):
    /// Currently unused)
    pub batch_timeout: Duration,

    /// how many events to allow to pile up. Overrides DEFAULT_PENDING_WORK_CAPACITY
    pub pending_work_capacity: usize,

    /// user_agent_addition is an option that allows you to augment the "User-Agent"
    /// header that libhoney sends along with each event.  The default User-Agent is
    /// "libhoney-go/<version>". If you set this variable, its contents will be appended
    /// to the User-Agent string, separated by a space. The expected format is
    /// product-name/version, eg "myapp/1.0"
    pub user_agent_addition: Option<String>,
}

impl Default for TransmissionOptions {
    fn default() -> Self {
        TransmissionOptions {
            max_batch_size: DEFAULT_MAX_BATCH_SIZE,
            max_concurrent_batches: DEFAULT_MAX_CONCURRENT_BATCHES,
            batch_timeout: DEFAULT_BATCH_TIMEOUT,
            pending_work_capacity: DEFAULT_PENDING_WORK_CAPACITY,
            user_agent_addition: None,
        }
    }
}

/// Transmission handles collecting and sending individual events to Honeycomb
#[derive(Debug)]
pub struct Transmission {
    pub(crate) options: TransmissionOptions,
    user_agent: String,

    runtime: Runtime,

    work_sender: Sender<Event>,
    work_receiver: Receiver<Event>,
    response_sender: Sender<Response>,
    response_receiver: Receiver<Response>,

    responses: Arc<Mutex<Vec<Response>>>,
}

impl Drop for Transmission {
    fn drop(&mut self) {
        self.stop()
    }
}

impl Transmission {
    fn new_runtime(options: Option<&TransmissionOptions>) -> Runtime {
        let mut builder = Builder::new();
        if let Some(opts) = options {
            builder
                .blocking_threads(opts.max_concurrent_batches)
                .core_threads(opts.max_concurrent_batches);
        };
        builder
            .clock(Clock::system())
            .keep_alive(Some(Duration::from_secs(60)))
            .name_prefix("libhoney-rust")
            .stack_size(3 * 1024 * 1024)
            .build()
            .unwrap()
    }

    pub(crate) fn new(options: TransmissionOptions) -> Self {
        let runtime = Transmission::new_runtime(None);

        let (work_sender, work_receiver) = bounded(options.pending_work_capacity * 4);
        let (response_sender, response_receiver) = bounded(options.pending_work_capacity * 4);

        Transmission {
            options,
            runtime,
            work_sender,
            work_receiver,
            response_sender,
            response_receiver,
            responses: Arc::new(Mutex::new(Vec::new())),
            user_agent: format!("{}/{}", DEFAULT_NAME_PREFIX, env!("CARGO_PKG_VERSION")),
        }
    }

    // TODO: return Result
    pub(crate) fn start(&mut self) {
        let work_receiver = self.work_receiver.clone();
        let response_sender = self.response_sender.clone();
        let options = self.options.clone();
        let user_agent = self.user_agent.clone();

        // Batch sending thread
        self.runtime.spawn(lazy(|| {
            Transmission::sender(work_receiver, response_sender, options, user_agent)
        }));
    }

    fn sender(
        work_receiver: Receiver<Event>,
        response_sender: Sender<Response>,
        options: TransmissionOptions,
        user_agent: String,
    ) -> impl Future<Item = (), Error = ()> {
        let mut runtime = Transmission::new_runtime(Some(&options));
        let mut batch: Events = Vec::with_capacity(options.max_batch_size);

        loop {
            let options = options.clone();
            match work_receiver.recv() {
                Ok(event) => {
                    if event.fields.contains_key("internal_stop_event") {
                        break;
                    }
                    batch.push(event);
                }
                Err(e) => {
                    //TODO(nlopes): this is not enough
                    eprintln!("ERROR: {:?}", e);
                }
            };

            if batch.len() >= options.max_batch_size {
                let batch_copy = batch.clone();
                let batch_response_sender = response_sender.clone();
                let batch_user_agent = user_agent.clone();

                runtime.spawn(lazy(move || {
                    for response in Transmission::send_batch(
                        batch_copy,
                        options,
                        batch_user_agent,
                        SystemTime::now(),
                    ) {
                        batch_response_sender.send(response).unwrap();
                    }
                    Ok(()).into_future()
                }));
                batch.clear();
            }
        }

        runtime.shutdown_now().wait().unwrap();
        Ok(()).into_future()
    }

    // TODO(nlopes): check timestamps (both setting up and updating them). Also make sure
    // we're measuring both queue times and total times
    fn send_batch(
        events: Events,
        options: TransmissionOptions,
        user_agent: String,
        clock: SystemTime,
    ) -> Vec<Response> {
        let mut opts: crate::ClientOptions = Default::default();
        let mut payload: Vec<EventData> = Vec::new();

        for event in &events {
            opts = event.options.clone();
            payload.push(EventData {
                data: event.fields.clone(),
                time: event.timestamp,
                samplerate: event.options.sample_rate,
            })
        }

        let endpoint = format!("{}{}{}", opts.api_host, BATCH_ENDPOINT, &opts.dataset);
        let client = reqwest::Client::new();

        let user_agent = if let Some(ua_addition) = options.user_agent_addition {
            format!("{}{}", user_agent, ua_addition)
        } else {
            user_agent
        };

        match client
            .post(&endpoint)
            .header(header::USER_AGENT, user_agent)
            .header(header::CONTENT_TYPE, "application/json")
            .header("X-Honeycomb-Team", opts.api_key)
            .json(&payload)
            .send()
        {
            Ok(mut res) => match res.status() {
                StatusCode::OK => {
                    let honey_responses: Vec<HoneyResponse> = res.json().unwrap();

                    honey_responses
                        .iter()
                        .zip(events.iter())
                        .map(|(hr, e)| Response {
                            status_code: StatusCode::from_u16(hr.clone().status as u16).unwrap(),
                            body: "".to_string(),
                            duration: clock.elapsed().unwrap(),
                            metadata: e.metadata.clone(),
                            error: hr.error.clone(),
                        })
                        .collect()
                }
                status => events
                    .iter()
                    .map(|e| Response {
                        status_code: status,
                        body: res.text().unwrap(),
                        duration: clock.elapsed().unwrap(),
                        metadata: e.metadata.clone(),
                        error: None,
                    })
                    .collect(),
            },
            Err(e) => panic!("What should we do? Error: {}", e),
        }
    }

    pub(crate) fn stop(&mut self) {
        self.work_sender.send(Event::stop_event()).unwrap();
    }

    pub(crate) fn send(&mut self, event: Event) {
        if !self.work_sender.is_full() {
            self.runtime.spawn(
                self.work_sender
                    .clone()
                    .send_timeout(event, Duration::from_millis(500))
                    .map_err(|e| {
                        eprintln!("Error when adding: {:#?}", e);
                    })
                    .into_future(),
            );
        }
    }

    /// responses provides access to the receiver
    pub fn responses(&self) -> Receiver<Response> {
        self.response_receiver.clone()
    }
}

#[cfg(test)]
mod tests {
    use mockito;
    use reqwest::StatusCode;

    use super::*;
    use crate::ClientOptions;

    #[test]
    fn test_defaults() {
        let transmission = Transmission::new(Default::default());
        assert_eq!(
            transmission.user_agent,
            format!("{}/{}", DEFAULT_NAME_PREFIX, env!("CARGO_PKG_VERSION"))
        );

        assert_eq!(transmission.options.max_batch_size, DEFAULT_MAX_BATCH_SIZE);
        assert_eq!(transmission.options.batch_timeout, DEFAULT_BATCH_TIMEOUT);
        assert_eq!(
            transmission.options.max_concurrent_batches,
            DEFAULT_MAX_CONCURRENT_BATCHES
        );
        assert_eq!(
            transmission.options.pending_work_capacity,
            DEFAULT_PENDING_WORK_CAPACITY
        );
    }

    #[test]
    fn test_modifiable_defaults() {
        let transmission = Transmission::new(TransmissionOptions {
            user_agent_addition: Some(" something/0.3".to_string()),
            ..Default::default()
        });
        assert_eq!(
            transmission.options.user_agent_addition,
            Some(" something/0.3".to_string())
        );
    }

    #[test]
    fn test_responses() {
        use crate::fields::FieldHolder;

        let mut transmission = Transmission::new(TransmissionOptions {
            max_batch_size: 5,
            ..Default::default()
        });
        transmission.start();

        let api_host = &mockito::server_url();
        let _m = mockito::mock(
            "POST",
            mockito::Matcher::Regex(r"/1/batch/(.*)$".to_string()),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"
[
  { "status":202 },
  { "status":202 },
  { "status":202 },
  { "status":202 },
  { "status":202 }
]
"#,
        )
        .create();

        for i in 0..5 {
            let mut event = Event::new(&ClientOptions {
                api_key: "some_api_key".to_string(),
                api_host: api_host.to_string(),
                ..Default::default()
            });
            event.add_field("id", serde_json::from_str(&i.to_string()).unwrap());
            transmission.send(event);
        }
        for (i, response) in transmission.responses().iter().enumerate() {
            if i == 4 {
                break;
            }
            assert_eq!(response.status_code, StatusCode::ACCEPTED);
            assert_eq!(response.body, "".to_string());
        }
        transmission.stop();
    }

    #[test]
    fn test_metadata() {
        use serde_json::json;

        let mut transmission = Transmission::new(TransmissionOptions {
            max_batch_size: 1,
            ..Default::default()
        });
        transmission.start();

        let api_host = &mockito::server_url();
        let _m = mockito::mock(
            "POST",
            mockito::Matcher::Regex(r"/1/batch/(.*)$".to_string()),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"
[
  { "status":202 }
]
"#,
        )
        .create();

        let mut event = Event::new(&ClientOptions {
            api_key: "some_api_key".to_string(),
            api_host: api_host.to_string(),
            ..Default::default()
        });
        event.metadata = Some(json!("some metadata in a string"));
        transmission.send(event);

        if let Some(response) = transmission.responses().iter().next() {
            assert_eq!(response.status_code, StatusCode::ACCEPTED);
            assert_eq!(response.metadata, Some(json!("some metadata in a string")));
        } else {
            panic!("did not receive an expected response");
        }
        transmission.stop();
    }

    #[test]
    fn test_bad_response() {
        use serde_json::json;

        let mut transmission = Transmission::new(TransmissionOptions {
            max_batch_size: 1,
            ..Default::default()
        });
        transmission.start();

        let api_host = &mockito::server_url();
        let _m = mockito::mock(
            "POST",
            mockito::Matcher::Regex(r"/1/batch/(.*)$".to_string()),
        )
        .with_status(400)
        .with_header("content-type", "application/json")
        .with_body("request body is malformed and cannot be read as JSON")
        .create();

        let mut event = Event::new(&ClientOptions {
            api_key: "some_api_key".to_string(),
            api_host: api_host.to_string(),
            ..Default::default()
        });

        event.metadata = Some(json!("some metadata in a string"));
        transmission.send(event);

        if let Some(response) = transmission.responses().iter().next() {
            assert_eq!(response.status_code, StatusCode::BAD_REQUEST);
            assert_eq!(
                response.body,
                "request body is malformed and cannot be read as JSON".to_string()
            );
        } else {
            panic!("did not receive an expected response");
        }
        transmission.stop();
    }
}
