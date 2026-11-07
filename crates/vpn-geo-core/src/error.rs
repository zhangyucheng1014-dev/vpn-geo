use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("configuration error: {0}")]
    Config(String),
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("network request failed: {0}")]
    Http(String),
    #[error("NetworkManager error: {0}")]
    NetworkManager(String),
    #[error("state error: {0}")]
    State(String),
    #[error("operation timed out: {0}")]
    Timeout(String),
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl Error {
    pub fn config(value: impl Into<String>) -> Self {
        Self::Config(value.into())
    }

    pub fn http(value: impl Into<String>) -> Self {
        Self::Http(value.into())
    }

    pub fn nm(value: impl Into<String>) -> Self {
        Self::NetworkManager(value.into())
    }

    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Config(_) | Self::InvalidArgument(_) => 2,
            Self::NotFound(_) => 3,
            _ => 1,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub trait Context<T> {
    fn context(self, message: impl Into<String>) -> Result<T>;
}

impl<T, E> Context<T> for std::result::Result<T, E>
where
    E: std::fmt::Display,
{
    fn context(self, message: impl Into<String>) -> Result<T> {
        self.map_err(|error| Error::Internal(format!("{}: {error}", message.into())))
    }
}
