use std::collections::HashSet;
use std::{path::PathBuf, sync::Arc};

use crate::cache::{Cache, Cached, RefreshOutcome};
use crate::github::{CommitInfo, GithubClient};
use crate::wpt_db::data_dir;

pub static DOWNLOAD_CACHE: Cache<DownloadCacheEntry> = Cache::new();

#[derive(Clone)]
pub struct DownloadCacheEntry {
    #[allow(dead_code)]
    pub etag: Option<Arc<str>>,
    pub artifacts: Arc<[DownloadLink]>,
    pub commit_info: Option<CommitInfo>,
}

pub struct DownloadLink {
    /// Download URL
    pub url: String,
    /// File name
    pub filename: String,
    /// File size
    pub size_in_bytes: u64,
    /// The OS (window, macOS, etc)
    pub platform: String,
    /// CPU architecture (e.g. aarch64 of x86_64)
    pub arch: String,
    /// The bundle format (DMG, AppImage, etc)
    #[allow(dead_code)]
    pub bundle_format: String,
    /// The artifact zip on disk (under `<data dir>/downloads`)
    pub file_path: PathBuf,
}

/// The on-disk file name for an artifact: its name plus a digest prefix,
/// so a re-run of the same workflow with different content gets a new file.
fn artifact_file_name(name: &str, digest: &str) -> String {
    let digest = digest.split(':').next_back().unwrap_or(digest);
    let digest = &digest[..digest.len().min(16)];
    format!("{name}-{digest}.zip")
}

pub async fn load_downloads(
    _existing: Option<Arc<Cached<DownloadCacheEntry>>>,
) -> RefreshOutcome<DownloadCacheEntry> {
    println!("Checking for new Browser UI builds...");

    // Request latest WPT report (with etag)
    let Ok(token) = std::env::var("GITHUB_TOKEN") else {
        println!("GITHUB_TOKEN not set: browser build downloads are unavailable");
        return RefreshOutcome::Failed;
    };
    let client = GithubClient::new(Some(&token));

    // if let Some(etag) = etag.as_ref() {
    //     builder = builder.header("If-None-Match", &**etag);
    // }

    // let result = client.list_successful_workflows_raw().await;
    // let s = str::from_utf8(&result).unwrap();
    // println!("{}", s);
    let result = client.list_successful_workflows().await;

    // if result.status() == 304 {
    //     println!("WPT results unchanged");
    //     DOWNLOAD_CACHE.mark_as_fresh();
    //     return;
    // }

    // let etag = result
    //     .headers()
    //     .get("etag")
    //     .and_then(|header| header.to_str().ok())
    //     .map(|s| Arc::from(s));

    // println!("New WPT results found. etag: {etag:?}");

    let latest_build_workflow = result.workflow_runs.iter().find(|run| {
        run.name == "Publish Browser"
            && run.status == "completed"
            && run
                .conclusion
                .as_ref()
                .is_some_and(|conclusion| conclusion == "success")
    });

    let Some(latest_build_workflow) = latest_build_workflow else {
        return RefreshOutcome::Failed;
    };

    let workflow_artifact_response = client
        .list_artifacts_for_workflow(latest_build_workflow.id)
        .await;
    // dbg!(&workflow_artifact_response.artifacts);

    let downloads_dir = data_dir().join("downloads");
    if let Err(err) = std::fs::create_dir_all(&downloads_dir) {
        println!("Failed to create downloads directory: {err}");
        return RefreshOutcome::Failed;
    }

    let mut artifacts: Vec<DownloadLink> =
        Vec::with_capacity(workflow_artifact_response.artifacts.len());

    for artifact in workflow_artifact_response.artifacts.into_iter() {
        // "Blitz_0.0.0_aarch64.dmg"

        let (rest, bundle_format) = artifact
            .name
            .rsplit_once('.')
            .expect("Artifact name has extensions");
        let bundle_format = bundle_format.to_string();
        let rest = rest.trim_end_matches("-setup");
        let mut parts = rest.splitn(3, '_').skip(1);
        let _version_str = parts.next().unwrap();
        let arch = parts.next().unwrap().to_string();

        let platform = match bundle_format.as_str() {
            "app" | "dmg" => "macOS",
            "exe" | "msi" => "Windows",
            "deb" | "rpm" | "AppImage" => "Linux",
            "apk" => "Android",
            _ => "Unknown,",
        }
        .to_string();
        println!("{}", &artifact.name);

        let file_path = downloads_dir.join(artifact_file_name(&artifact.name, &artifact.digest));
        if !file_path.exists() {
            let file_content = client.get_bytes(&artifact.archive_download_url).await;
            let tmp_path = file_path.with_extension("zip.part");
            std::fs::write(&tmp_path, &file_content).unwrap();
            std::fs::rename(&tmp_path, &file_path).unwrap();
        }

        artifacts.push(DownloadLink {
            // url: format!(
            //     "https://github.com/DioxusLabs/blitz/actions/runs/{}/artifacts/{}",
            //     latest_build_workflow.id, artifact.id
            // ),
            url: format!(
                "downloads/file?arch={arch}&platform={platform}&bundle_format={bundle_format}"
            ),
            filename: artifact.name,
            size_in_bytes: artifact.size_in_bytes as u64,
            platform,
            arch,
            bundle_format,
            file_path,
        });
    }

    // Remove files from superseded builds
    let keep: HashSet<&std::path::Path> = artifacts.iter().map(|a| a.file_path.as_path()).collect();
    if let Ok(entries) = std::fs::read_dir(&downloads_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !keep.contains(path.as_path()) {
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    artifacts.sort_by(|a, b| {
        a.platform
            .cmp(&b.platform)
            .then_with(|| a.arch.cmp(&b.arch))
    });

    let commit_info = latest_build_workflow
        .head_commit
        .as_ref()
        .map(|commit| CommitInfo {
            sha: commit.id.clone(),
            message: Some(commit.message.clone()),
            timestamp: commit.timestamp.clone(),
        })
        .or_else(|| {
            Some(CommitInfo {
                sha: latest_build_workflow.head_sha.clone(),
                message: None,
                timestamp: None,
            })
        });

    println!("New Browser build links processed and cached.");

    RefreshOutcome::Updated(DownloadCacheEntry {
        etag: None,
        artifacts: Arc::from(artifacts),
        commit_info,
    })
}
