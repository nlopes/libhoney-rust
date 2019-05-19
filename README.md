[![Build Status](https://travis-ci.org/nlopes/libhoney-rust.svg?branch=master)](https://travis-ci.org/nlopes/libhoney-rust)

# libhoney-rust

Rust library for sending data to Honeycomb.

Rust library for sending events to Honeycomb, a service for debugging your software in
production.

- [Usage and Examples](#usage-and-examples)
- [API Reference][API reference]

## Usage and Examples

### Initialization

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

###

## References
[API reference]: https://docs.rs/libhoney-rust


License: MIT
