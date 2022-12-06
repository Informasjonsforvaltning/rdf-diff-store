use std::{
    future::{ready, Ready},
    time::Instant,
};

use actix_web::dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform};
use futures_util::future::LocalBoxFuture;
use reqwest::StatusCode;

use super::HTTP_REQUEST_DURATION_SECONDS;

pub const CACHE_LEVEL_HEADER: &str = "Cache-Level";

// https://actix.rs/docs/middleware
pub struct HttpMetrics;

impl<S, B> Transform<S, ServiceRequest> for HttpMetrics
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = actix_web::Error;
    type InitError = ();
    type Transform = HttpMetricsMiddeware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(HttpMetricsMiddeware { service }))
    }
}

pub struct HttpMetricsMiddeware<S> {
    service: S,
}

impl<S, B> Service<ServiceRequest> for HttpMetricsMiddeware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = actix_web::Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let method = req.method().to_string();
        let path = req.uri().path().to_string();

        let fut = self.service.call(req);

        Box::pin(async move {
            let start_time = Instant::now();
            let res = fut.await?;
            let elapsed_time = start_time.elapsed().as_secs_f64();

            let cache_lvl = res
                .headers()
                .get(CACHE_LEVEL_HEADER)
                .and_then(|val| match val.to_str() {
                    Ok(str) => Some(str),
                    Err(e) => {
                        tracing::error!(
                            error = e.to_string(),
                            "unable to convert Cache-Level header to string"
                        );
                        None
                    }
                })
                .unwrap_or_default();

            if path.starts_with("/api") && res.status() != StatusCode::NOT_FOUND {
                HTTP_REQUEST_DURATION_SECONDS
                    .with_label_values(&[
                        &method,
                        &path,
                        &res.status().as_u16().to_string(),
                        cache_lvl,
                    ])
                    .observe(elapsed_time);
            }
            Ok(res)
        })
    }
}
