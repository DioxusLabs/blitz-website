//! A bare clone of a remote git repository kept in the data directory, read
//! directly from its object database with `gix` (no worktree, no git binary).

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

pub type Error = Box<dyn std::error::Error + Send + Sync>;

const REMOTE_HEAD: &str = "refs/remotes/origin/main";

pub struct GitMirror {
    url: String,
    path: PathBuf,
}

impl GitMirror {
    pub fn new(url: String, path: PathBuf) -> Self {
        Self { url, path }
    }

    fn open(&self) -> Result<gix::Repository, Error> {
        Ok(gix::open(&self.path)?)
    }

    /// Clone the repository if it isn't present yet, otherwise fetch from
    /// `origin`. Blocking; run on a blocking thread.
    pub fn sync(&self) -> Result<(), Error> {
        let interrupt = AtomicBool::new(false);
        if self.path.join("HEAD").exists() {
            let repo = self.open()?;
            repo.find_remote("origin")?
                .connect(gix::remote::Direction::Fetch)?
                .prepare_fetch(gix::progress::Discard, Default::default())?
                .receive(gix::progress::Discard, &interrupt)?;
        } else {
            if let Some(parent) = self.path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            gix::prepare_clone_bare(self.url.as_str(), &self.path)?
                .fetch_only(gix::progress::Discard, &interrupt)?;
        }
        Ok(())
    }

    /// A read-only view of the tree at the remote's `main` branch as of the
    /// last sync
    pub fn head(&self) -> Result<Snapshot, Error> {
        let repo = self.open()?;
        let commit = repo
            .find_reference(REMOTE_HEAD)?
            .into_fully_peeled_id()?
            .detach();
        Ok(Snapshot { repo, commit })
    }
}

pub struct Snapshot {
    repo: gix::Repository,
    /// The commit `main` pointed at; changes exactly when the content does
    pub commit: gix::ObjectId,
}

impl Snapshot {
    /// The contents of the file at `path` (relative to the repository root),
    /// or `None` if there is no such file
    pub fn read_file(&self, path: &str) -> Result<Option<Vec<u8>>, Error> {
        let mut tree = self.repo.find_commit(self.commit)?.tree()?;
        let Some(entry) = tree.peel_to_entry_by_path(path)? else {
            return Ok(None);
        };
        if !entry.mode().is_blob() {
            return Ok(None);
        }
        Ok(Some(entry.object()?.detach().data))
    }
}
