/*! Rust library for sending data to Honeycomb.

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
  options: libhoney::ClientOptions {
    api_key: "YOUR_API_KEY".to_string(),
    dataset: "honeycomb-rust-example".to_string(),
    ..Default::default()
  },
  transmission_options: Default::default(),
});

client.close();
```

Further configuration options can be found in the [API reference][API reference].

##

# References
[APpI reference]: https://docs.rs/libhoney-rust

 */
#![deny(missing_docs)]

mod builder;
mod client;
mod event;
mod fields;
mod response;
mod transmission;

pub use builder::{Builder, DynamicFieldFunc};
pub use client::{Client, ClientOptions};
pub use event::Event;
pub use transmission::{Transmission, TransmissionOptions};

/// Config allows the user to customise the initialisation of the library (effectively the
/// Client)
#[derive(Debug)]
#[must_use = "must be set up for client to be properly initialised"]
pub struct Config {
    /// options is a subset of the global libhoney config that focuses on the
    /// configuration of the client itself. The other config options are specific to a
    /// given transmission Sender and should be specified there if the defaults need to be
    /// overridden.
    pub options: ClientOptions,

    /// Configuration for the underlying sender. It is safe (and recommended) to leave
    /// these values at their defaults. You cannot change these values after calling
    /// init()
    pub transmission_options: TransmissionOptions,
}

/// init is called on app initialisation and passed a Config. A Config has two sets of
/// options (ClientOptions and TrasnmissionOptions).
pub fn init(config: Config) -> Client {
    let transmission = Transmission::new(config.transmission_options);
    Client::new(config.options, transmission)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init() {
        let client = init(Config {
            options: Default::default(),
            transmission_options: Default::default(),
        });
        assert_eq!(client.options.dataset, "librust-dataset");
    }
}
