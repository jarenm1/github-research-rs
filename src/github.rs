use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chrono::Utc;
use eyre::{bail, Context, Result};
use reqwest::Client;
use serde::Deserialize;
use tracing::{debug, instrument, warn};

use crate::{
    config::Config,
    database::{MongoDb, ReadmeDocument},
};

#[derive(Debug, Clone)]
pub struct GitHubClient {
    client: Client,
    config: Config,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommitInfo {
    pub oid: String,
    #[serde(rename = "messageHeadline")]
    pub message_headline: String,
    #[serde(rename = "committedDate")]
    pub committed_date: String,
    pub author: CommitAuthor,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommitAuthor {
    pub email: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Repository {
    pub name: String,
    pub owner: String,
    pub default_branch: String,
    pub commit_count: i32,
}

impl GitHubClient {
    pub fn new(config: Config) -> Self {
        let client = Client::builder()
            .build()
            .expect("Failed to create HTTP client");

        Self { client, config }
    }

    pub async fn get_user_id<'a>(&'a self, login: &'a str) -> Result<Option<String>> {
        let query = r"
        query($login: String!) {
          user(login: $login) {
            id
          }
        }
        ";

        let variables = serde_json::json!({
            "login": login
        });

        let response = self.graphql_request(query, variables).await?;
        let user_id = response["data"]["user"]["id"].as_str().map(String::from);
        Ok(user_id)
    }

    pub async fn get_commits<'a>(
        &'a self,
        owner: &'a str,
        repo: &'a str,
        branch: Option<&'a str>,
        by_author_id: Option<&'a str>,
    ) -> Result<Vec<CommitInfo>> {
        let branch = branch.unwrap_or(&self.config.default_branch);
        let query = if by_author_id.is_some() {
            include_str!("github/queries/commits_by_author.graphql")
        } else {
            include_str!("github/queries/commits.graphql")
        };

        let mut variables = serde_json::json!({
            "owner": owner,
            "name": repo,
            "branch": branch,
            "first": self.config.commits_per_page
        });

        if let Some(author_id) = by_author_id {
            variables["authorId"] = serde_json::Value::String(author_id.to_string());
        }

        let response = self.graphql_request(query, variables).await?;
        let commits = self.parse_commits_response(&response)?;
        Ok(commits)
    }

    pub async fn get_commit_patch<'a>(
        &'a self,
        owner: &'a str,
        repo: &'a str,
        commit_sha: &'a str,
    ) -> Result<String> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/commits/{}",
            owner, repo, commit_sha
        );

        let response = self
            .client
            .get(&url)
            .header(
                "Authorization",
                format!("Bearer {}", self.config.github_token),
            )
            .header("Accept", "application/vnd.github.v3.diff")
            .header("User-Agent", "github-research-rs")
            .send()
            .await?;

        if response.status().is_server_error() {
            let text = response.text().await?;
            bail!("error {text}")
        }

        // Handle successful response
        let body = response
            .text()
            .await
            .wrap_err_with(|| format!("Failed to read response body for commit {commit_sha}"))?;

        if body.is_empty() {
            warn!(
                "Empty patch for commit {commit_sha} in {owner}/{repo} \
                (this usually means it's an empty commit)"
            );
            Ok(String::new())
        } else {
            Ok(body)
        }
    }

    #[instrument(skip_all)]
    pub async fn get_user_contributed_repos<'a>(
        &'a self,
        username: &'a str,
    ) -> Result<Vec<Repository>> {
        let query = include_str!("github/queries/user_contributed_repos.graphql");
        let variables = serde_json::json!({
            "username": username
        });

        let response = self.graphql_request(query, variables).await?;

        let contributions_array = response["data"]["user"]["contributionsCollection"]
            ["commitContributionsByRepository"]
            .as_array()
            .unwrap_or(&Vec::new())
            .to_owned();

        let mut repos = Vec::new();
        for contribution in contributions_array {
            let repo_obj = &contribution["repository"];
            let commit_count = contribution["contributions"]["totalCount"]
                .as_i64()
                .unwrap_or(0) as i32;

            if commit_count > 0 {
                repos.push(Repository {
                    name: repo_obj["name"].as_str().unwrap_or_default().to_string(),
                    owner: repo_obj["owner"]["login"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    default_branch: repo_obj["defaultBranchRef"]["name"]
                        .as_str()
                        .unwrap_or(&self.config.default_branch)
                        .to_string(),
                    commit_count,
                });
            }
        }

        Ok(repos)
    }

    async fn graphql_request(
        &self,
        query: &str,
        variables: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let body = serde_json::json!({
            "query": query,
            "variables": variables
        });

        let response = self
            .client
            .post(&self.config.github_graphql_api)
            .header(
                "Authorization",
                format!("Bearer {}", self.config.github_token),
            )
            .header("User-Agent", "github-research-rs")
            .json(&body)
            .send()
            .await?;

        // Check if the response was successful
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            eprintln!("GitHub API error: Status: {status}, Body: {text}");
            bail!("GitHub API error: {}", status);
        }

        // Try to parse the response as JSON
        let text = response.text().await?;
        let json: serde_json::Value = serde_json::from_str(&text).inspect_err(|e| {
            eprintln!("Failed to parse JSON response: {text}... {e:?}");
        })?;

        // Check for GraphQL errors
        if let Some(errors) = json.get("errors") {
            eprintln!("GraphQL errors: {errors}");
            eyre::bail!("GraphQL error: {}", errors);
        }

        Ok(json)
    }

    fn parse_commits_response(&self, response: &serde_json::Value) -> Result<Vec<CommitInfo>> {
        let edges = response["data"]["repository"]["ref"]["target"]["history"]["edges"]
            .as_array()
            .unwrap_or(&Vec::new())
            .to_owned();

        let mut commits = Vec::new();
        for edge in edges {
            if let Some(node) = edge.get("node").and_then(|n| n.as_object()) {
                let commit: CommitInfo =
                    serde_json::from_value(serde_json::Value::Object(node.clone()))?;
                commits.push(commit);
            }
        }

        Ok(commits)
    }

    #[instrument(skip(self))]
    pub async fn get_readme<'a>(
        &'a self,
        owner: &'a str,
        repo: &'a str,
        db: &'a MongoDb,
    ) -> Result<Option<String>> {
        // Check cache first
        if let Some(cached) = db.get_cached_readme(owner, repo).await? {
            debug!("Using cached README for {owner}/{repo}");
            return Ok(Some(cached.content));
        }

        let url = format!("https://api.github.com/repos/{owner}/{repo}/readme");

        let response = self
            .client
            .get(&url)
            .header(
                "Authorization",
                format!("Bearer {}", self.config.github_token),
            )
            .header("Accept", "application/vnd.github.v3+json")
            .header("User-Agent", "github-research-rs")
            .send()
            .await?;

        if !response.status().is_success() {
            warn!(
                "Failed to fetch README for {owner}/{repo}: {}",
                response.status()
            );
            return Ok(None);
        }

        let json: serde_json::Value = response.json().await?;
        let Some(content) = json.get("content").and_then(|c| c.as_str()) else {
            warn!("No content field in README response for {owner}/{repo}");
            return Ok(None);
        };

        // Decode base64 content
        let decoded = match BASE64.decode(content.replace('\n', "")) {
            Ok(bytes) => {
                String::from_utf8(bytes).wrap_err("Failed to convert README content to UTF-8")?
            }
            Err(e) => {
                warn!("Failed to decode README content: {e}");
                return Ok(None);
            }
        };

        // Cache the README
        let readme_doc = ReadmeDocument {
            owner: owner.to_string(),
            repo: repo.to_string(),
            content: decoded.clone(),
            cached_at: Utc::now(),
        };
        db.cache_readme(readme_doc).await?;

        Ok(Some(decoded))
    }
}
