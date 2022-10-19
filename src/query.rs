use std::{string::FromUtf8Error, time::Instant};

use lazy_static::lazy_static;
use moka::sync::Cache;
use oxigraph::{io::GraphFormat, model::GraphNameRef, sparql::QueryResultsFormat};

use crate::{
    error::Error,
    git::fetch_graphs,
    metrics::{GRAPH_PARSE_TIME, QUERY_PROCESSING_TIME},
    rdf::to_turtle,
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

pub async fn graphs_with_cache(cache: &QueryCache, timestamp: u64) -> Result<(String, u64), Error> {
    if let Some(graph_store) = cache.graphs_cache.get(&timestamp) {
        let query_result = to_turtle(&graph_store)?;

        Ok((query_result, 1))
    } else {
        let graph_store = load_graph_store(timestamp).await?;
        let query_result = to_turtle(&graph_store)?;

        cache.graphs_cache.insert(timestamp, graph_store);

        Ok((query_result, 0))
    }
}

pub async fn query_with_cache(
    cache: &QueryCache,
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
        let graph_store = load_graph_store(timestamp).await?;
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

async fn load_graph_store(timestamp: u64) -> Result<oxigraph::store::Store, Error> {
    let store = oxigraph::store::Store::new()?;

    let graphs = fetch_graphs(timestamp).await?;
    if graphs.len() == 0 {
        return Ok(store);
    }

    let graph = combine_graphs(graphs)?;
    let start_time = Instant::now();

    // Combining all graphs into a single one and using bulk loader is a bit
    // faster than parsing one at a time (with or without bulk loader).
    store.bulk_loader().load_graph(
        graph.as_ref(),
        GraphFormat::Turtle,
        GraphNameRef::DefaultGraph,
        None,
    )?;

    let elapsed_millis = start_time.elapsed().as_millis();
    GRAPH_PARSE_TIME.observe(elapsed_millis as f64 / 1000.0);

    Ok(store)
}

fn combine_graphs(graphs: Vec<Vec<u8>>) -> Result<String, Error> {
    let graph_strs = graphs
        .into_iter()
        .map(|graph| String::from_utf8(graph))
        .collect::<Result<Vec<String>, FromUtf8Error>>()?;

    let graph_count = graph_strs.len();
    let mut prefix_merge = Vec::with_capacity(graph_count);
    let mut graph_merge = Vec::with_capacity(graph_count);

    for i in 0..graph_strs.len() {
        let split = graph_strs[i].split_once("\n");
        if let Some((prefix_part, graph_part)) = split {
            prefix_merge.push(prefix_part);
            graph_merge.push(graph_part);
        }
    }

    let mut combined = prefix_merge;
    combined.append(&mut graph_merge);
    Ok(combined.join("\n"))
}
