use std::{env, io::Cursor, path::Path, time::Instant};

use base64::{engine::general_purpose, Engine as _};
use git2::Repository;
use lazy_static::lazy_static;
use tokio::{
    fs::{remove_file, File},
    io::{AsyncReadExt, AsyncWriteExt},
};

use crate::{
    error::Error,
    git::{checkout_timestamp, commit_file},
    metrics::FILE_READ_TIME,
    models,
    rdf::RdfPrettifier,
};

lazy_static! {
    pub static ref GIT_REPOS_ROOT_PATH: String =
        env::var("GIT_REPOS_ROOT_PATH").unwrap_or_else(|e| {
            tracing::error!(
                error = e.to_string().as_str(),
                "GIT_REPOS_ROOT_PATH not found"
            );
            std::process::exit(1)
        });
    static ref GIT_REPO_URL: String = env::var("GIT_REPO_URL").unwrap_or_else(|e| {
        tracing::error!(error = e.to_string().as_str(), "GIT_REPO_URL not found");
        std::process::exit(1)
    });
}

/// Store graph.
pub async fn store_graph<P: RdfPrettifier>(
    repo: &Repository,
    rdf_prettifier: &P,
    graph: &models::Graph,
    timestamp: u64,
) -> Result<(), Error> {
    let graph_content = rdf_prettifier.prettify(&graph.graph).await?;

    let valid_graph_filename = general_purpose::STANDARD.encode(&graph.id)
        .replace("/", "_")
        .replace("+", "-");
    let filename = format!("{}.ttl", valid_graph_filename);
    let path = repo
        .path()
        .parent()
        .ok_or::<Error>("invalid repo path".into())?
        .join(Path::new(&filename));

    let abort_due_to_no_change = if let Ok(mut file) = File::open(&path).await {
        let mut contents = vec![];
        file.read_to_end(&mut contents).await?;
        String::from_utf8(contents)? == graph_content
    } else {
        false
    };
    if abort_due_to_no_change {
        return Ok(());
    }

    let mut file = File::create(&path).await?;
    let mut buffer = Cursor::new(graph_content);
    file.write_all_buf(&mut buffer).await?;
    file.shutdown().await?;

    commit_file(
        &repo,
        &Path::new(&filename).into(),
        format!("{} - update: {}", timestamp, graph.id),
        timestamp,
    )
    .await?;

    // Push ever x seconds instead
    // push_updates(&repo)?;

    Ok(())
}

/// Delete graph.
pub async fn delete_graph(
    repo: &Repository,
    id: String,
    timestamp: u64,
) -> Result<(), Error> {
    let valid_graph_filename = general_purpose::STANDARD.encode(&id).replace("/", "_").replace("+", "-");
    let filename = format!("{}.ttl", valid_graph_filename);
    let path = repo.path().join(Path::new(&filename));

    remove_file(&path).await?;
    commit_file(
        &repo,
        &path,
        format!("{} - delete: {}", timestamp, id),
        timestamp
    ).await?;

    Ok(())
}

/// Fetch all graphs for a given timestamp.
pub async fn read_all_graph_files(
    repo: &Repository,
    timestamp: u64,
) -> Result<Vec<Vec<u8>>, Error> {
    let result = if checkout_timestamp(&repo, timestamp)? {
        let repo_dir = repo
            .path()
            .parent()
            .ok_or::<Error>("invalid repo path".into())?;
        read_all_files(repo_dir).await?
    } else {
        Vec::new()
    };

    Ok(result)
}

/// Read all files in repository's root dir. Not recursively.
async fn read_all_files(path: &Path) -> Result<Vec<Vec<u8>>, Error> {
    let start_time = Instant::now();

    let mut files = Vec::new();
    for filepath in path.read_dir()? {
        let filepath = filepath?;
        if filepath.file_type()?.is_file()
            && filepath
                .file_name()
                .into_string()
                .map_err(|_| Error::String("filename err".to_string()))?
                .ends_with(".ttl")
        {
            files.push(tokio::fs::read(filepath.path()).await?)
        }
    }

    let elapsed_millis = start_time.elapsed().as_millis();
    FILE_READ_TIME.observe(elapsed_millis as f64 / 1000.0);

    Ok(files)
}
