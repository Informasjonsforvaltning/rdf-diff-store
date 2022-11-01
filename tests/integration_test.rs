use actix_web::web;
use rdf_diff_store::{
    git::{store_graph, ReusableRepoPool},
    models::Graph,
};

#[tokio::test]
async fn test() {
    let pool = ReusableRepoPool::new("./tmp-repos".to_string(), 2).expect("unable to create repos");
    let pool = web::Data::new(async_lock::Mutex::new(pool));

    let graph = Graph {
        id: "<#/(%¤=:".to_string(),
        graph: r#"
        @prefix si: <https://www.w3schools.com/rdf/> .

        <https://www.w3schools00.com> si:author "Jan Egil Refsnes" ;
            si:title "W3Schools" .
        "#
        .to_string(),
        format: Some("text/turtle".to_string()),
    };

    let repo = ReusableRepoPool::pop(&pool).await;
    store_graph(&repo, &reqwest::Client::new(), graph)
        .await
        .expect("unable to store graph");

    ReusableRepoPool::push(pool, repo).await;
}
