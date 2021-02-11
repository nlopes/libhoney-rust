use std::sync::Arc;

use async_executors::TokioTpBuilder;
use libhoney::{Error, FieldHolder, FutureExecutor};

fn main() -> Result<(), Error> {
    env_logger::init();

    let mut builder = TokioTpBuilder::new();
    builder.tokio_builder().enable_io().enable_time();
    let executor = Arc::new(builder.build().expect("failed to build Tokio executor"));
    executor.block_on(async_main(executor.clone()))
}

async fn async_main(executor: FutureExecutor) -> Result<(), Error> {
    let client = libhoney::init(libhoney::Config {
        executor,
        options: libhoney::client::Options {
            api_key: std::env::var("HONEYCOMB_API_KEY").expect("need to set HONEYCOMB_API_KEY"),
            dataset: std::env::var("HONEYCOMB_DATASET").expect("need to set HONEYCOMB_DATASET"),
            ..Default::default()
        },
        transmission_options: libhoney::transmission::Options::default(),
    })
    .expect("failed to spawn Honeycomb client");
    let mut event = client.new_event();
    event.add_field("extra", libhoney::Value::String("wheeee".to_string()));
    event.add_field("extra_ham", libhoney::Value::String("cheese".to_string()));
    match event.send(&client).await {
        Ok(()) => {
            let response = client.responses().recv().await.unwrap();
            assert_eq!(response.error, None);
        }
        Err(e) => {
            log::error!("Could not send event: {}", e);
        }
    }
    client.close().await?;
    Ok(())
}
