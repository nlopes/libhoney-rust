use std::collections::HashMap;

use log::info;
use serde_json::Value;

use crate::fields::FieldHolder;
use crate::response::Response;
use crate::Event;
use crate::Transmission;
use crate::{Builder, DynamicFieldFunc};

const DEFAULT_API_HOST: &str = "https://api.honeycomb.io";
const DEFAULT_API_KEY: &str = "";
const DEFAULT_DATASET: &str = "librust-dataset";
const DEFAULT_SAMPLE_RATE: usize = 1;

/// ClientOptions is a subset of the global libhoney config that focuses on the
/// configuration of the client itself.
#[derive(Debug, Clone)]
pub struct ClientOptions {
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

impl Default for ClientOptions {
    fn default() -> Self {
        ClientOptions {
            api_key: DEFAULT_API_KEY.to_string(),
            dataset: DEFAULT_DATASET.to_string(),
            api_host: DEFAULT_API_HOST.to_string(),
            sample_rate: DEFAULT_SAMPLE_RATE,
        }
    }
}

/// Client represents an object that can create new builders and events and send them
/// somewhere.
#[derive(Debug)]
pub struct Client {
    pub(crate) options: ClientOptions,
    pub(crate) transmission: Transmission,

    builder: Builder,
}

impl Client {
    /// new creates a new Client with the provided ClientOptions and initialised
    /// Transmission.
    ///
    /// Once populated, it auto starts the transmission background threads and is ready to
    /// send events.
    pub fn new(options: ClientOptions, transmission: Transmission) -> Self {
        info!("Creating honey client");

        let mut c = Client {
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
    pub fn close(mut self) {
        self.transmission.stop();
    }

    /// flush closes and reopens the Transmission, ensuring events are sent without
    /// waiting on the batch to be sent asyncronously. Generally, it is more efficient to
    /// rely on asyncronous batches than to call Flush, but certain scenarios may require
    /// Flush if asynchronous sends are not guaranteed to run (i.e. running in AWS Lambda)
    /// Flush is not thread safe - use it only when you are sure that no other parts of
    /// your program are calling Send
    pub fn flush(self) {
        unimplemented!()
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

    /// responses returns a Vec from which the caller can read the responses to sent
    /// events.
    ///
    /// TODO(nlopes): This should be a future probably - really not happy with this
    pub fn responses(&self) -> Vec<Response> {
        self.transmission.responses()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client() {
        let client = Client::new(Default::default(), Transmission::new(Default::default()));
        client.close();
    }
}
