//! The Blitz wasm demo: the `blitz-wasm-demo` artifact of the latest
//! successful push-to-main run of Blitz's "Publish Wasm Demo" workflow,
//! unpacked (and gzipped for `ServeDir::precompressed_gzip`) under
//! `<data dir>/wasm-demo/<digest>/` and served at `/demo/files/<digest>/`.

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use flate2::{write::GzEncoder, Compression};

use crate::cache::{Cache, Cached, RefreshOutcome};
use crate::github::{CommitInfo, GithubClient};
use crate::wpt_db::data_dir;

pub static WASM_DEMO_CACHE: Cache<WasmDemoCacheEntry> = Cache::new();

/// URL prefix the unpacked artifacts are served under
pub const WASM_DEMO_URL_PREFIX: &str = "/demo/files";

const WORKFLOW_FILE: &str = "publish-wasm.yml";
const ARTIFACT_NAME: &str = "blitz-wasm-demo";

#[derive(Clone)]
pub struct WasmDemoCacheEntry {
    /// The artifact digest prefix the build is stored and served under
    pub key: String,
    /// URL of the Browser App's `index.html`
    pub index_url: String,
    /// Bytes transferred to load the app (its gzipped wasm and JS)
    pub download_size: u64,
    pub commit_info: Option<CommitInfo>,
}

pub fn wasm_demo_dir() -> PathBuf {
    data_dir().join("wasm-demo")
}

pub async fn load_wasm_demo(
    existing: Option<Arc<Cached<WasmDemoCacheEntry>>>,
) -> RefreshOutcome<WasmDemoCacheEntry> {
    println!("Checking for new wasm demo builds...");

    let Ok(token) = std::env::var("GITHUB_TOKEN") else {
        println!("GITHUB_TOKEN not set: the wasm demo is unavailable");
        return RefreshOutcome::Failed;
    };
    let client = GithubClient::new(Some(&token));

    // `WASM_DEMO_RUN_ID` pins a specific run (e.g. a PR's) for testing
    let run = match std::env::var("WASM_DEMO_RUN_ID") {
        Ok(run_id) => {
            let Ok(run_id) = run_id.parse() else {
                println!("Invalid WASM_DEMO_RUN_ID: {run_id}");
                return RefreshOutcome::Failed;
            };
            client.get_workflow_run(run_id).await
        }
        Err(_) => {
            let runs = client
                .list_successful_runs_of_workflow(WORKFLOW_FILE, "main", "push")
                .await;
            let Some(run) = runs.workflow_runs.into_iter().next() else {
                println!("No successful wasm demo build found");
                return RefreshOutcome::Failed;
            };
            run
        }
    };

    let artifacts = client.list_artifacts_for_workflow(run.id).await;
    let Some(artifact) = artifacts
        .artifacts
        .into_iter()
        .find(|artifact| artifact.name == ARTIFACT_NAME && !artifact.expired)
    else {
        println!("Wasm demo run {} has no {ARTIFACT_NAME} artifact", run.id);
        return RefreshOutcome::Failed;
    };

    let digest = artifact.digest.split(':').next_back().unwrap_or_default();
    let key = digest[..digest.len().min(16)].to_string();
    if key.is_empty() {
        return RefreshOutcome::Failed;
    }
    if existing.is_some_and(|existing| existing.key == key) {
        return RefreshOutcome::Unchanged;
    }

    let demo_dir = wasm_demo_dir();
    let build_dir = demo_dir.join(&key);
    if !build_dir.exists() {
        let zip = client.get_bytes(&artifact.archive_download_url).await;
        let unpack_dir = build_dir.clone();
        let result = tokio::task::spawn_blocking(move || unpack(&zip, &unpack_dir))
            .await
            .unwrap();
        if let Err(err) = result {
            println!("Failed to unpack the wasm demo: {err}");
            return RefreshOutcome::Failed;
        }
    }

    // Remove superseded builds
    if let Ok(entries) = std::fs::read_dir(&demo_dir) {
        for entry in entries.flatten() {
            if entry.path() != build_dir {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }

    if !build_dir.join("index.html").exists() {
        println!("The wasm demo artifact has no index.html");
        return RefreshOutcome::Failed;
    }

    let commit_info = Some(match run.head_commit {
        Some(commit) => CommitInfo {
            sha: commit.id,
            message: Some(commit.message),
            timestamp: commit.timestamp,
        },
        None => CommitInfo {
            sha: run.head_sha,
            message: None,
            timestamp: None,
        },
    });

    println!("Wasm demo build {key} cached.");

    RefreshOutcome::Updated(WasmDemoCacheEntry {
        index_url: format!("{WASM_DEMO_URL_PREFIX}/{key}/index.html"),
        download_size: gzipped_size(&build_dir.join("assets")),
        key,
        commit_info,
    })
}

/// Unpack the artifact zip into `dir`, adding a `.gz` copy of each text and
/// wasm file. Unpacks into a sibling directory first so a partial unpack is
/// never served.
fn unpack(zip: &[u8], dir: &Path) -> std::io::Result<()> {
    let tmp_dir = dir.with_extension("part");
    if tmp_dir.exists() {
        std::fs::remove_dir_all(&tmp_dir)?;
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(zip))?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let Some(relative_path) = file.enclosed_name() else {
            continue;
        };
        let path = tmp_dir.join(relative_path);
        if file.is_dir() {
            std::fs::create_dir_all(&path)?;
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut contents = Vec::with_capacity(file.size() as usize);
        std::io::copy(&mut file, &mut contents)?;
        std::fs::write(&path, &contents)?;

        let compress = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| matches!(ext, "wasm" | "js" | "html" | "css" | "svg"));
        if compress {
            let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
            encoder.write_all(&contents)?;
            let mut gz_path = path.into_os_string();
            gz_path.push(".gz");
            std::fs::write(gz_path, encoder.finish()?)?;
        }
    }
    std::fs::rename(&tmp_dir, dir)
}

/// Total size of the gzipped `.wasm` and `.js` files in a directory
fn gzipped_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.ends_with(".wasm.gz") || name.ends_with(".js.gz")
        })
        .filter_map(|entry| entry.metadata().ok())
        .map(|metadata| metadata.len())
        .sum()
}
