use steam_vent::NetworkError;

use crate::cdn::manifest::error::ManifestError;

#[derive(Debug, Error)]
pub enum Error {
    #[error("unexpected: {0}")]
    Unexpected(String),
    #[error("web request - {0}")]
    Request(String),
    #[error("{0}")]
    Network(String),
    #[error("malformed vdf - {0}")]
    InvalidVDF(String),
    #[error("manifest {}", 0.to_string())]
    Manifest(ManifestError),
    #[error("unexpected none")]
    NoneOption,
}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        Self::Request(err.to_string())
    }
}

impl From<NetworkError> for Error {
    fn from(err: NetworkError) -> Self {
        Self::Network(err.to_string())
    }
}

impl From<keyvalues_parser::error::Error> for Error {
    fn from(err: keyvalues_parser::error::Error) -> Self {
        Self::InvalidVDF(err.to_string())
    }
}

impl From<ManifestError> for Error {
    fn from(err: ManifestError) -> Self {
        Self::Manifest(err)
    }
}
