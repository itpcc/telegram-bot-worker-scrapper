use std::io;

use http;
use snafu::{prelude::*, Report};

#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum Error {
    #[snafu(display("path error"))]
    PathEnv { source: io::Error },
    #[snafu(display("IO error"))]
    LevelFilterError {
        source: tracing::metadata::ParseLevelFilterError,
    },
    #[snafu(display("unsupport env"))]
    UnsupportEnv,
    #[snafu(display("overflow error"))]
    Overflow,
    #[snafu(display("Str UTF-8 decode error"))]
    StrUtf8Error { source: std::str::Utf8Error },
    #[snafu(display("serde json error"))]
    SerdeJsonError { source: serde_json::Error },
    #[snafu(display("invalid timestamp"))]
    NaiveDateTimeError,
    #[snafu(display("invalid Unix timestamp range"))]
    OffsetDateTimeRangeError { source: time::error::ComponentRange },
    #[snafu(display("tracing global default error"))]
    GlobalDefautError,
    #[snafu(display("invalid decimal"))]
    DecimalError,
    #[snafu(display("reconect error"))]
    ReconnectError,
    #[snafu(display("EMpty error"))]
    EmptyError,
    #[snafu(display("invalid JSON String"))]
    JsonStringError,
    #[snafu(display("Signal IO Error"))]
    SignalError { source: std::io::Error },
    #[snafu(display("WebSocket error"))]
    WSError {
        source: async_tungstenite::tungstenite::Error,
    },
    #[snafu(display("Message send error"))]
    SendError {
        source: tokio::sync::mpsc::error::SendError<crate::model::MessagePayload>,
    },
    #[snafu(display("HTTP error"))]
    HTTPError { source: http::Error },
    #[snafu(display("Reqwest error"))]
    ReqwestError { source: reqwest::Error },
    #[snafu(display("URL Parsing error"))]
    URLError { source: url::ParseError },
    #[snafu(display("Regex error"))]
    RegexError { source: regex::Error },
    #[snafu(display("Tokio Task Join error"))]
    TokioJoinError { source: tokio::task::JoinError },
    #[snafu(display("WebDriver Fantoccini CMD error"))]
    FantocciniCmdError { source: fantoccini::error::CmdError },
    #[snafu(display("WebDriver Fantoccini Session error"))]
    FantocciniSessionError {
        source: fantoccini::error::NewSessionError,
    },
    #[snafu(display("IO error"))]
    IOError { source: io::Error },
    #[snafu(display("System Time error"))]
    SystemTimeError { source: std::time::SystemTimeError },
}

impl Error {
    pub fn report(&self) -> () {
        match self {
            e => tracing::error!("error: error_msg {}", Report::from_error(e)),
        }
    }
}
