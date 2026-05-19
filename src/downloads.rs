use std::{
    io::Write as _,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
    time::Instant,
};

use tempfile::NamedTempFile;

use crate::github::GithubClient;

pub static DOWNLOAD_CACHE: DownloadCache = DownloadCache::new();

pub struct DownloadCache(Mutex<Option<Arc<DownloadCacheEntry>>>);
impl DownloadCache {
    const fn new() -> Self {
        Self(Mutex::new(None))
    }

    pub fn get_cloned(&self) -> Option<Arc<DownloadCacheEntry>> {
        (*self.0.lock().unwrap()).clone()
    }

    #[allow(dead_code)]
    pub fn get_mut(&self) -> MutexGuard<'_, Option<Arc<DownloadCacheEntry>>> {
        self.0.lock().unwrap()
    }

    pub fn update(&self, etag: Option<Arc<str>>, artifacts: Arc<[DownloadLink]>) {
        let cached_at = Instant::now();
        *self.0.lock().unwrap() = Some(Arc::new(DownloadCacheEntry {
            etag,
            cached_at,
            artifacts,
        }));
    }

    #[allow(dead_code)]
    pub fn mark_as_fresh(&self) {
        let cached_at = Instant::now();
        let mut inner = self.0.lock().unwrap();
        if let Some(entry) = inner.take() {
            *inner = Some(Arc::new(DownloadCacheEntry {
                cached_at,
                etag: entry.etag.clone(),
                artifacts: entry.artifacts.clone(),
            }));
        }
    }
}

pub struct DownloadCacheEntry {
    pub etag: Option<Arc<str>>,
    pub cached_at: Instant,
    pub artifacts: Arc<[DownloadLink]>,
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
    pub _file: NamedTempFile,
    /// The file content
    pub file_path: PathBuf,
}

pub async fn load_downloads(_etag: Option<Arc<str>>) {
    println!("Checking for new Browser UI builds...");

    // Request latest WPT report (with etag)
    let token = std::env::var("GITHUB_TOKEN").expect("GITHUB_TOKEN environment variable not found");
    let client = GithubClient::new(&token);

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
        return;
    };

    let workflow_artifact_response = client
        .list_artifacts_for_workflow(latest_build_workflow.id)
        .await;
    // dbg!(&workflow_artifact_response.artifacts);

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

        let file_content = client.get_bytes(&artifact.archive_download_url).await;
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(&file_content).unwrap();
        let file_path = file.path().to_path_buf();

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
            _file: file,
            file_path,
        });
    }

    artifacts.sort_by(|a, b| {
        a.platform
            .cmp(&b.platform)
            .then_with(|| a.arch.cmp(&b.arch))
    });

    DOWNLOAD_CACHE.update(None, Arc::from(artifacts));

    println!("New Browser build links processed and cached.");
}
