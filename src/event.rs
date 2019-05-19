use std::collections::HashMap;

use chrono::prelude::{DateTime, Utc};
use rand::Rng;
use serde_json::Value;

use crate::client::Client;
use crate::fields::FieldHolder;
use crate::ClientOptions;

/// Event is used to hold data that can be sent to Honeycomb. It can also specify
/// overrides of the config settings (ClientOptions).
#[derive(Debug, Clone)]
pub struct Event {
    pub(crate) options: ClientOptions,
    pub(crate) timestamp: DateTime<Utc>,
    pub(crate) fields: HashMap<String, Value>,
    metadata: Value,
}

impl FieldHolder for Event {
    fn get_fields(&mut self) -> &mut HashMap<String, Value> {
        &mut self.fields
    }
}

impl Event {
    /// new creates a new event with the passed ClientOptions
    pub fn new(options: &ClientOptions) -> Self {
        Event {
            options: options.clone(),
            timestamp: Utc::now(),
            fields: HashMap::new(),
            metadata: Value::Null,
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
    pub fn send(&self, client: &mut Client) {
        if self.should_drop() {
            return;
        }
        client.transmission.send(self.clone());
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

        Event {
            options: ClientOptions::default(),
            timestamp: Utc::now(),
            fields: h,
            metadata: Value::Null,
        }
    }
}

#[cfg(test)]
mod tests {
    use mockito;
    use reqwest::StatusCode;

    use super::*;
    use crate::ClientOptions;

    #[test]
    fn test_add() {
        let mut e = Event::new(&ClientOptions {
            api_key: "some_api_key".to_string(),
            ..Default::default()
        });
        let now = Value::String(Utc::now().to_rfc3339());
        e.add_field("my_timestamp", now.clone());

        assert_eq!(e.options.api_key, "some_api_key");
        assert_eq!(e.fields["my_timestamp"], now);
    }

    #[test]
    fn test_send() {
        use crate::{Transmission, TransmissionOptions};

        let api_host = &mockito::server_url();
        let _m = mockito::mock(
            "POST",
            mockito::Matcher::Regex(r"/1/batch/(.*)$".to_string()),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("finished batch to honeycomb")
        .create();

        let options = ClientOptions {
            api_host: api_host.to_string(),
            ..Default::default()
        };

        let mut client = Client::new(
            options.clone(),
            Transmission::new(TransmissionOptions {
                max_batch_size: 1,
                ..Default::default()
            }),
        );

        let mut e = Event::new(&options);
        e.add_field("field_name", Value::String("field_value".to_string()));
        e.send(&mut client);

        std::thread::sleep(std::time::Duration::from_millis(1000));
        let only = &client.transmission.responses()[0];
        assert_eq!(only.status_code, StatusCode::OK);
        assert_eq!(only.body, "finished batch to honeycomb".to_string());
    }
}
