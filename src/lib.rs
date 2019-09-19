/*! Rust library for sending data to Honeycomb.

# **IMPORTANT NOTE**: please do **NOT** use this in **production** (yet).

But I'd be forever greatful if you can try it out and provide feedback. There are a few
reasons why I think this is not yet for prime time (aka: production):

- Honeycomb uses the singleton pattern for the libraries but I decided not to use it here (mostly due to: harder to get right, it feels to me like a rust anti-pattern). If you think I should have, please let me know.

- I'm not convinced of the threading code. Although "it works" it probably isn't great - any feedback would be greatly appreciated.

- I've set Metadata as a "serialisable json value" (`serde_json::Value`) - this isn't quite right but allows for a nicer use of the library. Check the [nlopes/metadata-user-set branch](https://github.com/nlopes/libhoney-rust/tree/nlopes/metadata-user-set) for a user defined Metadata instead.

For these reasons, you're probably better waiting for a 1.0.0 relase (I'll follow
[semantic versioning][semantic versioning]). Having said that, if you still want to use
this, thank you for being brave, and make sure to open bugs against it!

# libhoney

Rust library for sending events to Honeycomb, a service for debugging your software in
production.

- [Usage and Examples](#usage-and-examples)
- [API Reference][API reference]

# Usage and Examples

## Initialization

Initialize the library by passing in your Team API key and the default dataset name to
which it should send events. When you call the library’s initialization routine, it spins
up background threads to handle sending all the events. Calling .close() on the client
will terminate all background threads.

```rust
let client = libhoney::init(libhoney::Config{
  options: libhoney::client::Options {
    api_key: "YOUR_API_KEY".to_string(),
    dataset: "honeycomb-rust-example".to_string(),
    ..libhoney::client::Options::default()
  },
  transmission_options: libhoney::transmission::Options::default(),
});

client.close();
```

Further configuration options can be found in the [API reference][API reference].

## Building and Sending Events

Once initialized, the libhoney client is ready to send events. Events go through three
phases:

- Creation `event := builder.new_event()`
- Adding fields `event.add_field("key", Value::String("val".to_string()))`, `event.add(data)`
- Transmission `event.send(&mut client)`

Upon calling .send(), the event is dispatched to be sent to Honeycomb. All libraries set
defaults that will allow your application to function as smoothly as possible during error
conditions.

In its simplest form, you can add a single attribute to an event with the `.add_field(k,
v)` method. If you add the same key multiple times, only the last value added will be
kept.

More complex structures (maps and structs—things that can be serialized into a JSON
object) can be added to an event with the .add(data) method.

Events can have metadata associated with them that is not sent to Honeycomb. This metadata
is used to identify the event when processing the response. More detail about metadata is
below in the Response section.

## Handling responses

Sending an event is an asynchronous action and will avoid blocking by default. .send()
will enqueue the event to be sent as soon as possible (thus, the return value doesn’t
indicate that the event was successfully sent). Use the Vec returned by .responses() to
check whether events were successfully received by Honeycomb’s servers.

Before sending an event, you have the option to attach metadata to that event. This
metadata is not sent to Honeycomb; instead, it’s used to help you match up individual
responses with sent events. When sending an event, libhoney will take the metadata from
the event and attach it to the response object for you to consume. Add metadata by
populating the .metadata attribute directly on an event.

Responses have a number of fields describing the result of an attempted event send:

- `metadata`: the metadata you attached to the event to which this response corresponds

- `status_code`: the HTTP status code returned by Honeycomb when trying to send the event. 2xx indicates success.

- `duration`: the time.Duration it took to send the event.

- `body`: the body of the HTTP response from Honeycomb. On failures, this body contains some more information about the failure.

- `error`: when the event doesn’t even get to create a HTTP attempt, the reason will be in this field. (e.g. when sampled or dropped because of a queue overflow).

You don’t have to process responses if you’re not interested in them—simply ignoring them
is perfectly safe. Unread responses will be dropped.

## Examples

Honeycomb can calculate all sorts of statistics, so send the data you care about and let
us crunch the averages, percentiles, lower/upper bounds, cardinality—whatever you want—for
you.

### Simple: send an event
```rust
# use std::collections::HashMap;
# use serde_json::{json, Value};
# use libhoney::{init, Config};
# let api_host = &mockito::server_url();
# let _m = mockito::mock(
#    "POST",
#     mockito::Matcher::Regex(r"/1/batch/(.*)$".to_string()),
# )
# .with_status(200)
# .with_header("content-type", "application/json")
# .with_body("finished batch to honeycomb")
# .create();

# let options = libhoney::client::Options{api_host: api_host.to_string(), api_key: "some key".to_string(), ..libhoney::client::Options::default()};
use libhoney::FieldHolder; // Add trait to allow for adding fields
// Call init to get a client
let mut client = init(libhoney::Config {
  options: options,
  transmission_options: libhoney::transmission::Options::default(),
});

let mut data: HashMap<String, Value> = HashMap::new();
data.insert("duration_ms".to_string(), json!(153.12));
data.insert("method".to_string(), Value::String("get".to_string()));
data.insert("hostname".to_string(), Value::String("appserver15".to_string()));
data.insert("payload_length".to_string(), json!(27));

let mut ev = client.new_event();
ev.add(data);
 // In production code, please check return of `.send()`
ev.send(&mut client).err();
```

[API reference]: https://docs.rs/libhoney-rust
[semantic versioning]: https://semver.org

 */
#![deny(missing_docs)]

mod builder;
pub mod client;
mod errors;
mod event;
mod eventdata;
mod events;
mod fields;
pub mod mock;
mod response;
mod sender;
pub mod transmission;

pub use builder::{Builder, DynamicFieldFunc};
pub use client::Client;
pub use errors::{Error, ErrorKind, Result};
pub use event::{Event, Metadata};
pub use fields::FieldHolder;
pub use sender::Sender;
pub use serde_json::{json, Value};
use transmission::Transmission;

/// Config allows the user to customise the initialisation of the library (effectively the
/// Client)
#[derive(Debug, Clone)]
#[must_use = "must be set up for client to be properly initialised"]
pub struct Config {
    /// options is a subset of the global libhoney config that focuses on the
    /// configuration of the client itself. The other config options are specific to a
    /// given transmission Sender and should be specified there if the defaults need to be
    /// overridden.
    pub options: client::Options,

    /// Configuration for the underlying sender. It is safe (and recommended) to leave
    /// these values at their defaults. You cannot change these values after calling
    /// init()
    pub transmission_options: transmission::Options,
}

/// init is called on app initialisation and passed a `Config`. A `Config` has two sets of
/// options (`client::Options` and `transmission::Options`).
#[inline]
pub fn init(config: Config) -> Client<Transmission> {
    let transmission =
        Transmission::new(config.transmission_options).expect("failed to instantiate transmission");
    Client::new(config.options, transmission)
}

/// Auxiliary test module
pub mod test {
    use crate::mock;
    /// `init` is purely used for testing purposes
    pub fn init(config: super::Config) -> super::Client<mock::TransmissionMock> {
        let transmission = mock::TransmissionMock::new(config.transmission_options)
            .expect("failed to instantiate transmission");
        super::Client::new(config.options, transmission)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init() {
        let client = init(Config {
            options: client::Options::default(),
            transmission_options: transmission::Options::default(),
        });
        assert_eq!(client.options.dataset, "librust-dataset");
        client.close().unwrap();
    }
}
