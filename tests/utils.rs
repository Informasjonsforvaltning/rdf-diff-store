use std::{
    env,
    time::{SystemTime, UNIX_EPOCH},
};

use actix_web::web;
use async_trait::async_trait;
use rdf_diff_store::{error::Error, git::ReusableRepoPool, rdf::RdfPrettifier};

use lazy_static::lazy_static;
use reqwest::{header, StatusCode};
lazy_static! {
    pub static ref GITEA_API_PATH: String =
        env::var("GITEA_API_PATH").expect("GITEA_API_PATH not defined");
    pub static ref GIT_REPO_BASE_URL: String =
        env::var("GIT_REPO_BASE_URL").expect("GIT_REPO_BASE_URL not defined");
}

pub struct NoOpPrettifier {}

#[async_trait]
impl RdfPrettifier for NoOpPrettifier {
    fn new() -> Self {
        NoOpPrettifier {}
    }

    async fn prettify(&self, graph: &str) -> Result<String, Error> {
        Ok(graph.to_string())
    }
}

pub async fn create_repo_pool(
    name: &'static str,
    size: u64,
) -> web::Data<async_lock::Mutex<ReusableRepoPool>> {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time err")
        .as_secs()
        - 1;

    let timed_name = format!("{}-{}", time, name);

    create_gitea_repo(&timed_name)
        .await
        .expect("unable to create gitea repo");

    web::Data::new(async_lock::Mutex::new(
        ReusableRepoPool::new(
            format!("{}/gitea/{}.git", GIT_REPO_BASE_URL.clone(), timed_name),
            format!("./tmp-repos/{}", timed_name),
            size,
        )
        .expect("unable to create repo pool"),
    ))
}

async fn create_gitea_repo(name: &String) -> Result<(), Error> {
    let response = reqwest::Client::new()
        .post(format!("{}/v1/user/repos", GITEA_API_PATH.clone()))
        .header(header::CONTENT_TYPE, mime::APPLICATION_JSON.to_string())
        .body(format!(
            "{{\"name\": \"{}\", \"default_branch\": \"main\", \"auto_init\": false}}",
            name
        ))
        .send()
        .await?;

    match response.status() {
        StatusCode::OK | StatusCode::CREATED => Ok(()),
        _ => Err(format!(
            "unexpected status code creating repo in gitea: {}",
            response.status()
        )
        .into()),
    }
}
