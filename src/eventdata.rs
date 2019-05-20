use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Serialize)]
pub(crate) struct EventData {
    pub(crate) data: HashMap<String, Value>,
    pub(crate) time: DateTime<Utc>,
    pub(crate) samplerate: usize,
}
