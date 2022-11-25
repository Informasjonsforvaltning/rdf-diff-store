use std::time::{Duration, SystemTime, UNIX_EPOCH};

use actix_web::web;
use lazy_static::lazy_static;
use rdf_diff_store::{
    git::{push_updates, ReusableRepoPool},
    graphs::{read_all_graph_files, store_graph},
    models::Graph,
    rdf::RdfPrettifier,
};
use utils::NoOpPrettifier;

mod utils;

lazy_static! {
    static ref REPO_POOL: web::Data<async_lock::Mutex<ReusableRepoPool>> =
        web::Data::new(async_lock::Mutex::new(
            ReusableRepoPool::new("./tmp-repos".to_string(), 2)
                .expect("unable to create repo pool")
        ));
}

/// Store one graph, then store another, then check that graphs retured for the
/// three timestamps are correct. The three timestamps beeing: before first
/// graph is created, before second is created and after both are created.
#[tokio::test]
async fn test() {
    let push_repo = ReusableRepoPool::pop(&REPO_POOL).await;

    let mut graph = Graph {
        id: "<#/(%¤=:".to_string(),
        graph: r#"
        @prefix si: <https://www.w3schools.com/rdf/> .

        <https://www.w3schools00.com> si:author "Jan Egil Refsnes" ;
            si:title "W3Schools" .
        "#
        .to_string(),
        format: Some("text/turtle".to_string()),
    };

    let pre_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time err")
        .as_secs()
        - 1;

    store_graph(&push_repo, &NoOpPrettifier::new(), &graph)
        .await
        .expect("unable to store graph");
    push_updates(&push_repo).expect("unable to push");

    graph.id = "anotherone".to_string();

    std::thread::sleep(Duration::from_secs(1));

    let mid_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time err")
        .as_secs();

    std::thread::sleep(Duration::from_secs(1));

    store_graph(&push_repo, &NoOpPrettifier::new(), &graph)
        .await
        .expect("unable to store graph");
    push_updates(&push_repo).expect("unable to push");

    let post_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time err")
        .as_secs()
        + 1;

    // Use another repo from the pool to get graphs, to assert that fetch/pull works.
    let pull_repo = ReusableRepoPool::pop(&REPO_POOL).await;

    // The following order (post -> pre -> mid) is chosen to test that the repo
    // is able to move both backwards and forwards in time.

    // There should be 2 graphs when both are created.
    let graphs_post = read_all_graph_files(&pull_repo, post_time)
        .await
        .expect("unable to read graphs");
    assert_eq!(graphs_post.len(), 2);

    // There should be 0 graphs before the first is created.
    let graphs_pre = read_all_graph_files(&pull_repo, pre_time)
        .await
        .expect("unable to read graphs");
    assert_eq!(graphs_pre.len(), 0);

    // There should be 1 graph between first and seconds is created.
    let graphs_mid = read_all_graph_files(&pull_repo, mid_time)
        .await
        .expect("unable to read graphs");
    assert_eq!(graphs_mid.len(), 1);

    ReusableRepoPool::push(&REPO_POOL, pull_repo).await;
    ReusableRepoPool::push(&REPO_POOL, push_repo).await;
}

#[tokio::test]
async fn test_no_diff() {
    let push_repo = ReusableRepoPool::pop(&REPO_POOL).await;

    let graph = Graph {
        id: "duplicate".to_string(),
        graph: r#"
        @prefix si: <https://www.w3schools.com/rdf/> .

        <https://www.w3schools00.com> si:author "Jan Egil Refsnes" ;
            si:title "W3Schools" .
        "#
        .to_string(),
        format: Some("text/turtle".to_string()),
    };

    store_graph(&push_repo, &NoOpPrettifier::new(), &graph)
        .await
        .expect("unable to store graph");

    store_graph(&push_repo, &NoOpPrettifier::new(), &graph)
        .await
        .expect("unable to store graph");

    push_updates(&push_repo).expect("unable to push");

    ReusableRepoPool::push(&REPO_POOL, push_repo).await;

    // FIXME: must currently manually check that only 1 commit is made
}
