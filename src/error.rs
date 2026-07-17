use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Message(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("missing required dependency: {0}")]
    DependencyMissing(String),
    #[error("{0}")]
    Network(String),
    #[error("{0}")]
    TrainerNotFound(String),
    #[error("{0}")]
    InvalidPayload(String),
}

#[derive(Debug, Serialize)]
pub struct Failure<'a> {
    pub schema_version: u8,
    pub success: bool,
    pub operation: &'a str,
    pub appid: u32,
    pub error_code: &'a str,
    pub message: String,
}

pub fn json_failure(
    operation: &str,
    appid: u32,
    code: i32,
    error_code: &str,
    message: impl Into<String>,
) -> ! {
    let value = Failure {
        schema_version: 1,
        success: false,
        operation,
        appid,
        error_code,
        message: message.into(),
    };
    match serde_json::to_string(&value) {
        Ok(encoded) => println!("{encoded}"),
        Err(error) => eprintln!("ERROR: could not serialize JSON failure: {error}"),
    }
    std::process::exit(code)
}
