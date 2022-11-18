#[macro_use]
extern crate serde;

use std::{
    str::from_utf8,
    time::{Duration, Instant},
};

use actix_rt::time::interval;
use actix_web::{
    delete, get, middleware::Logger, post, web, App, HttpRequest, HttpResponse, HttpServer,
    Responder,
};
use lazy_static::lazy_static;
use rdf_diff_store::{
    api::validate_api_key,
    error::Error,
    git::{push_updates, ReusableRepoPool, GIT_REPOS_ROOT_PATH},
    graphs::{delete_graph, store_graph},
    metrics::PROCESSED_REQUESTS,
    metrics::{get_metrics, register_metrics, RESPONSE_TIME},
    models,
    rdf::{APIPrettifier, RdfPrettifier},
};

lazy_static! {
    // Only 1 (basically a lock) to avoid multiple writes and conflicts
    static ref REPO_POOL: web::Data<async_lock::Mutex<ReusableRepoPool>> = web::Data::new(async_lock::Mutex::new(ReusableRepoPool::new(GIT_REPOS_ROOT_PATH.clone(), 1).unwrap_or_else(|e| {
        tracing::error!(error = e.to_string().as_str(), "Unable to create repo pool");
        std::process::exit(1)
    })));
}

#[get("/livez")]
async fn livez() -> Result<impl Responder, Error> {
    Ok("ok")
}

#[get("/readyz")]
async fn readyz() -> Result<impl Responder, Error> {
    Ok("ok")
}

#[get("/metrics")]
async fn metrics_endpoint() -> impl Responder {
    match get_metrics() {
        Ok(metrics) => metrics,
        Err(e) => {
            tracing::error!(error = e.to_string(), "unable to gather metrics");
            "".to_string()
        }
    }
}

#[post("/api/graphs")]
async fn post_api_graphs(
    request: HttpRequest,
    state: web::Data<State>,
    repos: web::Data<async_lock::Mutex<ReusableRepoPool>>,
    body: web::Bytes,
) -> Result<impl Responder, Error> {
    validate_api_key(request)?;

    let start_time = Instant::now();

    let graph: models::Graph = serde_json::from_str(from_utf8(&body)?)?;

    let repo = ReusableRepoPool::pop(&repos).await;
    let result = store_graph(&repo, &state.rdf_prettifier, &graph).await;
    ReusableRepoPool::push(&repos, repo).await;

    let elapsed_millis = start_time.elapsed().as_millis();

    match result {
        Ok(_) => {
            PROCESSED_REQUESTS
                .with_label_values(&["POST", "/api/graphs", "success"])
                .inc();
            RESPONSE_TIME
                .with_label_values(&["POST", "/api/graphs", "0"])
                .observe(elapsed_millis as f64 / 1000.0);
            Ok(HttpResponse::Ok().message_body(""))
        }
        Err(e) => {
            PROCESSED_REQUESTS
                .with_label_values(&["POST", "/api/graphs", "error"])
                .inc();
            Err(e)
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct DeleteGraphQueryParams {
    id: String,
}

#[delete("/api/graphs")]
async fn delete_api_graphs(
    request: HttpRequest,
    repos: web::Data<async_lock::Mutex<ReusableRepoPool>>,
    query: web::Query<DeleteGraphQueryParams>,
) -> Result<impl Responder, Error> {
    validate_api_key(request)?;

    let start_time = Instant::now();
    let query_params = query.into_inner();

    let repo = ReusableRepoPool::pop(&repos).await;
    let result = delete_graph(&repo, query_params.id).await;
    ReusableRepoPool::push(&repos, repo).await;

    let elapsed_millis = start_time.elapsed().as_millis();

    match result {
        Ok(_) => {
            PROCESSED_REQUESTS
                .with_label_values(&["DELETE", "/api/graphs", "success"])
                .inc();
            RESPONSE_TIME
                .with_label_values(&["DELETE", "/api/graphs", "0"])
                .observe(elapsed_millis as f64 / 1000.0);
            Ok(HttpResponse::Ok().message_body(""))
        }
        Err(e) => {
            PROCESSED_REQUESTS
                .with_label_values(&["DELETE", "/api/graphs", "error"])
                .inc();
            Err(e)
        }
    }
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

    let state = State {
        rdf_prettifier: APIPrettifier::new(),
    };

    actix_rt::spawn(async {
        let mut interval = interval(Duration::from_secs(60));
        loop {
            interval.tick().await;

            let repo = ReusableRepoPool::pop(&REPO_POOL).await;
            push_updates(&repo).unwrap_or_else(|e| {
                tracing::error!(error = e.to_string().as_str(), "Unable to push updates");
                std::process::exit(1)
            });
            ReusableRepoPool::push(&REPO_POOL, repo).await;
        }
    });

    HttpServer::new(move || {
        App::new()
            .wrap(
                Logger::default()
                    .exclude("/livez".to_string())
                    .exclude("/readyz".to_string())
                    .exclude("/metrics".to_string())
                    .log_target("http"),
            )
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
