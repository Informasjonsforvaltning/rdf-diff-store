use std::{io::Cursor, time::Instant};

use lazy_static::lazy_static;
use moka::sync::Cache;
use oxigraph::{io::GraphFormat, model::GraphNameRef, sparql::QueryResultsFormat};
use reqwest::StatusCode;

use crate::{
    error::Error,
    metrics::{DATA_FETCH_TIME, GRAPH_PARSE_TIME, QUERY_PROCESSING_TIME},
    DIFF_STORE_URL,
};

lazy_static! {
    static ref DIFF_STORE_API_LOCK: async_lock::Mutex<()> = async_lock::Mutex::new(());
}

#[derive(Clone)]
pub struct QueryCache {
    pub graphs_cache: Cache<u64, oxigraph::store::Store>,
    pub query_cache: Cache<(u64, String), String>,
}

impl QueryCache {
    pub fn new() -> Self {
        Self {
            graphs_cache: Cache::new(1000),
            query_cache: Cache::new(1000),
        }
    }
}

pub async fn graphs_with_cache(
    cache: &QueryCache,
    http_client: &reqwest::Client,
    timestamp: u64,
) -> Result<(String, u64), Error> {
    if let Some(graph_store) = cache.graphs_cache.get(&timestamp) {
        let query_result = to_turtle(&graph_store)?;

        Ok((query_result, 1))
    } else {
        let graph_store = load_graph_store(http_client, timestamp).await?;
        let query_result = to_turtle(&graph_store)?;

        cache.graphs_cache.insert(timestamp, graph_store);

        Ok((query_result, 0))
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
    // Diff store repo is guarded by lock, no point in sending more than one request at once.
    let diff_repo_guard = DIFF_STORE_API_LOCK.lock().await;

    let start_time = Instant::now();
    let response = http_client
        .get(format!("{}/api/graphs/{timestamp}", DIFF_STORE_URL.clone()))
        .query(&[("raw", "true")])
        //.header("X-API-KEY", DIFF_STORE_API_KEY.clone())
        .send()
        .await?;
    let elapsed_millis = start_time.elapsed().as_millis();

    drop(diff_repo_guard);

    match response.status() {
        StatusCode::OK => {
            DATA_FETCH_TIME.observe(elapsed_millis as f64 / 1000.0);
            Ok(response.text().await?)
        }
        _ => Err(response.text().await?.into()),
    }
}

pub fn to_turtle(store: &oxigraph::store::Store) -> Result<String, Error> {
    let mut buff = Cursor::new(Vec::new());
    store.dump_graph(&mut buff, GraphFormat::Turtle, GraphNameRef::DefaultGraph)?;

    String::from_utf8(buff.into_inner()).map_err(|e| e.to_string().into())
}
