use vpn_geo_domain::DomainError;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("I/O: {0}")]
    Io(String),
    #[error("HTTP: {0}")]
    Http(String),
    #[error("NetworkManager: {0}")]
    NetworkManager(String),
    #[error("state: {0}")]
    State(String),
    #[error("timeout: {0}")]
    Timeout(String),
    #[error("internal: {0}")]
    Internal(String),
}

impl AppError {
    pub fn io(value: impl Into<String>) -> Self {
        Self::Io(value.into())
    }
    pub fn http(value: impl Into<String>) -> Self {
        Self::Http(value.into())
    }
    pub fn nm(value: impl Into<String>) -> Self {
        Self::NetworkManager(value.into())
    }
    pub fn state(value: impl Into<String>) -> Self {
        Self::State(value.into())
    }

    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Domain(DomainError::Config(_))
            | Self::Domain(DomainError::InvalidArgument(_)) => 2,
            Self::Domain(DomainError::NotFound(_)) => 3,
            _ => 1,
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;
