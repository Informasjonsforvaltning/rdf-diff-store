use std::env;

use lazy_static::lazy_static;
use oxigraph::{io::GraphFormat, model::GraphNameRef};
// use reqwest::StatusCode;
// use serde_json::json;

use crate::error::Error;

lazy_static! {
    static ref PRETTY_PRINT_URL: String = env::var("PRETTY_PRINT_URL").unwrap_or_else(|e| {
        tracing::error!(error = e.to_string().as_str(), "PRETTY_PRINT_URL not found");
        std::process::exit(1)
    });
}

pub fn to_turtle(store: &oxigraph::store::Store) -> Result<String, Error> {
    let mut buff = Vec::new();
    store.dump_graph(&mut buff, GraphFormat::Turtle, GraphNameRef::DefaultGraph)?;
    let turtle = String::from_utf8(buff)?;
    Ok(turtle)
}

pub async fn pretty_print(_http_client: &reqwest::Client, graph: &str) -> Result<String, Error> {
    return Ok(graph.to_string());

    // FIXME: Use prettyprint api

    // let response = http_client
    //     .post(format!("{}/api/prettify", PRETTY_PRINT_URL.clone()))
    //     .json(&json!({
    //         "format": "text/turtle",
    //         "output_format": "text/turtle",
    //         "graph": graph,
    //     }))
    //     .send()
    //     .await?;

    // match response.status() {
    //     StatusCode::OK => Ok(response.text().await?),
    //     _ => Err(format!(
    //         "Invalid response from pretty print api: {} - {}",
    //         response.status(),
    //         response.text().await?
    //     )
    //     .into()),
    // }
}
