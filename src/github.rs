use axum::body::Bytes;
use reqwest::Client;
use serde::{Deserialize, Serialize};

pub struct GithubClient {
    client: Client,
    auth_header: String,
}

impl GithubClient {
    pub fn new(token: &str) -> Self {
        Self {
            client: Client::new(),
            auth_header: format!("Bearer {token}"),
        }
    }

    pub async fn get(&self, url: &str) -> reqwest::Response {
        self.client
            .get(url)
            .header("authorization", &self.auth_header)
            .header("user-agent", "Blitz website")
            .send()
            .await
            .unwrap()
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

    #[allow(dead_code)]
    pub async fn list_artifacts(&self, page: usize) -> ArtifactResponse {
        self.get_json::<ArtifactResponse>(&format!(
            "/repos/dioxuslabs/blitz/actions/artifacts?per_page=100&page={page}"
        ))
        .await
    }

    pub async fn list_successful_workflows(&self) -> ListWorkflowsResponse {
        self.get_json::<ListWorkflowsResponse>(&format!(
            "/repos/dioxuslabs/blitz/actions/runs?per_page=100&status=success"
        ))
        .await
    }

    #[allow(dead_code)]
    pub async fn list_successful_workflows_raw(&self) -> Bytes {
        self.get_bytes(&format!(
            "https://api.github.com/repos/dioxuslabs/blitz/actions/runs?per_page=100&status=success"
        ))
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
    pub status: String,
    pub conclusion: Option<String>,
}
