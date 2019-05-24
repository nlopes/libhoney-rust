use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    pub status_code: Option<StatusCode>,
    pub body: Option<String>,
    pub duration: std::time::Duration,
    pub metadata: Option<Value>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct HoneyResponse {
    pub(crate) status: u16,
    pub(crate) error: Option<String>,
}
