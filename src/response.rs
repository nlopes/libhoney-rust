use reqwest::StatusCode;

#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    pub status_code: StatusCode,
    pub body: String,
    pub duration: std::time::Duration,
}
