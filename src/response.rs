use reqwest::StatusCode;

#[derive(Clone)]
pub struct Response {
    pub status_code: StatusCode,
    pub body: String,
    pub duration: std::time::Duration,
}
