use axum::{
    extract::{Query, State},
    Json,
};
use std::sync::Arc;

use crate::{
    api::types::{AppState, SearchQuery, SearchResult},
    ml::MachineLearning,
};

/// Search through commits using semantic similarity
#[utoipa::path(
    get,
    path = "/search",
    params(
        ("query" = String, Query, description = "The search query to find similar commits")
    ),
    responses(
        (status = 200, description = "List of commits sorted by similarity to the query", body = Vec<SearchResult>),
        (status = 500, description = "Internal server error")
    ),
    tag = "search"
)]
pub async fn search(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SearchQuery>,
) -> Json<Vec<SearchResult>> {
    let commits = state.db.get_all_commits().await.unwrap_or_default();
    let Ok(query_embedding) = state.machine_learning.get_embedding(&query.query).await else {
        return Json(Vec::new());
    };

    let mut results: Vec<_> = commits
        .into_iter()
        .map(|commit| SearchResult {
            similarity: MachineLearning::cosine_similarity(&query_embedding, &commit.embedding),
            commit,
        })
        .collect();

    results.sort_by(|a, b| {
        b.similarity
            .partial_cmp(&a.similarity)
            .expect("Similarity scores should be comparable")
    });

    Json(results)
}
