/*! Transmission handles the transmission of events to Honeycomb

*/
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_channel::{bounded, Receiver as ChannelReceiver, Sender as ChannelSender};
use async_std::future;
use async_std::sync::{Condvar, Mutex};
use async_trait::async_trait;
use derivative::Derivative;
use futures::channel::oneshot;
use futures::executor;
use futures::task::{Spawn, SpawnExt};
use log::{debug, error, info, trace};
use reqwest::{header, StatusCode};

use crate::errors::{Error, Result};
use crate::event::Event;
use crate::eventdata::EventData;
use crate::events::{Events, EventsResponse};
use crate::response::{HoneyResponse, Response};
use crate::sender::{Sender, StopFuture};
use crate::FutureExecutor;

// Re-export reqwest client to help users avoid versioning issues.
pub use reqwest::{Client as HttpClient, ClientBuilder as HttpClientBuilder};

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
const DEFAULT_SEND_TIMEOUT: Duration = Duration::from_millis(1_000);

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

#[derive(Debug)]
enum QueueEvent {
    Data(Event),
    Stop(oneshot::Sender<()>),
}

/// `Transmission` handles collecting and sending individual events to Honeycomb
#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct Transmission {
    pub(crate) options: Options,
    user_agent: String,

    #[derivative(Debug = "ignore")]
    executor: FutureExecutor,
    http_client: reqwest::Client,

    work_sender: ChannelSender<QueueEvent>,
    work_receiver: ChannelReceiver<QueueEvent>,
    response_sender: ChannelSender<Response>,
    response_receiver: ChannelReceiver<Response>,
}

impl Drop for Transmission {
    fn drop(&mut self) {
        // Wait for the stop event to bue queued, but don't wait for the queue to be flushed.
        // Clients should call stop() themselves if they want to block.
        executor::block_on(self.stop())
            .map(|_: StopFuture| ())
            .unwrap_or_else(|err| error!("Failed to enqueue stop event: {}", err));
    }
}

#[async_trait]
impl Sender for Transmission {
    fn start(&mut self) -> Result<()> {
        let work_receiver = self.work_receiver.clone();
        let response_sender = self.response_sender.clone();
        let options = self.options.clone();
        let user_agent = self.user_agent.clone();
        let executor = self.executor.clone();
        let http_client = self.http_client.clone();

        info!("transmission starting");
        // Task that processes all the work received.
        executor
            .spawn(Self::process_work(
                work_receiver,
                response_sender,
                executor.clone(),
                options,
                user_agent,
                http_client,
            ))
            .map_err(Error::from)
    }

    async fn stop(&mut self) -> Result<StopFuture> {
        info!("transmission stopping");
        if self.work_sender.is_full() {
            error!("work sender is full");
            return Err(Error::sender_full("work"));
        }

        trace!("Sending stop event");
        let (sender, receiver) = oneshot::channel();
        self.work_sender.send(QueueEvent::Stop(sender)).await?;
        Ok(Box::new(receiver))
    }

    async fn send(&self, event: Event) {
        let clock = Instant::now();
        if self.work_sender.is_full() {
            error!("work sender is full");
            self.response_sender
                .send(Response {
                    status_code: None,
                    body: None,
                    duration: clock.elapsed(),
                    metadata: event.metadata,
                    error: Some("queue overflow".to_string()),
                })
                .await
                .unwrap_or_else(|e| {
                    error!("response dropped, error: {}", e);
                });
        } else {
            let work_sender = self.work_sender.clone();
            let response_sender = self.response_sender.clone();
            if let Err(e) = future::timeout(
                DEFAULT_SEND_TIMEOUT,
                work_sender.clone().send(QueueEvent::Data(event.clone())),
            )
            .await
            {
                response_sender
                    .send(Response {
                        status_code: None,
                        body: None,
                        duration: clock.elapsed(),
                        metadata: event.metadata,
                        error: Some(e.to_string()),
                    })
                    .await
                    .unwrap_or_else(|e| {
                        error!("response dropped, error: {}", e);
                    });
            }
        }
    }

    /// responses provides access to the receiver
    fn responses(&self) -> ChannelReceiver<Response> {
        self.response_receiver.clone()
    }
}

struct BatchWaiter {
    batches_in_progress: Arc<Mutex<usize>>,
    condvar: Arc<Condvar>,
}
impl BatchWaiter {
    pub fn new() -> Self {
        Self {
            batches_in_progress: Arc::new(Mutex::new(0)),
            condvar: Arc::new(Condvar::new()),
        }
    }

    // Takes a mutable reference so that only the top-level caller can spawn batches.
    pub async fn start_batch(&mut self) -> BatchWaiterGuard {
        let mut lock_guard = self.batches_in_progress.lock().await;
        *lock_guard += 1;
        BatchWaiterGuard {
            batches_in_progress: self.batches_in_progress.clone(),
            condvar: self.condvar.clone(),
        }
    }

    pub async fn wait_for_completion(self) {
        let mut lock_guard = self.batches_in_progress.lock().await;
        while *lock_guard != 0 {
            lock_guard = self.condvar.wait(lock_guard).await;
        }
    }
}

#[must_use]
struct BatchWaiterGuard {
    batches_in_progress: Arc<Mutex<usize>>,
    condvar: Arc<Condvar>,
}
impl BatchWaiterGuard {
    // We can't implement this using Drop since Drop::drop can't use async.
    async fn end_batch(self) {
        let mut lock_guard = self.batches_in_progress.lock().await;
        *lock_guard -= 1;
        self.condvar.notify_one();
    }
}

impl Transmission {
    pub(crate) fn new(executor: FutureExecutor, options: Options) -> Result<Self> {
        let (work_sender, work_receiver) = bounded(options.pending_work_capacity * 4);
        let (response_sender, response_receiver) = bounded(options.pending_work_capacity * 4);

        Ok(Self {
            executor,
            options,
            work_sender,
            work_receiver,
            response_sender,
            response_receiver,
            user_agent: format!("{}/{}", DEFAULT_NAME_PREFIX, env!("CARGO_PKG_VERSION")),
            http_client: reqwest::Client::new(),
        })
    }

    /// Sets a custom reqwest client.
    pub fn set_http_client(&mut self, http_client: HttpClient) {
        self.http_client = http_client;
    }

    async fn process_work(
        work_receiver: ChannelReceiver<QueueEvent>,
        response_sender: ChannelSender<Response>,
        executor: Arc<dyn Spawn + Send + Sync>,
        options: Options,
        user_agent: String,
        http_client: reqwest::Client,
    ) {
        let mut batches: HashMap<String, Events> = HashMap::new();
        let mut expired = false;
        let mut batches_in_progress = BatchWaiter::new();

        let stop_sender = loop {
            let options = options.clone();

            let stop_sender =
                match future::timeout(options.batch_timeout, work_receiver.recv()).await {
                    Ok(Ok(QueueEvent::Stop(sender))) => {
                        debug!("Processing stop event");
                        Some(sender)
                    }
                    Ok(Ok(QueueEvent::Data(event))) => {
                        trace!(
                            "Processing data event for dataset `{}`",
                            event.options.dataset,
                        );
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
                        None
                    }
                    Err(future::TimeoutError { .. }) => {
                        expired = true;
                        None
                    }
                    Ok(Err(recv_err)) => {
                        error!("Error receiving from work channel: {}", recv_err);
                        // TODO(nlopes): is this the right behaviour?
                        break None;
                    }
                };

            let mut batches_sent = Vec::new();
            for (batch_name, batch) in batches.iter_mut() {
                if batch.is_empty() {
                    break;
                }
                let options = options.clone();

                let should_process_batch = if batch.len() >= options.max_batch_size {
                    trace!("Batch size exceeded with {} event(s)", batch.len());
                    true
                } else if expired {
                    trace!("Timer expired with {} event(s)", batch.len());
                    true
                } else if stop_sender.is_some() {
                    trace!("Shutting down worker and flushing {} event(s)", batch.len());
                    true
                } else {
                    false
                };

                if should_process_batch {
                    let batch_copy = batch.clone();
                    let batch_response_sender = response_sender.clone();
                    let batch_user_agent = user_agent.to_string();
                    // This is a shallow clone that allows reusing HTTPS connections across batches.
                    // From the reqwest docs:
                    //   "You do not have to wrap the Client it in an Rc or Arc to reuse it, because
                    //    it already uses an Arc internally."
                    let client_copy = http_client.clone();

                    let batch_guard = batches_in_progress.start_batch().await;
                    match executor.spawn(async move {
                        for response in Self::send_batch(
                            batch_copy,
                            options,
                            batch_user_agent,
                            Instant::now(),
                            client_copy,
                        )
                        .await
                        {
                            batch_response_sender
                                .send(response)
                                .await
                                .expect("unable to enqueue batch response");
                        }
                        batch_guard.end_batch().await;
                    }) {
                        Ok(_) => {}
                        Err(spawn_err) => {
                            error!("Failed to spawn task to send batch: {}", spawn_err);
                        }
                    }

                    batches_sent.push(batch_name.to_string())
                }
            }
            // clear all sent batches
            batches_sent.iter_mut().for_each(|name| {
                batches.remove(name);
            });

            if stop_sender.is_some() {
                break stop_sender;
            } else if expired {
                // If we get here and we were expired, then we've already triggered a send, so
                // we reset this to ensure it kicks off again
                expired = false;
            }
        };

        // Wait for all in-progress batches to be sent before completing the worker task. This
        // ensures that waiting on the worker task to complete also waits on any batches to
        // finish being sent.
        batches_in_progress.wait_for_completion().await;

        if let Some(sender) = stop_sender {
            sender.send(()).unwrap_or_else(|()| {
                // This happens if the caller doesn't await the future.
                error!("Stop receiver was dropped before notification was sent")
            });
        }
    }

    async fn send_batch(
        events: Events,
        options: Options,
        user_agent: String,
        clock: Instant,
        client: reqwest::Client,
    ) -> Vec<Response> {
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
    use crate::test::run_with_supported_executors;

    #[test]
    fn test_defaults() {
        run_with_supported_executors(|executor| async move {
            let transmission = Transmission::new(executor, Options::default()).unwrap();
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
        })
    }

    #[test]
    fn test_modifiable_defaults() {
        run_with_supported_executors(|executor| async move {
            let transmission = Transmission::new(
                executor,
                Options {
                    user_agent_addition: Some(" something/0.3".to_string()),
                    ..Options::default()
                },
            )
            .unwrap();
            assert_eq!(
                transmission.options.user_agent_addition,
                Some(" something/0.3".to_string())
            );
        })
    }

    #[test]
    fn test_responses() {
        run_with_supported_executors(|executor| async move {
            use crate::fields::FieldHolder;

            let mut transmission = Transmission::new(
                executor,
                Options {
                    max_batch_size: 5,
                    ..Options::default()
                },
            )
            .unwrap();
            transmission.start().unwrap();

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

            for i in 0i32..5 {
                let mut event = Event::new(&client::Options {
                    api_key: "some_api_key".to_string(),
                    api_host: api_host.to_string(),
                    ..client::Options::default()
                });
                event.add_field("id", serde_json::from_str(&i.to_string()).unwrap());
                transmission.send(event).await;
            }
            for _i in 0i32..5 {
                let response = transmission.responses().recv().await.unwrap();
                assert_eq!(response.status_code, Some(StatusCode::ACCEPTED));
                assert_eq!(response.body, None);
            }
            transmission.stop().await.unwrap().await.unwrap();
        })
    }

    #[test]
    fn test_metadata() {
        run_with_supported_executors(|executor| async move {
            use serde_json::json;

            let mut transmission = Transmission::new(
                executor,
                Options {
                    max_batch_size: 1,
                    ..Options::default()
                },
            )
            .unwrap();
            transmission.start().unwrap();

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
            transmission.send(event).await;

            if let Ok(response) = transmission.responses().recv().await {
                assert_eq!(response.status_code, Some(StatusCode::ACCEPTED));
                assert_eq!(response.metadata, metadata);
            } else {
                panic!("did not receive an expected response");
            }
            transmission.stop().await.unwrap().await.unwrap();
        })
    }

    #[test]
    fn test_multiple_batches() {
        run_with_supported_executors(|executor| async move {
            // What we try to test here is if events are sent in separate batches, depending
            // on their combination of api_host, api_key, dataset.
            //
            // For that, we set max_batch_size to 2, then we send 3 events, 2 with one
            // combination and 1 with another.  Only the two should be sent, and we should get
            // back two responses.
            use serde_json::json;
            let mut transmission = Transmission::new(
                executor,
                Options {
                    max_batch_size: 2,
                    batch_timeout: Duration::from_secs(5),
                    ..Options::default()
                },
            )
            .unwrap();
            transmission.start().unwrap();

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

            transmission.send(event3).await;
            transmission.send(event2).await;
            transmission.send(event1).await;

            let response1 = transmission.responses().recv().await.unwrap();
            let response2 = transmission.responses().recv().await.unwrap();
            let _ = future::timeout(Duration::from_millis(250), transmission.responses().recv())
                .await
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
            transmission.stop().await.unwrap().await.unwrap();
        })
    }

    #[test]
    fn test_bad_response() {
        run_with_supported_executors(|executor| async move {
            use serde_json::json;

            let mut transmission = Transmission::new(executor, Options::default()).unwrap();
            transmission.start().unwrap();

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
            transmission.send(event).await;

            if let Ok(response) = transmission.responses().recv().await {
                assert_eq!(response.status_code, Some(StatusCode::BAD_REQUEST));
                assert_eq!(
                    response.body,
                    Some("request body is malformed and cannot be read as JSON".to_string())
                );
            } else {
                panic!("did not receive an expected response");
            }
            transmission.stop().await.unwrap().await.unwrap();
        })
    }
}
