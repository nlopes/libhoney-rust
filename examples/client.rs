use libhoney::{Error, FieldHolder};

#[async_std::main]
async fn main() -> Result<(), Error> {
    env_logger::init();

    let client = libhoney::init(libhoney::Config {
        options: libhoney::client::Options {
            api_key: std::env::var("HONEYCOMB_API_KEY").expect("need to set HONEYCOMB_API_KEY"),
            dataset: std::env::var("HONEYCOMB_DATASET").expect("need to set HONEYCOMB_DATASET"),
            ..Default::default()
        },
        transmission_options: libhoney::transmission::Options::default(),
    });
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
