#[macro_use]
extern crate serde;

use std::{env, time::Instant};

use actix_web::{get, middleware::Logger, web, App, HttpResponse, HttpServer, Responder};
use lazy_static::lazy_static;
use query::QueryCache;

use crate::{
    error::Error,
    metrics::PROCESSED_REQUESTS,
    metrics::{get_metrics, register_metrics, RESPONSE_TIME},
    query::{graphs_with_cache, query_with_cache},
};

mod error;
mod metrics;
#[allow(dead_code, non_snake_case)]
mod models;
mod query;

lazy_static! {
    // static ref API_KEY: String = env::var("API_KEY").unwrap_or_else(|e| {
    //     tracing::error!(error = e.to_string().as_str(), "API_KEY not found");
    //     std::process::exit(1)
    // });
    // static ref DIFF_STORE_API_KEY: String = env::var("DIFF_STORE_API_KEY").unwrap_or_else(|e| {
    //     tracing::error!(error = e.to_string().as_str(), "DIFF_STORE_API_KEY not found");
    //     std::process::exit(1)
    // });
    static ref DIFF_STORE_URL: String = env::var("DIFF_STORE_URL").unwrap_or_else(|e| {
        tracing::error!(error = e.to_string().as_str(), "DIFF_STORE_URL not found");
        std::process::exit(1)
    });
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

// fn validate_api_key(request: HttpRequest) -> Result<(), Error> {
//     let token = request
//         .headers()
//         .get("X-API-KEY")
//         .ok_or(Error::Unauthorized("X-API-KEY header missing".to_string()))?
//         .to_str()
//         .map_err(|_| Error::Unauthorized("invalid api key".to_string()))?;

//     if token == API_KEY.clone() {
//         Ok(())
//     } else {
//         Err(Error::Unauthorized("Incorrect api key".to_string()))
//     }
// }

#[derive(Debug, Deserialize)]
pub struct SparqlQueryParams {
    query: String,
}

#[get("/api/sparql/{timestamp}")]
async fn get_api_sparql(
    //request: HttpRequest,
    path: web::Path<u64>,
    query: web::Query<SparqlQueryParams>,
    state: web::Data<State>,
) -> Result<impl Responder, Error> {
    //validate_api_key(request)?;

    let timestamp = path.into_inner();
    let query_params = query.into_inner();

    let start_time = Instant::now();
    let result = query_with_cache(
        &state.cache,
        &state.http_client,
        timestamp,
        query_params.query,
    )
    .await;
    let elapsed_millis = start_time.elapsed().as_millis();

    match result {
        Ok(ok) => {
            PROCESSED_REQUESTS
                .with_label_values(&["/api/sparql", "success"])
                .inc();
            RESPONSE_TIME
                .with_label_values(&["/api/sparql", &format!("{}", ok.1)])
                .observe(elapsed_millis as f64 / 1000.0);
            Ok(HttpResponse::Ok()
                .content_type(mime::APPLICATION_JSON)
                .message_body(ok.0))
        }
        Err(e) => {
            PROCESSED_REQUESTS
                .with_label_values(&["/api/sparql", "error"])
                .inc();
            Err(e)
        }
    }
}

#[get("/api/graphs/{timestamp}")]
async fn get_api_graphs(
    //request: HttpRequest,
    path: web::Path<u64>,
    state: web::Data<State>,
) -> Result<impl Responder, Error> {
    //validate_api_key(request)?;

    let timestamp = path.into_inner();

    let start_time = Instant::now();
    let result = graphs_with_cache(&state.cache, &state.http_client, timestamp).await;
    let elapsed_millis = start_time.elapsed().as_millis();

    match result {
        Ok(ok) => {
            PROCESSED_REQUESTS
                .with_label_values(&["/api/graphs", "success"])
                .inc();
            RESPONSE_TIME
                .with_label_values(&["/api/graphs", &format!("{}", ok.1)])
                .observe(elapsed_millis as f64 / 1000.0);
            Ok(HttpResponse::Ok()
                .content_type("text/turtle")
                .message_body(ok.0))
        }
        Err(e) => {
            PROCESSED_REQUESTS
                .with_label_values(&["/api/graphs", "error"])
                .inc();
            Err(e)
        }
    }
}

#[derive(Clone)]
struct State {
    cache: QueryCache,
    http_client: reqwest::Client,
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

    // Fail if env vars missing
    // let _ = API_KEY.clone();
    // let _ = DIFF_STORE_API_KEY.clone();

    let state = State {
        cache: QueryCache::new(),
        http_client: reqwest::Client::new(),
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
            .service(livez)
            .service(readyz)
            .service(metrics_endpoint)
            .service(get_api_sparql)
            .service(get_api_graphs)
    })
    .bind(("0.0.0.0", 8080))?
    .workers(4)
    .run()
    .await
}
