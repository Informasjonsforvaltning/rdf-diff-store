use std::time::Instant;

use actix_web::{get, middleware::Logger, web, App, HttpResponse, HttpServer, Responder};
use rdf_diff_store::git::{ReusableRepoPool, GIT_REPOS_ROOT_PATH};
use rdf_diff_store::metrics::CACHE_COUNT;

use rdf_diff_store::rdf::{APIPrettyPrinter, PrettyPrint};
use rdf_diff_store::{
    error::Error,
    metrics::{get_metrics, register_metrics, PROCESSED_REQUESTS, RESPONSE_TIME},
    query::QueryCache,
    query::{graphs_with_cache, query_with_cache},
};
use serde::Deserialize;

#[get("/livez")]
async fn livez() -> Result<impl Responder, Error> {
    Ok("ok")
}

#[get("/readyz")]
async fn readyz() -> Result<impl Responder, Error> {
    Ok("ok")
}

#[get("/metrics")]
async fn metrics_endpoint(state: web::Data<State>) -> impl Responder {
    CACHE_COUNT
        .with_label_values(&["graphs"])
        .set(state.cache.graphs_cache.entry_count() as i64);
    CACHE_COUNT
        .with_label_values(&["queries"])
        .set(state.cache.query_cache.entry_count() as i64);
    CACHE_COUNT
        .with_label_values(&["stores"])
        .set(state.cache.store_cache.entry_count() as i64);

    match get_metrics() {
        Ok(metrics) => metrics,
        Err(e) => {
            tracing::error!(error = e.to_string(), "unable to gather metrics");
            "".to_string()
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SparqlQueryParams {
    query: String,
}

#[get("/api/sparql/{timestamp}")]
async fn get_api_sparql(
    //request: HttpRequest,
    repos: web::Data<async_lock::Mutex<ReusableRepoPool>>,
    path: web::Path<u64>,
    query: web::Query<SparqlQueryParams>,
    state: web::Data<State>,
) -> Result<impl Responder, Error> {
    //validate_api_key(request)?;

    let timestamp = path.into_inner();
    let query_params = query.into_inner();

    let repo = ReusableRepoPool::pop(&repos).await;
    let start_time = Instant::now();
    let result = query_with_cache(
        &state.pretty_printer,
        &repo,
        &state.cache,
        timestamp,
        query_params.query,
    )
    .await;
    let elapsed_millis = start_time.elapsed().as_millis();
    ReusableRepoPool::push(&repos, repo).await;

    match result {
        Ok(ok) => {
            PROCESSED_REQUESTS
                .with_label_values(&["GET", "/api/sparql", "success"])
                .inc();
            RESPONSE_TIME
                .with_label_values(&["GET", "/api/sparql", &format!("{}", ok.1)])
                .observe(elapsed_millis as f64 / 1000.0);
            Ok(HttpResponse::Ok()
                .content_type(mime::APPLICATION_JSON)
                .message_body(ok.0))
        }
        Err(e) => {
            PROCESSED_REQUESTS
                .with_label_values(&["GET", "/api/sparql", "error"])
                .inc();
            Err(e)
        }
    }
}

#[get("/api/graphs/{timestamp}")]
async fn get_api_graphs(
    //request: HttpRequest,
    repos: web::Data<async_lock::Mutex<ReusableRepoPool>>,
    path: web::Path<u64>,
    state: web::Data<State>,
) -> Result<impl Responder, Error> {
    //validate_api_key(request)?;

    let timestamp = path.into_inner();

    let repo = ReusableRepoPool::pop(&repos).await;
    let start_time = Instant::now();
    let result = graphs_with_cache(&state.pretty_printer, &repo, &state.cache, timestamp).await;
    let elapsed_millis = start_time.elapsed().as_millis();
    ReusableRepoPool::push(&repos, repo).await;

    match result {
        Ok(ok) => {
            PROCESSED_REQUESTS
                .with_label_values(&["GET", "/api/graphs", "success"])
                .inc();
            RESPONSE_TIME
                .with_label_values(&["GET", "/api/graphs", &format!("{}", ok.1)])
                .observe(elapsed_millis as f64 / 1000.0);
            Ok(HttpResponse::Ok()
                .content_type("text/turtle")
                .message_body(ok.0))
        }
        Err(e) => {
            PROCESSED_REQUESTS
                .with_label_values(&["GET", "/api/graphs", "error"])
                .inc();
            Err(e)
        }
    }
}

#[derive(Clone)]
struct State {
    cache: QueryCache,
    pretty_printer: APIPrettyPrinter,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_current_span(false)
        .init();

    register_metrics();

    let repo_pool = ReusableRepoPool::new(GIT_REPOS_ROOT_PATH.clone(), 32).unwrap_or_else(|e| {
        tracing::error!(error = e.to_string().as_str(), "Unable to create repo pool");
        std::process::exit(1)
    });
    let repo_pool = web::Data::new(async_lock::Mutex::new(repo_pool));

    let state = State {
        cache: QueryCache::new(),
        pretty_printer: APIPrettyPrinter::new(),
    };

    HttpServer::new(move || {
        App::new()
            .wrap(
                Logger::default()
                    .exclude("/livez".to_string())
                    .exclude("/readyz".to_string())
                    .log_target("http"),
            )
            .app_data(web::Data::new(state.clone()))
            .app_data(web::Data::clone(&repo_pool))
            .service(livez)
            .service(readyz)
            .service(metrics_endpoint)
            .service(get_api_sparql)
            .service(get_api_graphs)
    })
    .bind(("0.0.0.0", 8080))?
    .workers(32)
    .run()
    .await
}
