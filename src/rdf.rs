use oxigraph::{io::GraphFormat, model::GraphNameRef};

use crate::error::Error;

pub fn to_turtle(store: &oxigraph::store::Store) -> Result<String, Error> {
    let mut buff = Vec::new();
    store.dump_graph(&mut buff, GraphFormat::Turtle, GraphNameRef::DefaultGraph)?;
    let turtle = String::from_utf8(buff)?;
    Ok(turtle)
}
