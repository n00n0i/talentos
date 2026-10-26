//! GraphQL endpoint: one POST, tenant context injected from the verified token.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

use crate::middleware::auth::Authed;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct GraphqlBody {
    pub query: String,
    #[serde(default)]
    pub variables: Option<serde_json::Value>,
    #[serde(default)]
    pub operation_name: Option<String>,
}

/// POST /api/v1/graphql
pub async fn execute(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<GraphqlBody>,
) -> Response {
    let gctx = talentos_graphql::GraphqlContext::new(ctx, state.pool.clone());

    let mut request = async_graphql::Request::new(body.query).data(gctx);
    if let Some(vars) = body.variables {
        request = request.variables(async_graphql::Variables::from_json(vars));
    }
    if let Some(name) = body.operation_name {
        request = request.operation_name(name);
    }

    let response = state.graphql_schema.execute(request).await;
    (StatusCode::OK, Json(response)).into_response()
}
