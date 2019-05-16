mod transmission;
mod response;

use transmission::{Transmission, TransmissionOptions};

const DEFAULT_API_HOST: &str = "https://api.honeycomb.io";
const DEFAULT_API_KEY: &str = "";
const DEFAULT_DATASET: &str = "libhoney-rust dataset";
const DEFAULT_SAMPLE_RATE: usize = 1;

#[derive(Debug, Clone, Copy)]
pub struct Options {
    api_key: &'static str,
    api_host: &'static str,
    dataset: &'static str,
    sample_rate: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            api_key: DEFAULT_API_KEY,
            dataset: DEFAULT_DATASET,
            api_host: DEFAULT_API_HOST,
            sample_rate: DEFAULT_SAMPLE_RATE,
        }
    }
}

pub struct HoneyClient {
    options: Options,
    transmission: Transmission,
}

impl HoneyClient {
    pub fn new(options: Options, transmission: Transmission) -> Self {
        let mut c = HoneyClient {
            options,
            transmission,
        };
        c.start();
        c
    }

    fn start(&mut self) {
        dbg!("HoneyClient start");
        self.transmission.start();
    }
}

pub fn init(options: Options, transmission_options: TransmissionOptions) -> HoneyClient {
    let transmission = Transmission::new(transmission_options);
    let client = HoneyClient::new(options, transmission);
    client
}

#[cfg(test)]
mod tests {
    use super::*;

    // #[test]
    // fn it_works() {
    //     let options = Options {
    //         api_key: "dope",
    //         ..Default::default()
    //     };
        //let client = init(options, Default::default());
        //assert_eq!(client.options.api_host, "https://api.honeycomb.io");
//}
}
