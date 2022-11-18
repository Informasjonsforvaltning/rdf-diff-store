use async_trait::async_trait;
use rdf_diff_store::{error::Error, rdf::RdfPrettifier};

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
