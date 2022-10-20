use std::{
    fs, io,
    path::Path,
    thread,
    time::{Duration, Instant},
};

use git2::Repository;

use crate::{
    error::Error,
    metrics::{FILE_READ_TIME, REPO_CHEKOUT_TIME, REPO_FETCH_TIME},
    GIT_REPO_URL, REPO_LOCK,
};

pub struct ReusableRepo {}

impl ReusableRepo {
    pub fn get_pool() -> Result<Vec<Repository>, Error> {
        (0..32)
            .map(|i| Ok(Repository::open(&format!("/repo/{}", i))?))
            .collect()
    }

    pub fn create_pool() -> Result<(), Error> {
        let path = "/repo/0";
        Repository::open(&path).or_else(|_| Repository::clone(&GIT_REPO_URL.clone(), &path))?;
        for i in 1..32 {
            copy_dir_all("/repo/0", format!("/repo/{}", i))?;
        }
        Ok(())
    }

    async fn new() -> Repository {
        loop {
            let mut repos = REPO_LOCK.lock().await;
            if let Some(repo) = repos.pop() {
                return repo;
            }
            drop(repos);
            thread::sleep(Duration::from_millis(5));
        }
    }

    async fn recycle(repo: Repository) {
        let mut indexes = REPO_LOCK.lock().await;
        indexes.push(repo);
    }
}

fn sync_repo(repo: &Repository) -> Result<bool, Error> {
    let start_time = Instant::now();

    repo.find_remote("origin")?.fetch(&["master"], None, None)?;

    let fetch_head = repo.find_reference("FETCH_HEAD")?;
    let fetch_commit = repo.reference_to_annotated_commit(&fetch_head)?;
    let analysis = repo.merge_analysis(&[&fetch_commit])?;

    let result = if analysis.0.is_up_to_date() {
        Ok(false)
    } else if analysis.0.is_fast_forward() {
        let refname = format!("refs/heads/master");
        let mut reference = repo.find_reference(&refname)?;
        reference.set_target(fetch_commit.id(), "Fast-Forward")?;
        repo.set_head(&refname)?;
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

pub async fn fetch_graphs(timestamp: u64) -> Result<Vec<Vec<u8>>, Error> {
    let repo = ReusableRepo::new().await;
    sync_repo(&repo)?;
    match checkout_timestamp(&repo, timestamp)? {
        true => {
            let repo_dir = repo.path().parent().ok_or::<Error>("Invalid repo".into())?;
            let files = read_files(repo_dir).await?;
            ReusableRepo::recycle(repo).await;

            Ok(files)
        }
        false => {
            ReusableRepo::recycle(repo).await;

            Ok(Vec::new())
        }
    }
}

async fn read_files(path: &Path) -> Result<Vec<Vec<u8>>, Error> {
    let start_time = Instant::now();

    let mut files = Vec::new();
    for filepath in path.read_dir()? {
        let filepath = filepath?;
        if filepath.file_type()?.is_file() {
            files.push(tokio::fs::read(filepath.path()).await?)
        }
    }

    let elapsed_millis = start_time.elapsed().as_millis();
    FILE_READ_TIME.observe(elapsed_millis as f64 / 1000.0);

    Ok(files)
}

fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> io::Result<()> {
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}
