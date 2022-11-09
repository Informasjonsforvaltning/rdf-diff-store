use std::io;

use actix_web::{HttpResponse, ResponseError};

use crate::models;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    String(String),
    #[error("Unauthorized: {0}")]
    Unauthorized(String),
    #[error(transparent)]
    Utf8Error(#[from] std::str::Utf8Error),
    #[error(transparent)]
    FromUtf8Error(#[from] std::string::FromUtf8Error),
    #[error(transparent)]
    SerdeJsonError(#[from] serde_json::Error),
    #[error(transparent)]
    StorageError(#[from] oxigraph::store::StorageError),
    #[error(transparent)]
    LoaderError(#[from] oxigraph::store::LoaderError),
    #[error(transparent)]
    SerializerError(#[from] oxigraph::store::SerializerError),
    #[error(transparent)]
    EvaluationError(#[from] oxigraph::sparql::EvaluationError),
    #[error(transparent)]
    ReqwestError(#[from] reqwest::Error),
    #[error(transparent)]
    GitError(#[from] git2::Error),
    #[error(transparent)]
    IOError(#[from] io::Error),
    #[error(transparent)]
    JoinError(#[from] tokio::task::JoinError),
}

impl From<&str> for Error {
    fn from(e: &str) -> Self {
        Self::String(e.to_string())
    }
}

impl From<String> for Error {
    fn from(e: String) -> Self {
        Self::String(e)
    }
}

impl ResponseError for Error {
    fn error_response(&self) -> HttpResponse {
        use Error::*;

        match self {
            GitError(e) => {
                tracing::error!(error = e.to_string().as_str(), "git error occured")
            }
            e => {
                tracing::warn!(
                    error = e.to_string().as_str(),
                    "Error occured when handling request"
                )
            }
        };

        match self {
            Unauthorized(_) => HttpResponse::Unauthorized().json(models::Error::message(self)),
            _ => HttpResponse::InternalServerError().json(models::Error::error(self)),
        }
    }
}

impl models::Error {
    fn message<S: ToString>(message: S) -> Self {
        models::Error {
            message: Some(message.to_string()),
            ..Default::default()
        }
    }
    fn error<S: ToString>(error: S) -> Self {
        models::Error {
            error: Some(error.to_string()),
            ..Default::default()
        }
    }
}
