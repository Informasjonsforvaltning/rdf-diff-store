use std::{
    env, fs, io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use actix_web::web;
use git2::{Commit, Repository, Signature};
use lazy_static::lazy_static;

use crate::{
    error::Error,
    metrics::{REPO_CHEKOUT_TIME, REPO_COMMIT_TIME, REPO_FETCH_TIME, REPO_PUSH_TIME},
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
    /// Create a pool of repos, each in a subfolder of the given path.
    pub fn new(root_path: String, size: u64) -> Result<Self, Error> {
        let path = format!("{}/0", root_path);
        // Repo might already exist (persistent volume)
        Repository::open(GIT_REPO_URL.clone())
            .or_else(|_| Repository::clone(&GIT_REPO_URL, &path))?;

        // Create n copies of the same repo, no need to clone n-1 more times.
        for i in 1..size {
            let destination = format!("{}/{}", GIT_REPOS_ROOT_PATH.clone(), i);
            if !Path::exists(destination.as_ref()) {
                copy_dir_recursive(&path, destination)?;
            }
        }

        let repos = (0..size)
            .map(|i| {
                let path = format!("{}/{}", GIT_REPOS_ROOT_PATH.clone(), i);
                Ok(Repository::open(&path)?)
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
    pub async fn push(pool: &web::Data<async_lock::Mutex<ReusableRepoPool>>, repo: Repository) {
        pool.lock().await.repos.push(repo);
    }
}

/// Checkout main branch and fetch updates.
pub fn checkout_main_and_fetch_updates(repo: &Repository) -> Result<bool, Error> {
    let start_time = Instant::now();

    let refname = "refs/heads/main";
    if let Err(e) = repo.head() {
        // Default ref is master, swap to main.
        if e.message() == "reference 'refs/heads/master' not found" {
            repo.set_head(refname)?;
        }
    }

    repo.find_remote("origin")?.fetch(&["main"], None, None)?;

    let updated = if let Ok(fetch_head) = repo.find_reference("FETCH_HEAD") {
        let fetch_commit = repo.reference_to_annotated_commit(&fetch_head)?;
        let analysis = repo.merge_analysis(&[&fetch_commit])?;

        if analysis.0.is_up_to_date() {
            Ok(false)
        } else if analysis.0.is_fast_forward() {
            repo.find_reference(refname)
                .and_then(|mut reference| {
                    reference.set_target(fetch_commit.id(), "Fast-Forward")?;
                    Ok(reference)
                })
                // Reference might not exist when cloned repo is empty.
                .or_else(|_| repo.reference(refname, fetch_commit.id(), true, ""))?;

            repo.set_head(refname)?;
            repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))?;
            Ok(true)
        } else {
            Err("Not able to fast-forward repo.".into())
        }
    } else {
        Ok(false)
    };

    let elapsed_millis = start_time.elapsed().as_millis();
    if let Ok(change) = updated {
        REPO_FETCH_TIME
            .with_label_values(&[&format!("{}", change)])
            .observe(elapsed_millis as f64 / 1000.0);
    }

    updated
}

/// Checkout a timestamp. Returns false if no files exists at that point in time.
pub fn checkout_timestamp(repo: &Repository, timestamp: u64) -> Result<bool, Error> {
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
            repo.set_head(&refname)?;
            repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))?;
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
pub async fn commit_file(repo: &Repository, path: &PathBuf, message: String) -> Result<(), Error> {
    let start_time = Instant::now();

    let mut index = repo.index()?;
    index.add_path(path)?;
    index.write()?;

    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;

    let mut parents = Vec::new();
    // repo.head() fails when empty repo is cloned and no commits exist. In that case, there is no parents.
    if let Ok(Some(parent)) = repo.head().map(|r| r.target()) {
        parents.push(repo.find_commit(parent)?);
    }

    let signature = Signature::now("rdf-diff-store", "fellesdatakatalog@digdir.no")?;
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        message.as_str(),
        &tree,
        parents.iter().collect::<Vec<&Commit>>().as_slice(),
    )?;

    let elapsed_millis = start_time.elapsed().as_millis();
    REPO_COMMIT_TIME.observe(elapsed_millis as f64 / 1000.0);

    Ok(())
}

/// Push commits.
pub fn push_updates(repo: &Repository) -> Result<(), Error> {
    let start_time = Instant::now();

    repo.find_remote("origin")?
        .push(&["refs/heads/main:refs/heads/main"], None)?;

    let elapsed_millis = start_time.elapsed().as_millis();
    REPO_PUSH_TIME.observe(elapsed_millis as f64 / 1000.0);

    Ok(())
}

/// Copy folder recursively.
pub fn copy_dir_recursive(
    source: impl AsRef<Path>,
    destination: impl AsRef<Path>,
) -> io::Result<()> {
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
