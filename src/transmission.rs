/*! Transmission handles the transmission of events to Honeycomb

*/
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc::{channel as bounded, Receiver as ChannelReceiver, Sender as ChannelSender};

use log::{debug, error, info, trace};
use reqwest::{header, StatusCode};
use tokio::runtime::{Builder, Handle, Runtime};
use tokio::sync::Mutex;

use crate::errors::Result;
use crate::event::Event;
use crate::eventdata::EventData;
use crate::events::{Events, EventsResponse};
use crate::response::{HoneyResponse, Response};
use crate::sender::Sender;

const BATCH_ENDPOINT: &str = "/1/batch/";

const DEFAULT_NAME_PREFIX: &str = "libhoney-rust";
// DEFAULT_MAX_BATCH_SIZE how many events to collect in a batch
const DEFAULT_MAX_BATCH_SIZE: usize = 50;
// DEFAULT_MAX_CONCURRENT_BATCHES how many batches to maintain in parallel
const DEFAULT_MAX_CONCURRENT_BATCHES: usize = 10;
// DEFAULT_BATCH_TIMEOUT how frequently to send unfilled batches
const DEFAULT_BATCH_TIMEOUT: Duration = Duration::from_millis(100);
// DEFAULT_PENDING_WORK_CAPACITY how many events to queue up for busy batches
const DEFAULT_PENDING_WORK_CAPACITY: usize = 10_000;
// DEFAULT_SEND_TIMEOUT how much to wait to send an event
const DEFAULT_SEND_TIMEOUT: Duration = Duration::from_millis(5_000);

/// Options includes various options to tweak the behavious of the sender.
#[derive(Debug, Clone)]
pub struct Options {
    /// how many events to collect into a batch before sending. Overrides
    /// DEFAULT_MAX_BATCH_SIZE.
    pub max_batch_size: usize,

    /// how many batches can be inflight simultaneously. Overrides
    /// DEFAULT_MAX_CONCURRENT_BATCHES.
    pub max_concurrent_batches: usize,

    /// how often to send off batches. Overrides DEFAULT_BATCH_TIMEOUT.
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

impl Default for Options {
    fn default() -> Self {
        Self {
            max_batch_size: DEFAULT_MAX_BATCH_SIZE,
            max_concurrent_batches: DEFAULT_MAX_CONCURRENT_BATCHES,
            batch_timeout: DEFAULT_BATCH_TIMEOUT,
            pending_work_capacity: DEFAULT_PENDING_WORK_CAPACITY,
            user_agent_addition: None,
        }
    }
}

/// `Transmission` handles collecting and sending individual events to Honeycomb
#[derive(Debug, Clone)]
pub struct Transmission {
    pub(crate) options: Options,
    user_agent: String,

    runtime: Arc<Mutex<Runtime>>,
    handle: Handle,
    http_client: reqwest::Client,

    work_sender: ChannelSender<Event>,
    work_receiver: Arc<Mutex<ChannelReceiver<Event>>>,
}

impl Drop for Transmission {
    fn drop(&mut self) {
        self.stop().unwrap();
    }
}

impl Sender for Transmission {
    fn start(&mut self) {
        let work_receiver = self.work_receiver.clone();
        let options = self.options.clone();
        let user_agent = self.user_agent.clone();
        let http_client = self.http_client.clone();

        info!("transmission starting");
        // thread that processes all the work received
        let handle = self.handle.clone();
        self.handle.spawn(async {
            Self::process_work(handle, work_receiver, options, user_agent, http_client).await
        });
    }

    fn stop(&mut self) -> Result<()> {
        info!("transmission stopping");
        let sender = self.work_sender.clone();
        self.handle.spawn(async move {
            let _ = sender
                .send(Event::stop_event())
                .await
                .map_err(|e| error!("failed to send stop: {:?}", e));
        });

        Ok(())
    }

    fn send(&mut self, event: Event) {
        trace!("in transmission send");

        let work_sender = self.work_sender.clone();

        self.handle.spawn(async move {
            trace!("in spawn work sender send");

            if let Err(e) = work_sender
                .send_timeout(event.clone(), DEFAULT_SEND_TIMEOUT)
                .await
            {
                error!("response dropped, error: {}", e);
            }
        });
    }
}

impl Transmission {
    fn new_runtime(options: Option<&Options>) -> Result<Runtime> {
        let mut builder = Builder::new_multi_thread();
        if let Some(opts) = options {
            builder.worker_threads(opts.max_concurrent_batches);
        };
        Ok(builder
            .thread_name("libhoney-rust")
            .thread_stack_size(3 * 1024 * 1024)
            .enable_io()
            .enable_time()
            .build()?)
    }

    pub(crate) fn new(options: Options) -> Result<Self> {
        let runtime = Self::new_runtime(None)?;

        let (work_sender, work_receiver) = bounded(options.pending_work_capacity * 4);

        Ok(Self {
            handle: runtime.handle().clone(),
            runtime: Arc::new(Mutex::new(runtime)),
            options,
            work_sender,
            work_receiver: Arc::new(Mutex::new(work_receiver)),
            user_agent: format!("{}/{}", DEFAULT_NAME_PREFIX, env!("CARGO_PKG_VERSION")),
            http_client: reqwest::Client::new(),
        })
    }

    async fn process_work(
        handle: Handle,
        work_receiver: Arc<Mutex<ChannelReceiver<Event>>>,
        options: Options,
        user_agent: String,
        http_client: reqwest::Client,
    ) {
        let mut batches: HashMap<String, Events> = HashMap::new();
        let mut expired = false;

        loop {
            info!("in process work loop");
            let options = options.clone();

            let mut work = work_receiver.lock().await;
            tokio::select! {
                next = work.recv() => {
                    match next {
                        None => {
                          error!("channel closed");
                        },
                        Some(event) => {
                            debug!("got event");

                            if event.fields.contains_key("internal_stop_event") {
                                info!("got 'internal_stop_event' event");
                                break;
                            }
                            let key = format!(
                                "{}_{}_{}",
                                event.options.api_host, event.options.api_key, event.options.dataset
                            );
                            batches
                                .entry(key)
                                .and_modify(|v| v.push(event.clone()))
                                .or_insert({
                                    let mut v = Vec::with_capacity(options.max_batch_size);
                                    v.push(event);
                                    v
                                });
                        }
                    }
                },
                _ = tokio::time::sleep(options.batch_timeout) => {
                    expired = true;
                }
            };

            debug!("batches length: {:?}", batches.len());

            let mut batches_sent = Vec::new();
            for (batch_name, batch) in batches.iter_mut() {
                if batch.is_empty() {
                    debug!("empty batch");
                    break;
                }
                let options = options.clone();

                if batch.len() >= options.max_batch_size || expired {
                    trace!(
                        "Timer expired or batch size exceeded with {} event(s)",
                        batch.len()
                    );
                    let batch_copy = batch.clone();
                    let batch_user_agent = user_agent.to_string();
                    // This is a shallow clone that allows reusing HTTPS connections across batches.
                    // From the reqwest docs:
                    //   "You do not have to wrap the Client it in an Rc or Arc to reuse it, because
                    //    it already uses an Arc internally."
                    let client_copy = http_client.clone();

                    handle.spawn(async move {
                        debug!("in runtime spawn");

                        for response in Self::send_batch(
                            batch_copy,
                            options,
                            batch_user_agent,
                            Instant::now(),
                            client_copy,
                        )
                        .await
                        {
                            debug!("sent batch: {:?}", response.status_code);
                        }
                    });
                    batches_sent.push(batch_name.to_string())
                }
            }
            // clear all sent batches
            batches_sent.iter_mut().for_each(|name| {
                batches.remove(name);
            });

            // If we get here and we were expired, then we've already triggered a send, so
            // we reset this to ensure it kicks off again
            if expired {
                expired = false;
            }
        }
    }

    async fn send_batch(
        events: Events,
        options: Options,
        user_agent: String,
        clock: Instant,
        client: reqwest::Client,
    ) -> Vec<Response> {
        debug!("in send batch");

        let mut opts: crate::client::Options = crate::client::Options::default();
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

        let user_agent = if let Some(ua_addition) = options.user_agent_addition {
            format!("{}{}", user_agent, ua_addition)
        } else {
            user_agent
        };

        trace!("Sending payload: {:#?}", payload);
        let response = client
            .post(&endpoint)
            .header(header::USER_AGENT, user_agent)
            .header(header::CONTENT_TYPE, "application/json")
            .header("X-Honeycomb-Team", opts.api_key)
            .json(&payload)
            .send()
            .await;

        trace!("Received response: {:#?}", response);
        match response {
            Ok(res) => match res.status() {
                StatusCode::OK => {
                    let responses: Vec<HoneyResponse>;
                    match res.json().await {
                        Ok(r) => responses = r,
                        Err(e) => {
                            return events.to_response(None, None, clock, Some(e.to_string()));
                        }
                    }
                    let total_responses = if responses.is_empty() {
                        1
                    } else {
                        responses.len() as u64
                    };

                    let spent = Duration::from_secs(clock.elapsed().as_secs() / total_responses);

                    responses
                        .iter()
                        .zip(events.iter())
                        .map(|(hr, e)| Response {
                            status_code: StatusCode::from_u16(hr.status).ok(),
                            body: None,
                            duration: spent,
                            metadata: e.metadata.clone(),
                            error: hr.error.clone(),
                        })
                        .collect()
                }
                status => {
                    let body = match res.text().await {
                        Ok(t) => t,
                        Err(e) => format!("HTTP Error but could not read response body: {}", e),
                    };
                    events.to_response(Some(status), Some(body), clock, None)
                }
            },
            Err(err) => events.to_response(None, None, clock, Some(err.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;

    use super::*;
    use crate::client;

    #[test]
    fn test_defaults() {
        let transmission = Transmission::new(Options::default()).unwrap();
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
        let transmission = Transmission::new(Options {
            user_agent_addition: Some(" something/0.3".to_string()),
            ..Options::default()
        })
        .unwrap();
        assert_eq!(
            transmission.options.user_agent_addition,
            Some(" something/0.3".to_string())
        );
    }

    #[test]
    fn test_responses() {
        use crate::fields::FieldHolder;

        let mut transmission = Transmission::new(Options {
            max_batch_size: 5,
            ..Options::default()
        })
        .unwrap();
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
            let mut event = Event::new(&client::Options {
                api_key: "some_api_key".to_string(),
                api_host: api_host.to_string(),
                ..client::Options::default()
            });
            event.add_field("id", serde_json::from_str(&i.to_string()).unwrap());
            transmission.send(event);
        }
        for (i, response) in transmission.responses().iter().enumerate() {
            if i == 4 {
                break;
            }
            assert_eq!(response.status_code, Some(StatusCode::ACCEPTED));
            assert_eq!(response.body, None);
        }
        transmission.stop().unwrap();
    }

    #[test]
    fn test_metadata() {
        use serde_json::json;

        let mut transmission = Transmission::new(Options {
            max_batch_size: 1,
            ..Options::default()
        })
        .unwrap();
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

        let metadata = Some(json!("some metadata in a string"));
        let mut event = Event::new(&client::Options {
            api_key: "some_api_key".to_string(),
            api_host: api_host.to_string(),
            ..client::Options::default()
        });
        event.metadata = metadata.clone();
        transmission.send(event);

        if let Some(response) = transmission.responses().iter().next() {
            assert_eq!(response.status_code, Some(StatusCode::ACCEPTED));
            assert_eq!(response.metadata, metadata);
        } else {
            panic!("did not receive an expected response");
        }
        transmission.stop().unwrap();
    }

    #[test]
    fn test_multiple_batches() {
        // What we try to test here is if events are sent in separate batches, depending
        // on their combination of api_host, api_key, dataset.
        //
        // For that, we set max_batch_size to 2, then we send 3 events, 2 with one
        // combination and 1 with another.  Only the two should be sent, and we should get
        // back two responses.
        use serde_json::json;
        let mut transmission = Transmission::new(Options {
            max_batch_size: 2,
            batch_timeout: Duration::from_secs(5),
            ..Options::default()
        })
        .unwrap();
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
  { "status":202 }
]"#,
        )
        .create();

        let mut event1 = Event::new(&client::Options {
            api_key: "some_api_key".to_string(),
            api_host: api_host.to_string(),
            dataset: "same".to_string(),
            ..client::Options::default()
        });
        event1.metadata = Some(json!("event1"));
        let mut event2 = event1.clone();
        event2.metadata = Some(json!("event2"));
        let mut event3 = event1.clone();
        event3.options.dataset = "other".to_string();
        event3.metadata = Some(json!("event3"));

        transmission.send(event3);
        transmission.send(event2);
        transmission.send(event1);

        let response1 = transmission.responses().iter().next().unwrap();
        let response2 = transmission.responses().iter().next().unwrap();
        let _ = transmission
            .responses()
            .recv_timeout(Duration::from_millis(250))
            .err();

        assert_eq!(response1.status_code, Some(StatusCode::ACCEPTED));
        assert_eq!(response2.status_code, Some(StatusCode::ACCEPTED));

        // Responses can come out of order so we check against any of the metadata
        assert!(
            response1.metadata == Some(json!("event1"))
                || response1.metadata == Some(json!("event2"))
        );
        assert!(
            response2.metadata == Some(json!("event1"))
                || response2.metadata == Some(json!("event2"))
        );
        transmission.stop().unwrap();
    }

    #[test]
    fn test_bad_response() {
        use serde_json::json;

        let mut transmission = Transmission::new(Options::default()).unwrap();
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

        let mut event = Event::new(&client::Options {
            api_key: "some_api_key".to_string(),
            api_host: api_host.to_string(),
            ..client::Options::default()
        });

        event.metadata = Some(json!("some metadata in a string"));
        transmission.send(event);

        if let Some(response) = transmission.responses().iter().next() {
            assert_eq!(response.status_code, Some(StatusCode::BAD_REQUEST));
            assert_eq!(
                response.body,
                Some("request body is malformed and cannot be read as JSON".to_string())
            );
        } else {
            panic!("did not receive an expected response");
        }
        transmission.stop().unwrap();
    }
}
