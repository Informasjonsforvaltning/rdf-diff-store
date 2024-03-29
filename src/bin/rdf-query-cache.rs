use actix_web::http::header;
use actix_web::{get, middleware::Logger, web, App, HttpResponse, HttpServer, Responder};
use rdf_diff_store::api::{livez, readyz};
use rdf_diff_store::git::{
    checkout_main_and_fetch_updates, repo_metadata, ReusableRepoPool, GIT_REPOS_ROOT_PATH,
    GIT_REPO_URL,
};
use rdf_diff_store::metrics::middleware::CACHE_LEVEL_HEADER;
use rdf_diff_store::metrics::{middleware::HttpMetrics, CACHE_COUNT};

use rdf_diff_store::rdf::{APIPrettifier, RdfPrettifier};
use rdf_diff_store::{
    error::Error,
    metrics::{get_metrics, register_metrics},
    query::QueryCache,
    query::{graphs_with_cache, query_with_cache},
};
use serde::Deserialize;

#[get("/metrics")]
async fn metrics_endpoint(state: web::Data<State>) -> impl Responder {
    // Update number of items in caches for each metric request.
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
            // Log an error and return no metrics.
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
    // validate_api_key(request)?;

    let timestamp = path.into_inner();
    let query_params = query.into_inner();

    let repo = ReusableRepoPool::pop(&repos).await;
    checkout_main_and_fetch_updates(&repo)?;
    let result = query_with_cache(
        &state.rdf_prettifier,
        &repo,
        &state.cache,
        timestamp,
        query_params.query,
    )
    .await;
    ReusableRepoPool::push(&repos, repo).await;

    // Dont check result before pushing repo back into pool.
    let (body, cache_lvl) = result?;
    Ok(HttpResponse::Ok()
        .insert_header((header::CONTENT_TYPE, mime::APPLICATION_JSON))
        .insert_header((CACHE_LEVEL_HEADER, cache_lvl.to_string()))
        .message_body(body))
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
    checkout_main_and_fetch_updates(&repo)?;
    let result = graphs_with_cache(&state.rdf_prettifier, &repo, &state.cache, timestamp).await;
    ReusableRepoPool::push(&repos, repo).await;

    // Dont check result before pushing repo back into pool.
    let (body, cache_lvl) = result?;
    Ok(HttpResponse::Ok()
        .insert_header((CACHE_LEVEL_HEADER, cache_lvl.to_string()))
        .message_body(body))
}

#[get("/api/metadata")]
async fn get_api_metadata(
    //request: HttpRequest,
    repos: web::Data<async_lock::Mutex<ReusableRepoPool>>,
) -> Result<impl Responder, Error> {
    //validate_api_key(request)?;

    let repo = ReusableRepoPool::pop(&repos).await;
    checkout_main_and_fetch_updates(&repo)?;
    let result = repo_metadata(&repo).await;
    ReusableRepoPool::push(&repos, repo).await;

    // Dont check result before pushing repo back into pool.
    Ok(HttpResponse::Ok().json(result?))
}

#[derive(Clone)]
struct State {
    cache: QueryCache,
    rdf_prettifier: APIPrettifier,
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

    let repo_pool = ReusableRepoPool::new(GIT_REPO_URL.clone(), GIT_REPOS_ROOT_PATH.clone(), 32)
        .unwrap_or_else(|e| {
            tracing::error!(error = e.to_string().as_str(), "unable to create repo pool");
            std::process::exit(1)
        });
    let repo_pool = web::Data::new(async_lock::Mutex::new(repo_pool));

    let state = State {
        cache: QueryCache::new(),
        rdf_prettifier: APIPrettifier::new(),
    };

    HttpServer::new(move || {
        App::new()
            .wrap(
                Logger::default()
                    .exclude("/")
                    .exclude("/livez".to_string())
                    .exclude("/readyz".to_string())
                    .exclude("/metrics".to_string())
                    .log_target("http"),
            )
            .wrap(HttpMetrics)
            .app_data(web::Data::new(state.clone()))
            .app_data(web::Data::clone(&repo_pool))
            .service(livez)
            .service(readyz)
            .service(metrics_endpoint)
            .service(get_api_sparql)
            .service(get_api_graphs)
            .service(get_api_metadata)
    })
    .bind(("0.0.0.0", 8081))?
    .workers(32)
    .run()
    .await
}
