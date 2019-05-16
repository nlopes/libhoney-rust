use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use crossbeam_channel::{bounded, Receiver, Sender};
use futures::future::{lazy, Future, IntoFuture};
use reqwest::StatusCode;
use tokio::runtime::{Builder, Runtime};
use tokio_timer::clock::Clock;

use crate::response::Response;

const DEFAULT_NAME_PREFIX: &str = "libhoney-rust";
// DEFAULT_MAX_BATCH_SIZE how many events to collect in a batch
const DEFAULT_MAX_BATCH_SIZE: usize = 50;
// DEFAULT_MAX_CONCURRENT_BATCHES how many batches to maintain in parallel
const DEFAULT_MAX_CONCURRENT_BATCHES: usize = 80;
// DEFAULT_MAX_BATCH_TIMEOUT how frequently to send unfilled batches
const DEFAULT_MAX_BATCH_TIMEOUT: Duration = Duration::from_millis(100);
// DEFAULT_PENDING_WORK_CAPACITY how many events to queue up for busy batches
const DEFAULT_PENDING_WORK_CAPACITY: usize = 10_000;

#[derive(Debug, Clone, Copy)]
pub struct TransmissionOptions {
    max_batch_size: usize,
    max_concurrent_batches: usize,
    max_batch_timeout: Duration,
    pending_work_capacity: usize,
}

impl Default for TransmissionOptions {
    fn default() -> Self {
        TransmissionOptions {
            max_batch_size: DEFAULT_MAX_BATCH_SIZE,
            max_concurrent_batches: DEFAULT_MAX_CONCURRENT_BATCHES,
            max_batch_timeout: DEFAULT_MAX_BATCH_TIMEOUT,
            pending_work_capacity: DEFAULT_PENDING_WORK_CAPACITY,
        }
    }
}

pub struct Transmission {
    pub(crate) options: TransmissionOptions,
    user_agent: String,

    runtime: Runtime,

    work_sender: Sender<String>,
    work_receiver: Receiver<String>,
    response_sender: Sender<Response>,
    response_receiver: Receiver<Response>,

    responses: Arc<Mutex<Vec<Response>>>,
}

impl Transmission {
    fn new_runtime(options: Option<TransmissionOptions>) -> Runtime {
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
        let options = self.options;
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
        work_receiver: Receiver<String>,
        response_sender: Sender<Response>,
        options: TransmissionOptions,
        user_agent: String,
    ) -> impl Future<Item = (), Error = ()> {
        let mut runtime = Transmission::new_runtime(Some(options));
        let mut batch: Vec<String> = Vec::with_capacity(options.max_batch_size);

        loop {
            match work_receiver.recv() {
                Ok(s) => {
                    if s == "STOP_WORK" {
                        break;
                    }
                    batch.push(s);
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
        bcopy: Vec<String>,
        options: TransmissionOptions,
        user_agent: String,
        clock: SystemTime,
    ) -> Response {
        // TODO(nlopes): send batch to honeycomb here
        eprintln!("Send batch {:?} to honeycomb!", bcopy);
        Response {
            status_code: StatusCode::OK,
            body: "finished batch to honeycomb".to_string(),
            duration: clock.elapsed().unwrap(),
        }
    }

    pub fn stop(self) {
        self.work_sender.send("STOP_WORK".to_string()).unwrap();
        self.runtime.shutdown_now().wait().unwrap();
    }

    pub fn add(&mut self, s: String) {
        if !self.work_sender.is_full() {
            self.runtime.spawn(
                self.work_sender
                    .clone()
                    .send_timeout(s, Duration::from_millis(500))
                    .map_err(|e| {
                        eprintln!("Error when adding: {:#?}", e);
                    })
                    .into_future(),
            );
        }
    }

    fn responses(&self) -> Vec<Response> {
        self.responses.lock().unwrap().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    #[test]
    fn test_batch() {
        let mut transmission = Transmission::new(TransmissionOptions {
            max_batch_size: 5,
            ..Default::default()
        });
        transmission.start();

        let ua = transmission.user_agent.clone();
        assert_eq!(
            ua,
            format!("{}/{}", DEFAULT_NAME_PREFIX, env!("CARGO_PKG_VERSION"))
        );

        for i in 0..5 {
            transmission.add(i.to_string());
        }

        std::thread::sleep(Duration::from_millis(1000));

        let expected = Response {
            status_code: StatusCode::OK,
            body: "finished batch to honeycomb".to_string(),
            duration: Duration::from_millis(0), //We don't care in testing
        };
        let only = &transmission.responses()[0];
        assert_eq!(only.status_code, expected.status_code,);
        assert_eq!(only.body, expected.body);

        transmission.stop();
    }
}
