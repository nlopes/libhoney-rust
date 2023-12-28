/*! Client is the interface to create new builders, events and send the latter somewhere using `Transmission`.

*/
use std::collections::HashMap;

use log::info;
use serde_json::Value;
use tokio::sync::mpsc::Receiver;

use crate::errors::Result;
use crate::fields::FieldHolder;
use crate::response::Response;
use crate::sender::Sender;
use crate::Event;
use crate::{Builder, DynamicFieldFunc};

const DEFAULT_API_HOST: &str = "https://api.honeycomb.io";
const DEFAULT_API_KEY: &str = "";
const DEFAULT_DATASET: &str = "librust-dataset";
const DEFAULT_SAMPLE_RATE: usize = 1;

/// Options is a subset of the global libhoney config that focuses on the
/// configuration of the client itself.
#[derive(Debug, Clone)]
pub struct Options {
    /// api_key is the Honeycomb authentication token. If it is specified during libhoney
    /// initialization, it will be used as the default API key for all events. If absent,
    /// API key must be explicitly set on a builder or event. Find your team's API keys at
    /// https://ui.honeycomb.io/account
    pub api_key: String,

    /// api_host is the hostname for the Honeycomb API server to which to send this
    /// event. default: https://api.honeycomb.io/
    pub api_host: String,

    /// dataset is the name of the Honeycomb dataset to which to send these events.  If it
    /// is specified during libhoney initialization, it will be used as the default
    /// dataset for all events. If absent, dataset must be explicitly set on a builder or
    /// event.
    pub dataset: String,

    /// sample_rate is the rate at which to sample this event. Default is 1, meaning no
    /// sampling. If you want to send one event out of every 250 times Send() is called,
    /// you would specify 250 here.
    pub sample_rate: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            api_key: DEFAULT_API_KEY.to_string(),
            dataset: DEFAULT_DATASET.to_string(),
            api_host: DEFAULT_API_HOST.to_string(),
            sample_rate: DEFAULT_SAMPLE_RATE,
        }
    }
}

/// Client represents an object that can create new builders and events and send them
/// somewhere.
#[derive(Debug, Clone)]
pub struct Client<T: Sender> {
    pub(crate) options: Options,
    /// transmission mechanism for the client
    pub transmission: T,

    builder: Builder,
}

impl<T> Client<T>
where
    T: Sender,
{
    /// new creates a new Client with the provided Options and initialised
    /// Transmission.
    ///
    /// Once populated, it auto starts the transmission background threads and is ready to
    /// send events.
    pub fn new(options: Options, transmission: T) -> Self {
        info!("Creating honey client");

        let mut c = Self {
            transmission,
            options: options.clone(),
            builder: Builder::new(options),
        };
        c.start();
        c
    }

    fn start(&mut self) {
        self.transmission.start();
    }

    /// add adds its data to the Client's scope. It adds all fields in a struct or all
    /// keys in a map as individual Fields. These metrics will be inherited by all
    /// builders and events.
    pub fn add(&mut self, data: HashMap<String, Value>) {
        self.builder.add(data);
    }

    /// add_field adds a Field to the Client's scope. This metric will be inherited by all
    /// builders and events.
    pub fn add_field(&mut self, name: &str, value: Value) {
        self.builder.add_field(name, value);
    }

    /// add_dynamic_field takes a field name and a function that will generate values for
    /// that metric. The function is called once every time a new_event() is created and
    /// added as a field (with name as the key) to the newly created event.
    pub fn add_dynamic_field(&mut self, name: &str, func: DynamicFieldFunc) {
        self.builder.add_dynamic_field(name, func);
    }

    /// close waits for all in-flight messages to be sent. You should call close() before
    /// app termination.
    pub fn close(mut self) -> Result<()> {
        info!("closing libhoney client");
        self.transmission.stop()
    }

    /// flush closes and reopens the Transmission, ensuring events are sent without
    /// waiting on the batch to be sent asyncronously. Generally, it is more efficient to
    /// rely on asyncronous batches than to call Flush, but certain scenarios may require
    /// Flush if asynchronous sends are not guaranteed to run (i.e. running in AWS Lambda)
    /// Flush is not thread safe - use it only when you are sure that no other parts of
    /// your program are calling Send
    pub fn flush(&mut self) -> Result<()> {
        info!("flushing libhoney client");
        self.transmission.stop()?;
        self.transmission.start();
        Ok(())
    }

    /// new_builder creates a new event builder. The builder inherits any Dynamic or
    /// Static Fields present in the Client's scope.
    pub fn new_builder(&self) -> Builder {
        self.builder.clone()
    }

    /// new_event creates a new event prepopulated with any Fields present in the Client's
    /// scope.
    pub fn new_event(&self) -> Event {
        self.builder.new_event()
    }

    /// responses returns a receiver channel with responses
    pub fn responses(&self) -> Receiver<Response> {
        self.transmission.responses()
    }
}

#[cfg(test)]
mod tests {
    use super::{Client, FieldHolder, Options, Value};
    use crate::transmission::{self, Transmission};

    #[test]
    fn test_init() {
        let client = Client::new(
            Options::default(),
            Transmission::new(transmission::Options::default()).unwrap(),
        );
        client.close().unwrap();
    }

    #[tokio::test]
    async fn test_flush() {
        use reqwest::StatusCode;
        use serde_json::json;

        let api_host = &mockito::server_url();
        let _m = mockito::mock(
            "POST",
            mockito::Matcher::Regex(r"/1/batch/(.*)$".to_string()),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("[{ \"status\": 202 }]")
        .create();

        let mut client = Client::new(
            Options {
                api_key: "some api key".to_string(),
                api_host: api_host.to_string(),
                ..Options::default()
            },
            Transmission::new(transmission::Options::default()).unwrap(),
        );

        let mut event = client.new_event();
        event.add_field("some_field", Value::String("some_value".to_string()));
        event.metadata = Some(json!("some metadata in a string"));
        event.send(&mut client).unwrap();

        let response = client.responses().recv().await.unwrap();
        assert_eq!(response.status_code, Some(StatusCode::ACCEPTED));
        assert_eq!(response.metadata, Some(json!("some metadata in a string")));

        client.flush().unwrap();

        event = client.new_event();
        event.add_field("some_field", Value::String("some_value".to_string()));
        event.metadata = Some(json!("some metadata in a string"));
        event.send(&mut client).unwrap();

        let response = client.responses().recv().await.iter().next().unwrap();
        assert_eq!(response.status_code, Some(StatusCode::ACCEPTED));
        assert_eq!(response.metadata, Some(json!("some metadata in a string")));

        client.close().unwrap();
    }

    #[test]
    fn test_send_without_api_key() {
        use serde_json::json;

        use crate::errors::ErrorKind;

        let api_host = &mockito::server_url();
        let _m = mockito::mock(
            "POST",
            mockito::Matcher::Regex(r"/1/batch/(.*)$".to_string()),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("[{ \"status\": 202 }]")
        .create();

        let mut client = Client::new(
            Options {
                api_host: api_host.to_string(),
                ..Options::default()
            },
            Transmission::new(transmission::Options::default()).unwrap(),
        );

        let mut event = client.new_event();
        event.add_field("some_field", Value::String("some_value".to_string()));
        event.metadata = Some(json!("some metadata in a string"));
        let err = event.send(&mut client).err().unwrap();

        assert_eq!(err.kind, ErrorKind::MissingOption);
        assert_eq!(
            err.message,
            "missing option 'api_key', can't send to Honeycomb"
        );
        client.close().unwrap();
    }
}
