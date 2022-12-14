use std::{env, time::Instant};

use async_trait::async_trait;
use lazy_static::lazy_static;
use oxigraph::{io::GraphFormat, model::GraphNameRef};
use reqwest::StatusCode;
use serde_json::json;

use crate::{error::Error, metrics::RDF_PRETTIFIER_TIME};

lazy_static! {
    static ref RDF_PRETTIFIER_URL: String = env::var("RDF_PRETTIFIER_URL").unwrap_or_else(|e| {
        tracing::error!(
            error = e.to_string().as_str(),
            "RDF_PRETTIFIER_URL not found"
        );
        std::process::exit(1)
    });
    static ref RDF_PRETTIFIER_API_KEY: String =
        env::var("RDF_PRETTIFIER_API_KEY").unwrap_or_else(|e| {
            tracing::error!(
                error = e.to_string().as_str(),
                "RDF_PRETTIFIER_API_KEY not found"
            );
            std::process::exit(1)
        });
}

#[async_trait]
pub trait RdfPrettifier {
    fn new() -> Self;
    async fn prettify(&self, graph: &str) -> Result<String, Error>;
}

#[derive(Clone)]
pub struct APIPrettifier(reqwest::Client);

#[async_trait]
impl RdfPrettifier for APIPrettifier {
    fn new() -> Self {
        APIPrettifier(reqwest::Client::new())
    }

    async fn prettify(&self, graph: &str) -> Result<String, Error> {
        let retry_attempts = 5;
        let response = {
            let mut r = Err(Error::String(
                "invalid number of prettification attempts".into(),
            ));
            for _ in 0..retry_attempts {
                let start_time = Instant::now();
                r = self
                    .0
                    .post(RDF_PRETTIFIER_URL.clone())
                    .header("X-API-KEY", RDF_PRETTIFIER_API_KEY.clone())
                    .json(&json!({
                        "format": "text/turtle",
                        "output_format": "text/turtle",
                        "graph": graph,
                    }))
                    .send()
                    .await
                    .map_err(|e| Error::from(e));

                if r.is_ok() {
                    let elapsed_millis = start_time.elapsed().as_millis();
                    RDF_PRETTIFIER_TIME.observe(elapsed_millis as f64 / 1000.0);
                    break;
                }
            }
            r
        }?;

        match response.status() {
            StatusCode::OK => Ok(response.text().await?),
            _ => Err(format!(
                "invalid response from pretty print api: {} - {}",
                response.status(),
                response.text().await?
            )
            .into()),
        }
    }
}


pub fn to_turtle(store: &oxigraph::store::Store) -> Result<String, Error> {
    let mut buff = Vec::new();
    store.dump_graph(&mut buff, GraphFormat::Turtle, GraphNameRef::DefaultGraph)?;
    let turtle = String::from_utf8(buff)?;
    Ok(turtle)
}
