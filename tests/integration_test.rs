use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rdf_diff_store::{
    git::{checkout_main_and_fetch_updates, list_commit_times, push_updates, ReusableRepoPool},
    graphs::{read_all_graph_files, store_graph},
    models::Graph,
    rdf::RdfPrettifier,
};
use utils::{create_repo_pool, NoOpPrettifier};

mod utils;

/// Store one graph, then store another, then check that graphs retured for the
/// three timestamps are correct. The three timestamps beeing: before first
/// graph is created, before second is created and after both are created.
#[tokio::test]
async fn timestamps() {
    let repo_pool = create_repo_pool("timestamps", 2).await;
    let push_repo = ReusableRepoPool::pop(&repo_pool).await;

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

    checkout_main_and_fetch_updates(&push_repo).expect("unable to checkout main and fetch");
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
    let pull_repo = ReusableRepoPool::pop(&repo_pool).await;
    checkout_main_and_fetch_updates(&pull_repo).expect("unable to checkout main and fetch");

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

    ReusableRepoPool::push(&repo_pool, pull_repo).await;
    ReusableRepoPool::push(&repo_pool, push_repo).await;
}

#[tokio::test]
async fn test_no_diff() {
    let repo_pool = create_repo_pool("no-diff", 2).await;
    let push_repo = ReusableRepoPool::pop(&repo_pool).await;

    let graph = Graph {
        id: "duplicate".to_string(),
        graph: r#"
        @prefix si: <https://www.w3schools.com/rdf/> .

        <https://www.w3schools00.com> si:author "Jan Egil B" ;
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

    let commit_times = list_commit_times(&push_repo).expect("unable to list commits");
    assert_eq!(commit_times.len(), 1);

    ReusableRepoPool::push(&repo_pool, push_repo).await;
}
