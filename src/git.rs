use std::{
    env, fs,
    io::{self, Cursor},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use actix_web::web;
use git2::{Commit, Repository, Signature};
use lazy_static::lazy_static;
use tokio::{
    fs::{remove_file, File},
    io::AsyncWriteExt,
};

use crate::{
    error::Error,
    metrics::{FILE_READ_TIME, REPO_CHEKOUT_TIME, REPO_FETCH_TIME},
    models,
    rdf::pretty_print,
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

pub struct ReusableRepoPool {
    repos: Vec<Repository>,
}

impl ReusableRepoPool {
    pub fn new(root_path: String, size: u64) -> Result<Self, Error> {
        let path = format!("{}/0", root_path);
        Repository::clone(&GIT_REPO_URL.clone(), &path)?;

        // Create n copies of the same repo, no need to clone n more times.
        for i in 1..size {
            copy_dir_recursive(&path, format!("{}/{}", GIT_REPOS_ROOT_PATH.clone(), i))?;
        }

        let repos = (0..size)
            .map(|i| {
                Ok(Repository::open(&format!(
                    "{}/{}",
                    GIT_REPOS_ROOT_PATH.clone(),
                    i
                ))?)
            })
            .collect::<Result<_, git2::Error>>()?;

        Ok(Self { repos })
    }

    /// Get an available repository from pool. Must call push(repo) to put it back when done.
    pub async fn pop(pool: &web::Data<async_lock::Mutex<ReusableRepoPool>>) -> Repository {
        loop {
            if let Some(repo) = pool.lock().await.repos.pop() {
                return repo;
            }
            async_std::task::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Put a repo back in pool.
    pub async fn push(pool: web::Data<async_lock::Mutex<ReusableRepoPool>>, repo: Repository) {
        pool.lock().await.repos.push(repo);
    }
}

/// Store graph.
pub async fn store_graph(
    repo: &Repository,
    http_client: &reqwest::Client,
    graph: &models::Graph,
) -> Result<(), Error> {
    let graph_content = pretty_print(http_client, &graph.graph).await?;

    fetch_updates(&repo)?;

    let valid_graph_filename = base64::encode(&graph.id)
        .replace("/", "_")
        .replace("+", "-");
    let filename = format!("{}.ttl", valid_graph_filename);
    let path = repo
        .path()
        .parent()
        .ok_or::<Error>("invalid repo path".into())?
        .join(Path::new(&filename));

    let mut file = File::create(&path).await?;
    let mut buffer = Cursor::new(graph_content);
    file.write_all_buf(&mut buffer).await?;
    file.shutdown().await?;

    commit_file(
        &repo,
        &Path::new(&filename).into(),
        format!("update: {}", graph.id),
    )
    .await?;
    push_updates(&repo)?;

    Ok(())
}

/// Delete graph.
pub async fn delete_graph(repo: &Repository, id: String) -> Result<(), Error> {
    fetch_updates(&repo)?;

    let valid_graph_filename = base64::encode(&id).replace("/", "_").replace("+", "-");
    let filename = format!("{}.ttl", valid_graph_filename);
    let path = repo.path().join(Path::new(&filename));

    remove_file(&path).await?;
    commit_file(&repo, &path, format!("delete: {}", id)).await?;
    push_updates(&repo)?;

    Ok(())
}

/// Fetch all graphs for a given timestamp.
pub async fn read_all_graph_files(
    repo: &Repository,
    timestamp: u64,
) -> Result<Vec<Vec<u8>>, Error> {
    fetch_updates(&repo)?;

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

/// Fetch updates.
fn fetch_updates(repo: &Repository) -> Result<bool, Error> {
    let start_time = Instant::now();

    repo.find_remote("origin")?.fetch(&["main"], None, None)?;

    let fetch_head = repo.find_reference("FETCH_HEAD")?;

    let fetch_commit = repo.reference_to_annotated_commit(&fetch_head)?;
    let analysis = repo.merge_analysis(&[&fetch_commit])?;

    let refname = "refs/heads/main";
    let result = if analysis.0.is_up_to_date() {
        Ok(false)
    } else if analysis.0.is_fast_forward() {
        let mut reference = repo.find_reference(refname)?;
        reference.set_target(fetch_commit.id(), "Fast-Forward")?;
        repo.set_head(refname)?;
        repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))?;
        Ok(true)
    } else {
        Err("Not able to fast-forward repo.".into())
    };

    let elapsed_millis = start_time.elapsed().as_millis();
    if let Ok(change) = result {
        REPO_FETCH_TIME
            .with_label_values(&[&format!("{}", change)])
            .observe(elapsed_millis as f64 / 1000.0);
    }

    result
}

/// Checkout a timestamp. Returns false if no files exists at that point in time.
fn checkout_timestamp(repo: &Repository, timestamp: u64) -> Result<bool, Error> {
    let start_time = Instant::now();

    let mut revwalk = repo.revwalk()?;
    revwalk.set_sorting(git2::Sort::TIME | git2::Sort::REVERSE)?;
    revwalk.push_head()?;

    let mut commit_times = Vec::new();

    for oid in revwalk {
        let oid = oid?;
        let commit = repo.find_commit(oid)?;
        commit_times.push((commit.time(), oid));
    }

    let ts = timestamp as i64;
    let result = match commit_times.binary_search_by(|(time, _)| time.seconds().cmp(&ts)) {
        Err(0) => Ok(false),
        Ok(i) | Err(i) => {
            let oid = commit_times[i - 1].1;
            let commit = repo.find_commit(oid)?;

            match repo.branch(&oid.to_string(), &commit, false) {
                Ok(_) => {}
                Err(e) => {
                    if e.class() != git2::ErrorClass::Reference
                        && e.code() != git2::ErrorCode::Exists
                    {
                        return Err(e.into());
                    }
                }
            }

            let refname = format!("refs/heads/{}", &oid.to_string());
            let obj = repo.revparse_single(&refname)?;
            repo.checkout_tree(&obj, None)?;
            repo.set_head(&refname)?;
            Ok(true)
        }
    };

    let elapsed_millis = start_time.elapsed().as_millis();
    if let Ok(change) = result {
        REPO_CHEKOUT_TIME
            .with_label_values(&[&format!("{}", change)])
            .observe(elapsed_millis as f64 / 1000.0);
    }

    result
}

/// Commit file.
async fn commit_file(repo: &Repository, path: &PathBuf, message: String) -> Result<(), Error> {
    let tree_id = {
        let mut index = repo.index()?;

        index.add_path(path)?;
        index.write_tree()?
    };

    let tree = repo.find_tree(tree_id)?;

    let mut parents = Vec::new();
    // repo.head() fails when empty repo is cloned and no commits exist.
    if let Ok(Some(parent)) = repo.head().map(|r| r.target()) {
        parents.push(repo.find_commit(parent)?);
    }

    let commiter = Signature::now("rdf-diff-store", "fellesdatakatalog@digdir.no")?;
    repo.commit(
        Some("HEAD"),
        &commiter,
        &commiter,
        message.as_str(),
        &tree,
        parents.iter().collect::<Vec<&Commit>>().as_slice(),
    )?;

    Ok(())
}

/// Push commits.
fn push_updates(repo: &Repository) -> Result<(), Error> {
    repo.find_remote("origin")?
        .push(&["refs/heads/main:refs/heads/main"], None)?;

    Ok(())
}

/// Copy folder recursively.
fn copy_dir_recursive(source: impl AsRef<Path>, destination: impl AsRef<Path>) -> io::Result<()> {
    fs::create_dir_all(&destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target = destination.as_ref().join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(entry.path(), target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
