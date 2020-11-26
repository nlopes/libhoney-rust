use std::collections::HashMap;

use chrono::prelude::{DateTime, Utc};
use log::info;
use rand::Rng;

use crate::client;
use crate::errors::{Error, Result};
use crate::fields::FieldHolder;
use crate::sender::Sender;
use crate::Value;

/// `Metadata` is a type alias for an optional json serialisable value
pub type Metadata = Option<Value>;

/// `Event` is used to hold data that can be sent to Honeycomb. It can also specify
/// overrides of the config settings (`client::Options`).
#[derive(Debug, Clone)]
pub struct Event {
    pub(crate) options: client::Options,
    pub(crate) timestamp: DateTime<Utc>,
    pub(crate) fields: HashMap<String, Value>,
    pub(crate) metadata: Metadata,
    sent: bool,
}

impl FieldHolder for Event {
    fn add(&mut self, data: HashMap<String, Value>) {
        if !self.sent {
            self.fields.extend(data);
        }
    }

    /// add_field adds a field to the current (event/builder) fields
    fn add_field(&mut self, name: &str, value: Value) {
        if !self.sent {
            self.fields.insert(name.to_string(), value);
        }
    }

    /// add_func iterates over the results from func (until Err) and adds the results to
    /// the event/builder fields
    fn add_func<F>(&mut self, func: F)
    where
        F: Fn() -> Result<(String, Value)>,
    {
        if !self.sent {
            while let Ok((name, value)) = func() {
                self.add_field(&name, value);
            }
        }
    }
}

impl Event {
    /// new creates a new event with the passed ClientOptions
    pub fn new(options: &client::Options) -> Self {
        Self {
            options: options.clone(),
            timestamp: Utc::now(),
            fields: HashMap::new(),
            metadata: None,
            sent: false,
        }
    }

    /// send dispatches the event to be sent to Honeycomb, sampling if necessary.
    ///
    /// If you have sampling enabled (i.e. sample_rate >1), send will only actually
    /// transmit data with a probability of 1/sample_rate. No error is returned whether or
    /// not traffic is sampled, however, the Response sent down the response channel will
    /// indicate the event was sampled in the errors Err field.
    ///
    /// Send inherits the values of required fields from ClientOptions. If any required
    /// fields are specified in neither ClientOptions nor the Event, send will return an
    /// error. Required fields are api_host, api_key, and dataset. Values specified in an
    /// Event override ClientOptions.
    ///
    /// Once you send an event, any addition calls to add data to that event will return
    /// without doing anything. Once the event is sent, it becomes immutable.
    pub fn send<T: Sender>(&mut self, client: &mut client::Client<T>) -> Result<()> {
        if self.should_drop() {
            info!("dropping event due to sampling");
            return Ok(());
        }
        self.send_presampled(client)
    }

    /// `send_presampled` dispatches the event to be sent to Honeycomb.
    ///
    /// Sampling is assumed to have already happened. `send_presampled` will dispatch
    /// every event handed to it, and pass along the sample rate. Use this instead of
    /// `send()` when the calling function handles the logic around which events to drop
    /// when sampling.
    ///
    /// `send_presampled` inherits the values of required fields from `Config`. If any
    /// required fields are specified in neither `Config` nor the `Event`, `send` will
    /// return an error.  Required fields are `api_host`, `api_key`, and `dataset`. Values
    /// specified in an `Event` override `Config`.
    ///
    /// Once you `send` an event, any addition calls to add data to that event will return
    /// without doing anything. Once the event is sent, it becomes immutable.
    pub fn send_presampled<T: Sender>(&mut self, client: &mut client::Client<T>) -> Result<()> {
        if self.fields.is_empty() {
            return Err(Error::missing_event_fields());
        }

        if self.options.api_host.is_empty() {
            return Err(Error::missing_option("api_host", "can't send to Honeycomb"));
        }

        if self.options.api_key.is_empty() {
            return Err(Error::missing_option("api_key", "can't send to Honeycomb"));
        }

        if self.options.dataset.is_empty() {
            return Err(Error::missing_option("dataset", "can't send to Honeycomb"));
        }

        self.sent = true;
        client.transmission.send(self.clone());
        Ok(())
    }

    /// Set options sample_rate on the event
    pub fn set_sample_rate(&mut self, sample_rate: usize) {
        self.options.sample_rate = sample_rate;
    }

    /// Set timestamp on the event
    pub fn set_timestamp(&mut self, timestamp: DateTime<Utc>) {
        self.timestamp = timestamp;
    }

    /// Set metadata on the event
    pub fn set_metadata(&mut self, metadata: Metadata) {
        self.metadata = metadata;
    }

    /// Get event metadata
    pub fn metadata(&self) -> Metadata {
        self.metadata.clone()
    }

    /// Get event fields
    pub fn fields(&self) -> HashMap<String, Value> {
        self.fields.clone()
    }

    /// Get event fields (mutable)
    pub fn get_fields_mut(&mut self) -> &mut HashMap<String, Value> {
        &mut self.fields
    }

    fn should_drop(&self) -> bool {
        if self.options.sample_rate <= 1 {
            return false;
        }
        rand::thread_rng().gen_range(0, self.options.sample_rate) != 0
    }

    pub(crate) fn stop_event() -> Self {
        let mut h: HashMap<String, Value> = HashMap::new();
        h.insert("internal_stop_event".to_string(), Value::Null);

        Self {
            options: client::Options::default(),
            timestamp: Utc::now(),
            fields: h,
            metadata: None,
            sent: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use surf::StatusCode;

    use super::*;
    use crate::client;

    #[test]
    fn test_add() {
        let mut e = Event::new(&client::Options {
            api_key: "some_api_key".to_string(),
            ..client::Options::default()
        });
        let now = Value::String(Utc::now().to_rfc3339());
        e.add_field("my_timestamp", now.clone());

        assert_eq!(e.options.api_key, "some_api_key");
        assert_eq!(e.fields["my_timestamp"], now);
    }

    #[test]
    fn test_send() {
        use crate::transmission;

        let api_host = &mockito::server_url();
        let _m = mockito::mock(
            "POST",
            mockito::Matcher::Regex(r"/1/batch/(.*)$".to_string()),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("[{ \"status\": 200 }]")
        .create();

        let options = client::Options {
            api_key: "some api key".to_string(),
            api_host: api_host.to_string(),
            ..client::Options::default()
        };

        let mut client = client::Client::new(
            options.clone(),
            transmission::Transmission::new(transmission::Options {
                max_batch_size: 1,
                ..transmission::Options::default()
            })
            .unwrap(),
        );

        let mut e = Event::new(&options);
        e.add_field("field_name", Value::String("field_value".to_string()));
        e.send(&mut client).unwrap();

        if let Some(only) = client.transmission.responses().iter().next() {
            assert_eq!(only.status_code, Some(StatusCode::Ok));
        }
        client.close().unwrap();
    }

    #[test]
    fn test_empty() {
        use crate::errors::ErrorKind;
        use crate::transmission;

        let api_host = &mockito::server_url();
        let _m = mockito::mock(
            "POST",
            mockito::Matcher::Regex(r"/1/batch/(.*)$".to_string()),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("[{ \"status\": 200 }]")
        .create();

        let mut client = client::Client::new(
            client::Options {
                api_key: "some api key".to_string(),
                api_host: api_host.to_string(),
                ..client::Options::default()
            },
            transmission::Transmission::new(transmission::Options {
                max_batch_size: 1,
                ..transmission::Options::default()
            })
            .unwrap(),
        );

        let mut e = client.new_event();
        assert_eq!(
            e.send(&mut client).err().unwrap().kind,
            ErrorKind::MissingEventFields
        );
        client.close().unwrap();
    }
}
