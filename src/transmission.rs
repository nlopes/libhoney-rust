use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use crossbeam_channel::{bounded, Receiver, Sender};
use futures::future::{lazy, Future, IntoFuture};
use reqwest::header;
use tokio::runtime::{Builder, Runtime};
use tokio_timer::clock::Clock;

use crate::event::Event;
use crate::response::Response;

type Events = Vec<Event>;

const BATCH_ENDPOINT: &str = "/1/batch/";

const DEFAULT_NAME_PREFIX: &str = "libhoney-rust";
// DEFAULT_MAX_BATCH_SIZE how many events to collect in a batch
const DEFAULT_MAX_BATCH_SIZE: usize = 50;
// DEFAULT_MAX_CONCURRENT_BATCHES how many batches to maintain in parallel
const DEFAULT_MAX_CONCURRENT_BATCHES: usize = 80;
// DEFAULT_MAX_BATCH_TIMEOUT how frequently to send unfilled batches
const DEFAULT_MAX_BATCH_TIMEOUT: Duration = Duration::from_millis(100);
// DEFAULT_PENDING_WORK_CAPACITY how many events to queue up for busy batches
const DEFAULT_PENDING_WORK_CAPACITY: usize = 10_000;

/// TransmissionOptions includes various options to tweak the behavious of the sender.
#[derive(Debug, Clone)]
pub struct TransmissionOptions {
    // how many events to collect into a batch before sending. Overrides
    // DEFAULT_MAX_BATCH_SIZE.
    pub max_batch_size: usize,

    // how many batches can be inflight simultaneously. Overrides
    // DEFAULT_MAX_CONCURRENT_BATCHES.
    pub max_concurrent_batches: usize,


    pub max_batch_timeout: Duration,

    // how many events to allow to pile up. Overrides DEFAULT_PENDING_WORK_CAPACITY
    pub pending_work_capacity: usize,

    // user_agent_addition is a variable set at compile time via -ldflags to allow you to
    // augment the "User-Agent" header that libhoney sends along with each event.  The
    // default User-Agent is "libhoney-go/<version>". If you set this variable, its
    // contents will be appended to the User-Agent string, separated by a space. The
    // expected format is product-name/version, eg "myapp/1.0"
    pub user_agent_addition: Option<String>,
}

impl Default for TransmissionOptions {
    fn default() -> Self {
        TransmissionOptions {
            max_batch_size: DEFAULT_MAX_BATCH_SIZE,
            max_concurrent_batches: DEFAULT_MAX_CONCURRENT_BATCHES,
            max_batch_timeout: DEFAULT_MAX_BATCH_TIMEOUT,
            pending_work_capacity: DEFAULT_PENDING_WORK_CAPACITY,
            user_agent_addition: None,
        }
    }
}

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

    pub fn new(options: TransmissionOptions) -> Self {
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
    pub fn start(&mut self) {
        let work_receiver = self.work_receiver.clone();
        let response_sender = self.response_sender.clone();
        let response_receiver = self.response_receiver.clone();
        let options = self.options.clone();
        let responses = self.responses.clone();

        let user_agent = self.user_agent.clone();

        // Handle responses thread
        self.runtime.spawn(lazy(move || {
            Transmission::handle_responses(response_receiver, responses)
        }));

        // Batch sending thread
        self.runtime.spawn(lazy(move || {
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
                    eprintln!("ERROR: {:?}", e);
                }
            };

            if batch.len() >= options.max_batch_size {
                let batch_copy = batch.clone();
                let batch_response_sender = response_sender.clone();
                let batch_user_agent = user_agent.clone();

                runtime.spawn(lazy(move || {
                    let response = Transmission::send_batch(
                        batch_copy,
                        options,
                        batch_user_agent,
                        SystemTime::now(),
                    );
                    batch_response_sender.send(response).unwrap();
                    Ok(()).into_future()
                }));
                batch.clear();
            }
        }

        runtime.shutdown_now().wait().unwrap();
        Ok(()).into_future()
    }

    fn handle_responses(
        receiver: Receiver<Response>,
        responses: Arc<Mutex<Vec<Response>>>,
    ) -> impl Future<Item = (), Error = ()> {
        receiver
            .recv_timeout(Duration::from_millis(1000))
            .map(|s| responses.lock().unwrap().push(s))
            .map_err(|e| {
                eprintln!("Error when handling response: {}", e);
            })
            .into_future()
    }

    fn send_batch(
        events: Events,
        options: TransmissionOptions,
        user_agent: String,
        clock: SystemTime,
    ) -> Response {
        let mut opts: crate::ClientOptions = Default::default();
        // TODO(nlopes): send batch to honeycomb here
        for event in &events {
            opts = event.options.clone();
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
            .send()
        {
            Ok(mut res) => {
                return Response {
                    status_code: res.status(),
                    body: res.text().unwrap(),
                    duration: clock.elapsed().unwrap(),
                };
            }
            Err(e) => panic!("What should we do? Error: {}", e),
        }
    }

    pub fn stop(&mut self) {
        self.work_sender.send(Event::stop_event()).unwrap();
        drop(&mut self.runtime);
    }

    pub fn send(&mut self, event: Event) {
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

    pub fn responses(&self) -> Vec<Response> {
        self.responses.lock().unwrap().to_vec()
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
        assert_eq!(
            transmission.options.max_batch_timeout,
            DEFAULT_MAX_BATCH_TIMEOUT
        );
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
    fn test_batch() {
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
        .with_body("finished batch to honeycomb")
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

        std::thread::sleep(Duration::from_millis(1000));

        let only = &transmission.responses()[0];
        assert_eq!(only.status_code, StatusCode::OK);
        assert_eq!(only.body, "finished batch to honeycomb".to_string());
        transmission.stop();
    }
}
