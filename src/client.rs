use std::collections::HashMap;

use log::info;
use serde_json::Value;

use crate::fields::FieldHolder;
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
    // api_key is the Honeycomb authentication token. If it is specified during
    // libhoney initialization, it will be used as the default API key for all
    // events. If absent, API key must be explicitly set on a builder or
    // event. Find your team's API keys at https://ui.honeycomb.io/account
    pub(crate) api_key: String,

    // api_host is the hostname for the Honeycomb API server to which to send this
    // event. default: https://api.honeycomb.io/
    pub(crate) api_host: String,

    // dataset is the name of the Honeycomb dataset to which to send these events.
    // If it is specified during libhoney initialization, it will be used as the
    // default dataset for all events. If absent, dataset must be explicitly set
    // on a builder or event.
    pub(crate) dataset: String,

    // sample_rate is the rate at which to sample this event. Default is 1, meaning no
    // sampling. If you want to send one event out of every 250 times Send() is called,
    // you would specify 250 here.
    pub(crate) sample_rate: usize,
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

#[derive(Debug)]
pub struct Client {
    pub(crate) options: ClientOptions,
    pub(crate) transmission: Transmission,

    builder: Builder,
}

impl Drop for Client {
    fn drop(&mut self) {
        drop(&self.transmission);
    }
}

impl Client {
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

    pub fn add(&mut self, data: HashMap<String, Value>) {
        self.builder.add(data);
    }

    pub fn add_field(&mut self, name: &str, value: Value) {
        self.builder.add_field(name, value);
    }

    pub fn add_dynamic_field(&mut self, name: &str, func: DynamicFieldFunc) {
        self.builder.add_dynamic_field(name, func);
    }

    pub fn add_func(&mut self) {
        unimplemented!()
    }

    pub fn close(mut self) {
        self.transmission.stop();
    }

    pub fn flush(self) {
        unimplemented!()
    }

    pub fn new_builder(&self) -> Builder {
        self.builder.clone()
    }

    pub fn new_event(&self) -> Event {
        self.builder.new_event()
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
