use axum::body::Bytes;
use reqwest::Client;
use serde::{Deserialize, Serialize};

pub struct GithubClient {
    client: Client,
    auth_header: Option<String>,
}

impl GithubClient {
    pub fn new(token: Option<&str>) -> Self {
        Self {
            client: Client::new(),
            auth_header: token.map(|token| format!("Bearer {token}")),
        }
    }

    async fn try_get(&self, url: &str) -> Result<reqwest::Response, reqwest::Error> {
        let mut request = self.client.get(url).header("user-agent", "Blitz website");
        if let Some(auth_header) = &self.auth_header {
            request = request.header("authorization", auth_header);
        }
        request.send().await
    }

    pub async fn get(&self, url: &str) -> reqwest::Response {
        self.try_get(url).await.unwrap()
    }

    pub async fn get_bytes(&self, url: &str) -> Bytes {
        self.get(url).await.bytes().await.unwrap()
    }

    pub async fn get_json<T: for<'de> Deserialize<'de>>(&self, url: &str) -> T {
        self.get(&format!("https://api.github.com{url}"))
            .await
            .json()
            .await
            .unwrap()
    }

    pub async fn commit_info(&self, sha: &str) -> Option<CommitInfo> {
        let response = self
            .try_get(&format!(
                "https://api.github.com/repos/dioxuslabs/blitz/commits/{sha}"
            ))
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let commit: CommitResponse = response.json().await.ok()?;
        Some(CommitInfo {
            sha: sha.to_string(),
            message: Some(commit.commit.message),
            timestamp: commit.commit.committer.date,
        })
    }

    #[allow(dead_code)]
    pub async fn list_artifacts(&self, page: usize) -> ArtifactResponse {
        self.get_json::<ArtifactResponse>(&format!(
            "/repos/dioxuslabs/blitz/actions/artifacts?per_page=100&page={page}"
        ))
        .await
    }

    pub async fn list_successful_workflows(&self) -> ListWorkflowsResponse {
        self.get_json::<ListWorkflowsResponse>(
            "/repos/dioxuslabs/blitz/actions/runs?per_page=100&status=success",
        )
        .await
    }

    #[allow(dead_code)]
    pub async fn list_successful_workflows_raw(&self) -> Bytes {
        self.get_bytes(
            "https://api.github.com/repos/dioxuslabs/blitz/actions/runs?per_page=100&status=success",
        )
        .await
    }

    pub async fn list_artifacts_for_workflow(&self, workflow_id: u64) -> ArtifactResponse {
        self.get_json::<ArtifactResponse>(&format!(
            "/repos/dioxuslabs/blitz/actions/runs/{workflow_id}/artifacts"
        ))
        .await
    }
}

// List Artifacts

#[derive(Debug, Serialize, Deserialize)]
pub struct ArtifactResponse {
    pub total_count: u64,
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Artifact {
    pub archive_download_url: String,
    pub created_at: String,
    pub digest: String,
    pub expired: bool,
    pub expires_at: String,
    pub id: i64,
    pub name: String,
    pub node_id: String,
    pub size_in_bytes: i64,
    pub updated_at: String,
    pub url: String,
    pub workflow_run: ArtifactWorkflowRun,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ArtifactWorkflowRun {
    pub head_branch: String,
    pub head_repository_id: i64,
    pub head_sha: String,
    pub id: i64,
    pub repository_id: i64,
}

// List Workflows

#[derive(Debug, Serialize, Deserialize)]
pub struct ListWorkflowsResponse {
    pub total_count: u64,
    pub workflow_runs: Vec<WorkflowRun>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub id: u64,
    pub name: String,
    pub head_branch: String,
    pub head_sha: String,
    pub head_commit: Option<WorkflowCommit>,
    pub status: String,
    pub conclusion: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkflowCommit {
    pub id: String,
    pub message: String,
    pub timestamp: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CommitInfo {
    pub sha: String,
    pub message: Option<String>,
    pub timestamp: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CommitResponse {
    commit: CommitDetails,
}

#[derive(Debug, Deserialize)]
struct CommitDetails {
    message: String,
    committer: Committer,
}

#[derive(Debug, Deserialize)]
struct Committer {
    date: Option<String>,
}
