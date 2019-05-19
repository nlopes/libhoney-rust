
[![docs.rs](https://docs.rs/libhoney-rust/badge.svg)](https://docs.rs/libhoney-rust)
[![crates.io](https://img.shields.io/crates/v/libhoney-rust.svg)](https://crates.io/crates/libhoney-rust)
[![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/nlopes/libhoney-rust/blob/master/LICENSE)
[![Build Status](https://travis-ci.org/nlopes/libhoney-rust.svg?branch=master)](https://travis-ci.org/nlopes/libhoney-rust)

# libhoney

Rust library for sending data to Honeycomb.

Rust library for sending events to Honeycomb, a service for debugging your software in
production.

- [Usage and Examples](#usage-and-examples)
- [API Reference][API reference]

## Usage and Examples

### Initialization

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

###

[API reference]: https://docs.rs/libhoney-rust

