use std::env;

use async_trait::async_trait;
use lazy_static::lazy_static;
use oxigraph::{io::GraphFormat, model::GraphNameRef};
use reqwest::StatusCode;
use serde_json::json;

use crate::error::Error;

lazy_static! {
    static ref PRETTY_PRINT_URL: String = env::var("PRETTY_PRINT_URL").unwrap_or_else(|e| {
        tracing::error!(error = e.to_string().as_str(), "PRETTY_PRINT_URL not found");
        std::process::exit(1)
    });
}

#[async_trait]
pub trait PrettyPrint {
    fn new() -> Self;
    async fn pretty_print(&self, graph: &str) -> Result<String, Error>;
}

#[derive(Clone)]
pub struct APIPrettyPrinter(reqwest::Client);

#[async_trait]
impl PrettyPrint for APIPrettyPrinter {
    fn new() -> Self {
        APIPrettyPrinter(reqwest::Client::new())
    }

    async fn pretty_print(&self, graph: &str) -> Result<String, Error> {
        let response = self
            .0
            .post(format!("{}/api/prettify", PRETTY_PRINT_URL.clone()))
            .json(&json!({
                "format": "text/turtle",
                "output_format": "text/turtle",
                "graph": graph,
            }))
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => Ok(response.text().await?),
            _ => Err(format!(
                "Invalid response from pretty print api: {} - {}",
                response.status(),
                response.text().await?
            )
            .into()),
        }
    }
}

pub struct NoOpPrettyPrinter {}

#[async_trait]
impl PrettyPrint for NoOpPrettyPrinter {
    fn new() -> Self {
        NoOpPrettyPrinter {}
    }

    async fn pretty_print(&self, graph: &str) -> Result<String, Error> {
        Ok(graph.to_string())
    }
}

pub fn to_turtle(store: &oxigraph::store::Store) -> Result<String, Error> {
    let mut buff = Vec::new();
    store.dump_graph(&mut buff, GraphFormat::Turtle, GraphNameRef::DefaultGraph)?;
    let turtle = String::from_utf8(buff)?;
    Ok(turtle)
}
