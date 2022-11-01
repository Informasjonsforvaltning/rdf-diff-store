use std::time::{Duration, SystemTime, UNIX_EPOCH};

use actix_web::web;
use rdf_diff_store::{
    git::{read_all_graph_files, store_graph, ReusableRepoPool},
    models::Graph,
};

#[tokio::test]
async fn test() {
    let pool = ReusableRepoPool::new("./tmp-repos".to_string(), 2).expect("unable to create repos");
    let pool = web::Data::new(async_lock::Mutex::new(pool));

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

    let push_repo = ReusableRepoPool::pop(&pool).await;

    let pre_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time err")
        .as_secs()
        - 1;

    store_graph(&push_repo, &reqwest::Client::new(), &graph)
        .await
        .expect("unable to store graph");

    graph.id = "anotherone".to_string();

    std::thread::sleep(Duration::from_secs(1));

    let mid_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time err")
        .as_secs();

    std::thread::sleep(Duration::from_secs(1));

    store_graph(&push_repo, &reqwest::Client::new(), &graph)
        .await
        .expect("unable to store graph");

    let post_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time err")
        .as_secs()
        + 1;

    let pull_repo = ReusableRepoPool::pop(&pool).await;
    let graphs_mid = read_all_graph_files(&pull_repo, mid_time)
        .await
        .expect("unable to read graphs");
    let graphs_post = read_all_graph_files(&pull_repo, post_time)
        .await
        .expect("unable to read graphs");
    let graphs_pre = read_all_graph_files(&pull_repo, pre_time)
        .await
        .expect("unable to read graphs");

    assert_eq!(graphs_pre.len(), 0);
    assert_eq!(graphs_mid.len(), 1);
    assert_eq!(graphs_post.len(), 2);

    ReusableRepoPool::push(pool, push_repo).await;
}
