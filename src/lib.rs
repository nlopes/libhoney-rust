/*! Rust library for sending data to Honeycomb.

Rust library for sending events to Honeycomb, a service for debugging your software in
production.

- [Usage and Examples](#usage-and-examples)
- [API Reference][API reference]

# Usage and Examples

## Initialization

Initialize the library by passing in your Team API key and the default dataset name to
which it should send events. When you call the library’s initialization routine, it spins
up background threads to handle sending all the events. Once the client goes out of scope,
all background threads will be terminated.

```rust
let client = libhoney::init(libhoney::Config{
  api_key: "YOUR_API_KEY",
  dataset: "honeycomb-rust-example",
});
```

Further configuration options can be found in the [API reference][API reference].

##

# References
[API reference]: https://docs.rs/libhoney-rust

 */
#![deny(missing_docs)]

mod builder;
mod client;
mod event;
mod fields;
mod response;
mod transmission;

use builder::{Builder, DynamicFieldFunc};
use client::{Client, ClientOptions};
use event::Event;
use transmission::{Transmission, TransmissionOptions};

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
    let client = Client::new(config.options, transmission);
    client
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init() {
        let client = init(Config{
            options: Default::default(),
            transmission_options: Default::default(),
        });
        assert_eq!(client.options.dataset, "librust-dataset");
    }
}
