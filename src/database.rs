use crate::config::Config;
use color_eyre::eyre::{eyre, Result, WrapErr};
use futures::TryStreamExt;
use mongodb::{
    bson::{doc, Document},
    options::ClientOptions,
    Client, Collection,
};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CommitSummary {
    pub languages: Vec<String>,
    pub frameworks_libraries: Vec<String>,
    pub patterns: Vec<String>,
    pub specialized_knowledge: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ReadmeDocument {
    pub owner: String,
    pub repo: String,
    pub content: String,
    pub cached_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CommitDocument {
    pub sha: String,
    pub message: String,
    pub date: String,
    pub org: String,
    pub repo: String,
    pub patch: String,
    pub summary: CommitSummary,
    pub embedding: Vec<f32>,
}

#[derive(Debug)]
pub struct MongoDb {
    client: Client,
    config: Config,
}

impl MongoDb {
    #[instrument(skip(config))]
    pub async fn new(config: Config) -> Result<Self> {
        let client_options = ClientOptions::parse(&config.mongo_uri)
            .await
            .wrap_err_with(|| format!("Failed to parse MongoDB URI: {}", config.mongo_uri))?;
        let client =
            Client::with_options(client_options).wrap_err("Failed to create MongoDB client")?;

        // Verify connection by pinging the database
        client
            .database("admin")
            .run_command(doc! { "ping": 1 })
            .await
            .wrap_err(
                "Failed to connect to MongoDB - please check your credentials and connection",
            )?;

        Ok(Self { client, config })
    }

    fn get_collection(&self) -> Collection<CommitDocument> {
        self.client
            .database(&self.config.db_name)
            .collection(&self.config.collection_name)
    }

    fn get_readme_collection(&self) -> Collection<ReadmeDocument> {
        self.client
            .database(&self.config.db_name)
            .collection("readmes")
    }

    #[instrument(skip(self, commit))]
    pub async fn insert_commit(&self, commit: CommitDocument) -> Result<()> {
        self.get_collection()
            .insert_one(commit)
            .await
            .wrap_err("Failed to insert commit into MongoDB")?;
        Ok(())
    }

    #[instrument(skip(self))]
    pub async fn commit_exists(&self, sha: &str) -> Result<bool> {
        let filter = doc! { "sha": sha };
        let count = self
            .get_collection()
            .count_documents(filter)
            .await
            .wrap_err_with(|| format!("Failed to count documents for SHA: {}", sha))?;
        Ok(count > 0)
    }

    #[instrument(skip(self))]
    pub async fn get_all_commits(&self) -> Result<Vec<CommitDocument>> {
        self.get_collection()
            .find(doc! {})
            .await
            .wrap_err("Failed to find all commits")?
            .try_collect()
            .await
            .wrap_err("Failed to collect commits")
    }

    #[instrument(skip(self))]
    pub async fn get_cached_embedding(
        &self,
        model: &str,
        input_text: &str,
    ) -> Result<Option<Vec<f32>>> {
        let embeddings_collection: Collection<Document> = self
            .client
            .database(&self.config.db_name)
            .collection("embeddings");
        let filter = doc! {
            "model": model,
            "input": input_text
        };

        if let Some(doc) = embeddings_collection
            .find_one(filter)
            .await
            .wrap_err("Failed to find cached embedding")?
        {
            if let Some(embedding) = doc.get("embedding") {
                return Ok(Some(
                    embedding
                        .as_array()
                        .ok_or_else(|| eyre!("Embedding is not an array"))?
                        .iter()
                        .map(|v| {
                            v.as_f64()
                                .ok_or_else(|| eyre!("Embedding value is not a number"))
                                .map(|f| f as f32)
                        })
                        .collect::<Result<Vec<f32>>>()?,
                ));
            }
        }
        Ok(None)
    }

    #[instrument(skip(self, embedding))]
    pub async fn cache_embedding(
        &self,
        model: &str,
        input_text: &str,
        embedding: Vec<f32>,
    ) -> Result<()> {
        let embeddings_collection: Collection<Document> = self
            .client
            .database(&self.config.db_name)
            .collection("embeddings");
        let doc = doc! {
            "model": model,
            "input": input_text,
            "embedding": embedding
        };
        embeddings_collection
            .insert_one(doc)
            .await
            .wrap_err("Failed to cache embedding")?;
        Ok(())
    }

    #[instrument(skip(self))]
    pub async fn get_cached_readme(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<Option<ReadmeDocument>> {
        let filter = doc! {
            "owner": owner,
            "repo": repo
        };

        self.get_readme_collection()
            .find_one(filter)
            .await
            .wrap_err_with(|| format!("Failed to find cached README for {owner}/{repo}"))
    }

    #[instrument(skip(self, readme))]
    pub async fn cache_readme(&self, readme: ReadmeDocument) -> Result<()> {
        let filter = doc! {
            "owner": &readme.owner,
            "repo": &readme.repo
        };

        self.get_readme_collection()
            .replace_one(filter, readme)
            .upsert(true)
            .await
            .wrap_err("Failed to cache README")?;
        Ok(())
    }
}
