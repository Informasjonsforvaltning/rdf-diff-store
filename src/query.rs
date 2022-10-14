use std::time::Instant;

use moka::sync::Cache;
use oxigraph::{io::GraphFormat, model::GraphNameRef, sparql::QueryResultsFormat};

use crate::{
    error::Error,
    metrics::{DATA_FETCH_TIME, GRAPH_PARSE_TIME, QUERY_PROCESSING_TIME},
    DIFF_STORE_URL,
};

#[derive(Clone)]
pub struct QueryCache {
    graphs_cache: Cache<u64, oxigraph::store::Store>,
    query_cache: Cache<(u64, String), String>,
}

impl QueryCache {
    pub fn new() -> Self {
        Self {
            graphs_cache: Cache::new(1000),
            query_cache: Cache::new(1000),
        }
    }
}

pub async fn query_with_cache(
    cache: &QueryCache,
    http_client: &reqwest::Client,
    timestamp: u64,
    query: String,
) -> Result<(String, u64), Error> {
    if let Some(query_result) = cache.query_cache.get(&(timestamp, query.clone())) {
        Ok((query_result, 2))
    } else if let Some(graph_store) = cache.graphs_cache.get(&timestamp) {
        let query_result = execute_query_in_store(&graph_store, &query)?;
        cache
            .query_cache
            .insert((timestamp, query), query_result.clone());

        Ok((query_result, 1))
    } else {
        let graph_store = load_graph_store(http_client, timestamp).await?;
        let query_result = execute_query_in_store(&graph_store, &query)?;

        cache.graphs_cache.insert(timestamp, graph_store);
        cache
            .query_cache
            .insert((timestamp, query), query_result.clone());

        Ok((query_result, 0))
    }
}

fn execute_query_in_store(store: &oxigraph::store::Store, query: &str) -> Result<String, Error> {
    let start_time = Instant::now();
    let query_result = store.query(query)?;
    let elapsed_millis = start_time.elapsed().as_millis();
    QUERY_PROCESSING_TIME.observe(elapsed_millis as f64 / 1000.0);

    let mut results = Vec::new();
    query_result.write(&mut results, QueryResultsFormat::Json)?;
    let raw_json = String::from_utf8(results)?;
    Ok(raw_json)
}

async fn load_graph_store(
    http_client: &reqwest::Client,
    timestamp: u64,
) -> Result<oxigraph::store::Store, Error> {
    let text = fetch_graph(http_client, timestamp).await?;
    let start_time = Instant::now();
    let store = oxigraph::store::Store::new()?;
    let graphs = text.split("# ---");
    for graph in graphs {
        if graph.contains("<") {
            store.load_graph(
                graph.to_string().as_ref(),
                GraphFormat::Turtle,
                GraphNameRef::DefaultGraph,
                None,
            )?;
        }
    }
    let elapsed_millis = start_time.elapsed().as_millis();
    GRAPH_PARSE_TIME.observe(elapsed_millis as f64 / 1000.0);
    Ok(store)
}

async fn fetch_graph(http_client: &reqwest::Client, timestamp: u64) -> Result<String, Error> {
    let start_time = Instant::now();
    let response = http_client
        .get(format!(
            "{}/api/graphs/{timestamp}?raw=true",
            DIFF_STORE_URL.clone()
        ))
        //.header("X-API-KEY", DIFF_STORE_API_KEY.clone())
        .send()
        .await?;
    let elapsed_millis = start_time.elapsed().as_millis();
    DATA_FETCH_TIME.observe(elapsed_millis as f64 / 1000.0);

    let text = response.text().await?;
    Ok(text)
}
