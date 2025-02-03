use crate::database::CommitDocument;
use crate::{config::Config, database::MongoDb, github::GitHubClient, ml::MachineLearning};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub struct AppState {
    pub db: MongoDb,
    pub config: Config,
    pub machine_learning: MachineLearning,
    pub github_client: GitHubClient,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SearchQuery {
    pub query: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ProcessUserQuery {
    /// GitHub username to process
    pub user: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SearchResult {
    /// Similarity score between 0 and 1
    pub similarity: f32,
    pub commit: CommitDocument,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ProcessUserResponse {
    /// Total number of commits expected to process
    pub total_expected: i32,
    /// Number of commits actually processed
    pub total_processed: i32,
    /// List of repositories that were processed
    pub repositories: Vec<String>,
}
