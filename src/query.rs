use std::{fmt, string::FromUtf8Error, time::Instant};

use git2::Repository;
use moka::sync::Cache;
use oxigraph::{io::GraphFormat, model::GraphNameRef, sparql::QueryResultsFormat};

use crate::{
    error::Error,
    graphs::read_all_graph_files,
    metrics::{GRAPH_PARSE_TIME, QUERY_PROCESSING_TIME},
    rdf::{to_turtle, RdfPrettifier},
};

#[derive(Debug)]
pub enum CacheLevel {
    Nothing,
    Graph,
    Query,
    Prettified,
}

impl fmt::Display for CacheLevel {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Clone)]
pub struct QueryCache {
    pub store_cache: Cache<u64, oxigraph::store::Store>,
    pub graphs_cache: Cache<u64, String>,
    pub query_cache: Cache<(u64, String), String>,
}

impl QueryCache {
    pub fn new() -> Self {
        Self {
            store_cache: Cache::new(1000),
            graphs_cache: Cache::new(1000),
            query_cache: Cache::new(1000),
        }
    }
}

/// Get graphs with cache. Return cache level alongside raw graph string.
pub async fn graphs_with_cache<P: RdfPrettifier>(
    rdf_prettifier: &P,
    repo: &Repository,
    cache: &QueryCache,
    timestamp: u64,
) -> Result<(String, CacheLevel), Error> {
    if let Some(graphs) = cache.graphs_cache.get(&timestamp) {
        Ok((graphs, CacheLevel::Prettified))
    } else if let Some(graph_store) = cache.store_cache.get(&timestamp) {
        let turtle = to_turtle(&graph_store)?;
        let prettified = rdf_prettifier.prettify(&turtle).await?;

        cache.graphs_cache.insert(timestamp, prettified.clone());
        Ok((prettified, CacheLevel::Graph))
    } else {
        let graph_store = read_files_into_graph_store(repo, timestamp).await?;
        let turtle = to_turtle(&graph_store)?;
        let prettified = rdf_prettifier.prettify(&turtle).await?;

        cache.store_cache.insert(timestamp, graph_store);
        cache.graphs_cache.insert(timestamp, prettified.clone());
        Ok((prettified, CacheLevel::Nothing))
    }
}

/// Query timestamp with cache. Return cache level alongside raw JSON result string.
pub async fn query_with_cache<P: RdfPrettifier>(
    _rdf_prettifier: &P,
    repo: &Repository,
    cache: &QueryCache,
    timestamp: u64,
    query: String,
) -> Result<(String, CacheLevel), Error> {
    if let Some(query_result) = cache.query_cache.get(&(timestamp, query.clone())) {
        Ok((query_result, CacheLevel::Query))
    } else if let Some(graph_store) = cache.store_cache.get(&timestamp) {
        let query_result = execute_query_in_store(&graph_store, &query)?;
        cache
            .query_cache
            .insert((timestamp, query), query_result.clone());

        Ok((query_result, CacheLevel::Graph))
    } else {
        let graph_store = read_files_into_graph_store(repo, timestamp).await?;
        let query_result = execute_query_in_store(&graph_store, &query)?;

        cache.store_cache.insert(timestamp, graph_store);
        cache
            .query_cache
            .insert((timestamp, query), query_result.clone());

        Ok((query_result, CacheLevel::Nothing))
    }
}

/// Execute sparql query in store and return JSON result as raw string.
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

/// Load graph store with all graphs for a given timestamp.
async fn read_files_into_graph_store(
    repo: &Repository,
    timestamp: u64,
) -> Result<oxigraph::store::Store, Error> {
    let store = oxigraph::store::Store::new()?;

    let graphs = read_all_graph_files(repo, timestamp).await?;
    if graphs.len() == 0 {
        return Ok(store);
    }

    // TODO: try to simply join all graphs without separating prefix and data.
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

/// Combine multiple graphs into one single graph (merging prefix sections and data separately).
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
