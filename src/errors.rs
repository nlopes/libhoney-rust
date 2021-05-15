use std::fmt;
use std::io;

use futures::channel::oneshot;
use futures::task::SpawnError;

/// Result shorthand for a `std::result::Result` wrapping our own `Error`
pub type Result<T> = std::result::Result<T, Error>;

/// Type of error, exposed through `Error` member `kind`
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ErrorKind {
    /// Event has no populated fields
    MissingEventFields,

    /// Mandatory client/transmission Option is missing
    MissingOption,

    /// User Func error
    UserFuncError,

    /// Sender full
    ChannelError,

    /// Any IO related error
    Io,

    /// Failed to spawn future
    Spawn,
}

/// Error
#[derive(Debug)]
pub struct Error {
    /// Error message
    pub message: String,
    /// Type of error
    pub kind: ErrorKind,
}

impl Error {
    #[doc(hidden)]
    pub(crate) fn missing_event_fields() -> Self {
        Self {
            message: String::from("event has no data"),
            kind: ErrorKind::MissingEventFields,
        }
    }

    #[doc(hidden)]
    pub(crate) fn missing_option(option: &str, extra: &str) -> Self {
        Self {
            message: format!("missing option '{}', {}", option, extra),
            kind: ErrorKind::MissingOption,
        }
    }

    #[doc(hidden)]
    pub(crate) fn sender_full(sender: &str) -> Self {
        Self {
            message: format!("sender '{}' is full", sender),
            kind: ErrorKind::ChannelError,
        }
    }

    #[doc(hidden)]
    pub(crate) fn with_description(description: &str, kind: ErrorKind) -> Self {
        Self {
            message: format!("error: {}", description),
            kind,
        }
    }
}

impl std::error::Error for Error {
    fn description(&self) -> &str {
        &*self.message
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "{}", self.message)
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::with_description(&e.to_string(), ErrorKind::Io)
    }
}

impl<T> From<async_channel::SendError<T>> for Error {
    fn from(e: async_channel::SendError<T>) -> Self {
        Self::with_description(&e.to_string(), ErrorKind::ChannelError)
    }
}

impl From<oneshot::Canceled> for Error {
    fn from(e: oneshot::Canceled) -> Self {
        Self::with_description(&e.to_string(), ErrorKind::ChannelError)
    }
}
impl From<SpawnError> for Error {
    fn from(e: SpawnError) -> Self {
        Self::with_description(&e.to_string(), ErrorKind::Spawn)
    }
}
