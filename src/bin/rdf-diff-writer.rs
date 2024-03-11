#[macro_use]
extern crate serde;

use std::{str::from_utf8, time::Duration};

use actix_rt::time::interval;
use actix_web::{
    delete, get, middleware::Logger, post, web, App, HttpRequest, HttpResponse, HttpServer,
    Responder,
};
use lazy_static::lazy_static;
use rdf_diff_store::{
    api::{livez, readyz, validate_api_key},
    error::Error,
    git::{
        checkout_main_and_fetch_updates, push_updates, ReusableRepoPool, GIT_REPOS_ROOT_PATH,
        GIT_REPO_URL,
    },
    graphs::{delete_graph, store_graph},
    metrics::{get_metrics, middleware::HttpMetrics, register_metrics},
    models,
    rdf::{APIPrettifier, RdfPrettifier},
};

lazy_static! {
    // Only 1 repo (basically a lock) to avoid conflicting pushes to git storage.
    static ref REPO_POOL: web::Data<async_lock::Mutex<ReusableRepoPool>> = web::Data::new(async_lock::Mutex::new(ReusableRepoPool::new(GIT_REPO_URL.clone(), GIT_REPOS_ROOT_PATH.clone(), 1).unwrap_or_else(|e| {
        tracing::error!(error = e.to_string().as_str(), "unable to create repo pool");
        std::process::exit(1)
    })));
}

#[get("/metrics")]
async fn metrics_endpoint() -> impl Responder {
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
pub struct UpdateGraphQueryParams {
    timestamp: u64,
}

#[post("/api/graphs")]
async fn post_api_graphs(
    request: HttpRequest,
    query: web::Query<UpdateGraphQueryParams>,
    state: web::Data<State>,
    repos: web::Data<async_lock::Mutex<ReusableRepoPool>>,
    body: web::Bytes,
) -> Result<impl Responder, Error> {
    validate_api_key(request)?;

    let query_params = query.into_inner();

    let graph: models::Graph = serde_json::from_str(from_utf8(&body)?)?;

    let repo = ReusableRepoPool::pop(&repos).await;
    checkout_main_and_fetch_updates(&repo)?;

    let result = store_graph(&repo, &state.rdf_prettifier, &graph, query_params.timestamp).await;
    ReusableRepoPool::push(&repos, repo).await;

    // Dont check result before pushing repo back into pool.
    result?;

    Ok(HttpResponse::Ok().message_body(""))
}

#[derive(Debug, Deserialize)]
pub struct DeleteGraphQueryParams {
    id: String,
    timestamp: u64,
}

#[delete("/api/graphs")]
async fn delete_api_graphs(
    request: HttpRequest,
    repos: web::Data<async_lock::Mutex<ReusableRepoPool>>,
    query: web::Query<DeleteGraphQueryParams>,
) -> Result<impl Responder, Error> {
    validate_api_key(request)?;

    let query_params = query.into_inner();

    let repo = ReusableRepoPool::pop(&repos).await;
    checkout_main_and_fetch_updates(&repo)?;
    let result = delete_graph(&repo, query_params.id, query_params.timestamp).await;
    ReusableRepoPool::push(&repos, repo).await;

    // Dont check result before pushing repo back into pool.
    result?;

    Ok(HttpResponse::Ok().message_body(""))
}

#[derive(Clone)]
struct State {
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

    actix_rt::spawn(async {
        // Push repo updates periodically, not related to when commits are made.
        // The time it takes to push does not scale linearly with amout of data.

        let mut interval = interval(Duration::from_secs(60));
        loop {
            interval.tick().await;

            let repo = ReusableRepoPool::pop(&REPO_POOL).await;
            if let Err(e) = push_updates(&repo) {
                tracing::error!(error = e.to_string(), "unable to push updates");
            }
            ReusableRepoPool::push(&REPO_POOL, repo).await;
        }
    });

    let state = State {
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
            .app_data(web::Data::clone(&REPO_POOL))
            .service(livez)
            .service(readyz)
            .service(metrics_endpoint)
            .service(post_api_graphs)
            .service(delete_api_graphs)
    })
    .bind(("0.0.0.0", 8080))?
    .workers(32)
    .run()
    .await
}
