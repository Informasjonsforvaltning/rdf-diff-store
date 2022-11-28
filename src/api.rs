use std::env;

use actix_web::{get, HttpRequest, Responder};
use lazy_static::lazy_static;

use crate::error::Error;

lazy_static! {
    static ref API_KEY: String = env::var("API_KEY").unwrap_or_else(|e| {
        tracing::error!(error = e.to_string().as_str(), "API_KEY not found");
        std::process::exit(1)
    });
}

#[get("/livez")]
pub async fn livez() -> Result<impl Responder, Error> {
    Ok("ok")
}

#[get("/readyz")]
pub async fn readyz() -> Result<impl Responder, Error> {
    Ok("ok")
}

/// Returns an error if X-API-KEY header does not match API_KEY environment variable.
pub fn validate_api_key(request: HttpRequest) -> Result<(), Error> {
    let token = request
        .headers()
        .get("X-API-KEY")
        .ok_or(Error::Unauthorized("X-API-KEY header missing".to_string()))?
        .to_str()
        .map_err(|_| Error::Unauthorized("invalid api key".to_string()))?;

    if token == API_KEY.clone() {
        Ok(())
    } else {
        Err(Error::Unauthorized("incorrect api key".to_string()))
    }
}
