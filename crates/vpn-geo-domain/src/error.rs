#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum DomainError {
    #[error("configuration: {0}")]
    Config(String),
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("policy rejected: {0}")]
    Policy(String),
}

impl DomainError {
    pub fn config(value: impl Into<String>) -> Self {
        Self::Config(value.into())
    }

    pub fn invalid(value: impl Into<String>) -> Self {
        Self::InvalidArgument(value.into())
    }

    pub fn not_found(value: impl Into<String>) -> Self {
        Self::NotFound(value.into())
    }
}

pub type DomainResult<T> = Result<T, DomainError>;
