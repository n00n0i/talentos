use axum::http::{header, Method};
use axum::routing::{get, post};
use axum::Router;
use std::time::Duration;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::request_id::SetRequestIdLayer;
use tower_http::trace::TraceLayer;

use crate::handlers::{auth, health, payroll};
use crate::middleware::auth::auth_middleware;
use crate::middleware::logging::log_request;
use crate::state::AppState;

pub fn build(state: AppState) -> Router {
    let public = Router::new()
        .route("/api/v1/auth/login", post(auth::login))
        .route("/api/v1/auth/refresh", post(auth::refresh));

    let protected = Router::new()
        .route("/api/v1/auth/me", get(auth::me))
        .route("/api/v1/payroll/runs", post(payroll::start_payroll_run))
        .route("/api/v1/payroll/runs/:id", get(payroll::get_payroll_run))
        .route(
            "/api/v1/payroll/runs/:id/approve",
            post(payroll::approve_payroll_run),
        )
        .route(
            "/api/v1/payroll/runs/:id/payslips",
            get(payroll::list_payslips),
        )
        .route(
            "/api/v1/payroll/payslips/:id/pdf",
            get(payroll::get_payslip_pdf),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    // B6: CORS layer driven by AUTH_ALLOWED_ORIGINS (spec §5 step 2).
    let origins: Vec<_> = state
        .allowed_origins
        .iter()
        .map(|o| o.parse::<axum::http::HeaderValue>().unwrap())
        .collect();
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::HeaderName::from_static("x-request-id"),
        ])
        .max_age(Duration::from_secs(86400));

    Router::new()
        .route("/health", get(health::health))
        .merge(public)
        .merge(protected)
        .layer(cors)
        .layer(axum::middleware::from_fn(log_request))
        .layer(SetRequestIdLayer::new(
            header::HeaderName::from_static("x-request-id"),
            tower_http::request_id::MakeRequestUuid,
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
