use std::{
    env, fs, io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use actix_web::web;
use git2::{Commit, Oid, RebaseOptions, Repository, Signature};
use lazy_static::lazy_static;

use crate::{
    error::Error,
    metrics::{REPO_CHEKOUT_TIME, REPO_COMMIT_TIME, REPO_FETCH_TIME, REPO_PUSH_TIME},
    models::metadata::Metadata,
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
    pub static ref GIT_REPO_URL: String = env::var("GIT_REPO_URL").unwrap_or_else(|e| {
        tracing::error!(error = e.to_string().as_str(), "GIT_REPO_URL not found");
        std::process::exit(1)
    });
}

pub struct ReusableRepoPool {
    repos: Vec<Repository>,
}

impl ReusableRepoPool {
    /// Create a pool of repos, each in a subfolder of the given path.
    pub fn new(git_repo_url: String, root_path: String, size: u64) -> Result<Self, Error> {
        let path = format!("{}/0", root_path);
        // Repo might already exist (persistent volume)
        Repository::open(&path).or_else(|_| Repository::clone(&git_repo_url, &path))?;

        // Create n copies of the same repo, no need to clone n-1 more times.
        for i in 1..size {
            let destination = format!("{}/{}", root_path, i);
            if !Path::exists(destination.as_ref()) {
                copy_dir_recursive(&path, destination)?;
            }
        }

        let repos = (0..size)
            .map(|i| {
                let path = format!("{}/{}", root_path, i);
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

/// Metadata.
pub async fn repo_metadata(repo: &Repository) -> Result<Metadata, Error> {
    let commit_time = list_commit_times(repo)?;

    Ok(Metadata {
        start_time: commit_time.first().map(|(time, _)| *time),
        end_time: commit_time.last().map(|(time, _)| *time),
    })
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
            Err(Error::from("Not able to fast-forward repo."))
        }
    } else {
        Ok(false)
    }?;

    let elapsed_millis = start_time.elapsed().as_millis();
    REPO_FETCH_TIME
        .with_label_values(&[&format!("{}", updated)])
        .observe(elapsed_millis as f64 / 1000.0);

    Ok(updated)
}

pub fn list_commit_times(repo: &Repository) -> Result<Vec<(u64, Oid)>, Error> {
    let mut revwalk = repo.revwalk()?;
    revwalk.set_sorting(git2::Sort::TIME | git2::Sort::REVERSE)?;
    revwalk.push_head()?;

    let mut commit_times = Vec::new();

    for oid in revwalk {
        let oid = oid?;
        let commit = repo.find_commit(oid)?;
        let msg = commit.message().unwrap_or("0");
        commit_times.push((timestamp_from_message(msg), oid));
    }

    Ok(commit_times)
}

fn timestamp_from_message(msg: &str) -> u64 {
    let ts_str = msg.split_whitespace()
        .next()
        .unwrap_or("0");

    match ts_str.parse::<u64>() {
        Ok(parsed) => {parsed}
        Err(_) => {0}
    }
}

fn oid_of_timestamp(repo: &Repository, timestamp: u64) -> Option<Oid> {
    let commit_times = list_commit_times(repo).ok()?;

    let res = commit_times.binary_search_by(|(time, _)| time.cmp(&timestamp));

    match res {
        Ok(0) | Err(0) => None,
        Ok(i) | Err(i) => Some(commit_times[i - 1].1)
    }
}

/// Checkout a timestamp. Returns false if no files exists at that point in time.
pub fn checkout_timestamp(repo: &Repository, timestamp: u64) -> Result<bool, Error> {
    let start_time = Instant::now();

    let result = if let Some(oid) = oid_of_timestamp(repo, timestamp) {
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
    } else {
        Ok(false)
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
pub async fn commit_file(repo: &Repository, path: &PathBuf, message: String, timestamp: u64) -> Result<(), Error> {
    let start_time = Instant::now();

    let mut index = repo.index()?;
    index.add_path(path)?;
    index.write()?;

    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;
    let signature = Signature::now("rdf-diff-store", "fellesdatakatalog@digdir.no")?;

    if let Err(_) = repo.head() {
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            message.as_str(),
            &tree,
            &[],
        )?;
    } else {
        let head_target = repo.head().unwrap().target().unwrap();
        let tip = repo.find_commit(head_target).unwrap();
        let upstream = repo.find_annotated_commit(tip.id()).unwrap();

        checkout_timestamp(repo, timestamp).unwrap();

        let mut parents = Vec::new();
        if let Ok(Some(parent)) = repo.head().map(|r| r.target()) {
            parents.push(repo.find_commit(parent)?);
        }

        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            message.as_str(),
            &tree,
            parents.iter().collect::<Vec<&Commit>>().as_slice(),
        ).unwrap();

        let ts_target = repo.head().unwrap().target().unwrap();
        let ts_tip = repo.find_commit(ts_target).unwrap();
        let commit_to_rebase = repo.find_annotated_commit(ts_tip.id()).unwrap();

        let mut opts: RebaseOptions<'_> = Default::default();
        let mut rebase = repo
            .rebase(Some(&commit_to_rebase), Some(&upstream), None, Some(&mut opts))
            .unwrap();

        let mut i = 0;
        while (rebase.len() < 0) & (i < rebase.len()) {
            rebase.commit(None, &signature, None).unwrap();
            rebase.next().unwrap().unwrap();
            i = i + 1;
        }
        rebase.finish(None).unwrap();
    }

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
