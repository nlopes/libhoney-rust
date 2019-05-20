use reqwest::StatusCode;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq)]
pub struct Response<T> {
    pub status_code: StatusCode,
    pub body: String,
    pub duration: std::time::Duration,
    pub metadata: Option<T>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct HoneyResponse {
    pub(crate) status: usize,
    pub(crate) error: Option<String>,
}
