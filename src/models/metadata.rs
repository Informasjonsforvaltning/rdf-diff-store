/*
 * RDF Query Cache
 *
 * API for querying diff-store with cache
 *
 * The version of the OpenAPI document: 0.1.0
 * 
 * Generated by: https://openapi-generator.tech
 */




#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Metadata {
    #[serde(rename = "start_time", skip_serializing_if = "Option::is_none")]
    pub start_time: Option<i64>,
    #[serde(rename = "end_time", skip_serializing_if = "Option::is_none")]
    pub end_time: Option<i64>,
}

impl Metadata {
    pub fn new() -> Metadata {
        Metadata {
            start_time: None,
            end_time: None,
        }
    }
}

