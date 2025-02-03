use axum::{
    extract::Query,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use eyre::{Report, WrapErr};
use serde_json;
use std::sync::Arc;
use tracing::{debug, error, info, instrument, warn};

use crate::{
    api::types::{AppState, ProcessUserQuery, ProcessUserResponse},
    database::CommitDocument,
};

/// Maximum size of a patch in bytes that we'll process
const MAX_PATCH_SIZE_BYTES: usize = 50_000;

/// Error type for API operations
#[derive(Debug)]
pub struct AppError(Report);

impl From<Report> for AppError {
    fn from(err: Report) -> Self {
        Self(err)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        error!("Internal error: {:?}", self.0);
        (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error").into_response()
    }
}

type AppResult<T> = Result<T, AppError>;

/// Process a GitHub user's repositories and commits
#[utoipa::path(
    get,
    path = "/process",
    params(
        ("user" = String, Query, description = "GitHub username to process")
    ),
    responses(
        (status = 200, description = "Successfully processed user's repositories", body = ProcessUserResponse),
        (status = 500, description = "Internal server error")
    ),
    tag = "process"
)]
#[instrument(skip(state))]
pub async fn process_user(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ProcessUserQuery>,
) -> AppResult<Json<ProcessUserResponse>> {
    info!("Processing user: {}", query.user);
    let repos = state
        .github_client
        .get_user_contributed_repos(&query.user)
        .await
        .wrap_err_with(|| format!("Failed to get contributed repos for user {}", query.user))?;

    let total_expected: i32 = repos.iter().map(|r| r.commit_count).sum();
    info!(
        "Found {} repositories with {} expected commits",
        repos.len(),
        total_expected
    );
    let mut total_processed = 0;
    let mut repositories = Vec::new();

    // Process each repository
    for repo in repos {
        debug!("Processing repository: {}/{}", repo.owner, repo.name);
        repositories.push(format!("{}/{}", repo.owner, repo.name));

        let author_id = state
            .github_client
            .get_user_id(&query.user)
            .await
            .wrap_err_with(|| format!("Failed to get GitHub user ID for {}", query.user))?
            .ok_or_else(|| eyre::eyre!("No GitHub ID found for user {}", query.user))?;

        let commits = state
            .github_client
            .get_commits(
                &repo.owner,
                &repo.name,
                Some(&repo.default_branch),
                Some(&author_id),
            )
            .await
            .wrap_err_with(|| {
                format!(
                    "Failed to get commits for repository {}/{}",
                    repo.owner, repo.name
                )
            })?;

        if commits.is_empty() {
            debug!("No commits found for {}/{}", repo.owner, repo.name);
            continue;
        }

        debug!(
            "Processing {} commits for {}/{}",
            commits.len(),
            repo.owner,
            repo.name
        );
        total_processed += commits.len() as i32;

        // Process each commit
        for commit in commits {
            debug!("Processing commit: {}", commit.oid);
            // Skip if already processed
            let exists = state
                .db
                .commit_exists(&commit.oid)
                .await
                .wrap_err_with(|| {
                    format!("Failed to check if commit {} exists in DB", commit.oid)
                })?;

            if exists {
                debug!("Commit already processed: {}", commit.oid);
                continue;
            }

            // Get commit patch
            let patch = state
                .github_client
                .get_commit_patch(&repo.owner, &repo.name, &commit.oid)
                .await
                .wrap_err_with(|| {
                    format!(
                        "Failed to get patch for commit {} in {}/{}",
                        commit.oid, repo.owner, repo.name
                    )
                })?;

            // Skip if patch is too large (50KB)
            if patch.len() > MAX_PATCH_SIZE_BYTES {
                warn!(
                    "Skipping large patch for commit {}: {} bytes",
                    commit.oid,
                    patch.len()
                );
                continue;
            }

            // Skip if patch is empty
            if patch.is_empty() {
                warn!("Skipping empty patch for commit {}", commit.oid);
                continue;
            }

            // Get README for additional context if available
            let readme_content = state
                .github_client
                .get_readme(&repo.owner, &repo.name, &state.db)
                .await
                .wrap_err_with(|| {
                    format!(
                        "Failed to get README for repository {}/{}",
                        repo.owner, repo.name
                    )
                })?;

            // Generate README summary if available
            let readme_summary = if let Some(readme) = &readme_content {
                Some(
                    state
                        .machine_learning
                        .summarize_readme(readme)
                        .await
                        .wrap_err_with(|| {
                            format!(
                                "Failed to generate README summary for repository {}/{}",
                                repo.owner, repo.name
                            )
                        })?,
                )
            } else {
                None
            };

            // Combine patch with README summary for context if available
            let text_to_summarize = readme_summary.map_or_else(
                || patch.clone(),
                |readme| {
                    format!("Repository README Summary:\n{readme}\n\nCommit Changes:\n{patch}",)
                },
            );

            // Generate summary first since we'll use it for embedding
            let summary = state
                .machine_learning
                .summarize_text(&text_to_summarize)
                .await
                .wrap_err_with(|| {
                    format!("Failed to generate summary for commit {}", commit.oid)
                })?;

            // Serialize summary to JSON for embedding
            let summary_json = serde_json::to_string(&summary).wrap_err_with(|| {
                format!("Failed to serialize summary for commit {}", commit.oid)
            })?;

            // Generate embedding from the serialized summary
            let embedding = state
                .machine_learning
                .get_embedding(&summary_json)
                .await
                .wrap_err_with(|| {
                    format!("Failed to generate embedding for commit {}", commit.oid)
                })?;

            debug!("Generated embedding and summary for commit: {}", commit.oid);

            // Store in database
            let commit_doc = CommitDocument {
                sha: commit.oid.clone(),
                message: commit.message_headline,
                date: commit.committed_date,
                org: repo.owner.clone(),
                repo: repo.name.clone(),
                patch,
                summary,
                embedding,
            };

            state
                .db
                .insert_commit(commit_doc)
                .await
                .wrap_err_with(|| format!("Failed to insert commit {} into DB", commit.oid))?;

            debug!("Successfully stored commit: {}", commit.oid);
        }
    }

    info!(
        "Completed processing user {}. Processed {}/{} commits",
        query.user, total_processed, total_expected
    );
    Ok(Json(ProcessUserResponse {
        total_expected,
        total_processed,
        repositories,
    }))
}
