use lazy_static::lazy_static;
use prometheus::{
    Encoder, Gauge, Histogram, HistogramOpts, HistogramVec, IntCounterVec, Opts, Registry,
};

use crate::{error::Error, CACHE};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();
    pub static ref GRAPH_CACHE_COUNT: Gauge = Gauge::new(
        "graph_cache_count",
        "Number of timestamped graphs in cached"
    )
    .unwrap_or_else(|e| {
        tracing::error!(error = e.to_string(), "graph_cache_count metric error");
        std::process::exit(1);
    });
    pub static ref QUERY_CACHE_COUNT: Gauge = Gauge::new(
        "query_cache_count",
        "Number of timestamped queries in cache"
    )
    .unwrap_or_else(|e| {
        tracing::error!(error = e.to_string(), "query_cache_count metric error");
        std::process::exit(1);
    });
    pub static ref PROCESSED_REQUESTS: IntCounterVec = IntCounterVec::new(
        Opts::new("processed_requests", "Processed Query Requests"),
        &["endpoint", "status"]
    )
    .unwrap_or_else(|e| {
        tracing::error!(error = e.to_string(), "processed_requests metric error");
        std::process::exit(1);
    });
    pub static ref RESPONSE_TIME: HistogramVec = HistogramVec::new(
        HistogramOpts {
            common_opts: Opts::new("response_time", "Response Times"),
            buckets: vec![0.05, 0.25, 1.0, 2.5, 5.0, 10.0, 25.0],
        },
        &["endpoint", "cache_lvl"]
    )
    .unwrap_or_else(|e| {
        tracing::error!(error = e.to_string(), "response_time");
        std::process::exit(1);
    });
    pub static ref QUERY_PROCESSING_TIME: Histogram = Histogram::with_opts(HistogramOpts {
        common_opts: Opts::new("query_processing_time", "Query Processing Times"),
        buckets: vec![0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05, 0.1],
    })
    .unwrap_or_else(|e| {
        tracing::error!(error = e.to_string(), "query_processing_time");
        std::process::exit(1);
    });
    pub static ref REPO_FETCH_TIME: HistogramVec = HistogramVec::new(
        HistogramOpts {
            common_opts: Opts::new("repo_fetch_time", "Repo Fetch Times"),
            buckets: vec![0.01, 0.05, 0.1, 0.5, 1.0, 2.5, 5.0],
        },
        &["change"]
    )
    .unwrap_or_else(|e| {
        tracing::error!(error = e.to_string(), "repo_fetch_time");
        std::process::exit(1);
    });
    pub static ref REPO_CHEKOUT_TIME: HistogramVec = HistogramVec::new(
        HistogramOpts {
            common_opts: Opts::new("repo_checkout_time", "Repo Checkout Times"),
            buckets: vec![0.01, 0.05, 0.1, 0.5, 1.0, 2.5, 5.0],
        },
        &["change"]
    )
    .unwrap_or_else(|e| {
        tracing::error!(error = e.to_string(), "repo_checkout_time");
        std::process::exit(1);
    });
    pub static ref FILE_READ_TIME: Histogram = Histogram::with_opts(HistogramOpts {
        common_opts: Opts::new("file_read_time", "File Read Time"),
        buckets: vec![0.01, 0.05, 0.1, 0.5, 1.0, 2.5, 5.0],
    })
    .unwrap_or_else(|e| {
        tracing::error!(error = e.to_string(), "file_read_time");
        std::process::exit(1);
    });
    pub static ref GRAPH_PARSE_TIME: Histogram = Histogram::with_opts(HistogramOpts {
        common_opts: Opts::new("graph_parse_time", "Data Fetch Times"),
        buckets: vec![0.1, 0.5, 1.0, 2.5, 5.0, 10.0, 25.0],
    })
    .unwrap_or_else(|e| {
        tracing::error!(error = e.to_string(), "graph_parse_time");
        std::process::exit(1);
    });
}

pub fn register_metrics() {
    REGISTRY
        .register(Box::new(PROCESSED_REQUESTS.clone()))
        .unwrap_or_else(|e| {
            tracing::error!(
                error = e.to_string(),
                "processed_query_requests collector error"
            );
            std::process::exit(1);
        });

    REGISTRY
        .register(Box::new(RESPONSE_TIME.clone()))
        .unwrap_or_else(|e| {
            tracing::error!(error = e.to_string(), "response_time collector error");
            std::process::exit(1);
        });

    REGISTRY
        .register(Box::new(GRAPH_CACHE_COUNT.clone()))
        .unwrap_or_else(|e| {
            tracing::error!(error = e.to_string(), "graph_cache_count collector error");
            std::process::exit(1);
        });

    REGISTRY
        .register(Box::new(QUERY_CACHE_COUNT.clone()))
        .unwrap_or_else(|e| {
            tracing::error!(error = e.to_string(), "query_cache_count collector error");
            std::process::exit(1);
        });

    REGISTRY
        .register(Box::new(QUERY_PROCESSING_TIME.clone()))
        .unwrap_or_else(|e| {
            tracing::error!(
                error = e.to_string(),
                "query_processing_time collector error"
            );
            std::process::exit(1);
        });

    REGISTRY
        .register(Box::new(REPO_CHEKOUT_TIME.clone()))
        .unwrap_or_else(|e| {
            tracing::error!(error = e.to_string(), "repo_checkout_time collector error");
            std::process::exit(1);
        });

    REGISTRY
        .register(Box::new(REPO_FETCH_TIME.clone()))
        .unwrap_or_else(|e| {
            tracing::error!(error = e.to_string(), "repo_fetch_time collector error");
            std::process::exit(1);
        });

    REGISTRY
        .register(Box::new(FILE_READ_TIME.clone()))
        .unwrap_or_else(|e| {
            tracing::error!(error = e.to_string(), "file_read_time collector error");
            std::process::exit(1);
        });

    REGISTRY
        .register(Box::new(GRAPH_PARSE_TIME.clone()))
        .unwrap_or_else(|e| {
            tracing::error!(error = e.to_string(), "graph_parse_time collector error");
            std::process::exit(1);
        });
}

pub fn get_metrics() -> Result<String, Error> {
    let cache = CACHE.clone();
    GRAPH_CACHE_COUNT.set(cache.graphs_cache.entry_count() as f64);
    QUERY_CACHE_COUNT.set(cache.query_cache.entry_count() as f64);

    let mut buffer = Vec::new();

    prometheus::TextEncoder::new()
        .encode(&REGISTRY.gather(), &mut buffer)
        .map_err(|e| e.to_string())?;

    let metrics = String::from_utf8(buffer).map_err(|e| e.to_string())?;
    Ok(metrics)
}
