use utoipa::OpenApi;

use crate::api::types::{ProcessUserQuery, ProcessUserResponse, SearchQuery, SearchResult};

/// API Documentation
#[derive(OpenApi)]
#[openapi(
    paths(
        crate::api::search::search,
        crate::api::process::process_user
    ),
    components(
        schemas(
            SearchQuery,
            SearchResult,
            ProcessUserQuery,
            ProcessUserResponse
        )
    ),
    tags(
        (name = "search", description = "Search API endpoints"),
        (name = "process", description = "Process GitHub user repositories")
    ),
    info(
        title = "GitHub Research API",
        version = "1.0.0",
        description = "API for searching and analyzing GitHub repositories",
    )
)]
pub struct ApiDoc;
