pub mod openapi;
pub mod process;
pub mod search;
pub mod types;

use std::sync::Arc;

use axum::{routing::get, Router};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::api::{openapi::ApiDoc, process::process_user, search::search, types::AppState};

pub fn create_router(state: Arc<AppState>) -> Router {
    let api_doc = ApiDoc::openapi();
    Router::new()
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", api_doc))
        .route("/search", get(search))
        .route("/process", get(process_user))
        .with_state(state)
}
