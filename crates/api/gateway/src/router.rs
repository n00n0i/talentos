use axum::http::{header, Method};
use axum::routing::{get, post};
use axum::Router;
use std::time::Duration;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::request_id::SetRequestIdLayer;
use tower_http::trace::TraceLayer;

use crate::handlers::{
    ai, attendance, audit, auth, employees, graphql, health, leave, mcp, me, memory, payroll,
    plugins, settings, users,
};
use crate::middleware::auth::auth_middleware;
use crate::middleware::logging::log_request;
use crate::state::AppState;

pub fn build(state: AppState) -> Router {
    let public = Router::new()
        .route("/api/v1/auth/login", post(auth::login))
        .route("/api/v1/auth/refresh", post(auth::refresh))
        .route("/api/v1/auth/accept-invite", post(users::accept_invite))
        .route(
            "/api/v1/auth/reset-password",
            post(users::accept_password_reset),
        );

    let protected = Router::new()
        .route("/api/v1/auth/me", get(auth::me))
        .route("/api/v1/auth/2fa/setup", post(auth::setup_2fa))
        .route("/api/v1/auth/2fa/enable", post(auth::enable_2fa))
        .route("/api/v1/auth/2fa/disable", post(auth::disable_2fa))
        .route("/api/v1/me/payslips", get(me::payslips))
        .route("/api/v1/me/payslips/:id/render", get(me::render))
        .route("/api/v1/me/payslips/:id/pdf", get(me::pdf))
        .route("/api/v1/me/attendance", get(me::my_attendance))
        .route("/api/v1/me/attendance/punch", post(me::punch))
        .route("/api/v1/auth/change-password", post(users::change_password))
        .route(
            "/api/v1/employees",
            get(employees::list).post(employees::create),
        )
        .route(
            "/api/v1/employees/:id",
            get(employees::get)
                .put(employees::update)
                .delete(employees::terminate),
        )
        .route("/api/v1/employees/import", post(employees::import))
        .route(
            "/api/v1/settings/payroll",
            get(settings::get_payroll).put(settings::put_payroll),
        )
        .route(
            "/api/v1/settings/organization",
            get(settings::get_organization),
        )
        .route(
            "/api/v1/settings/tenant",
            axum::routing::put(settings::put_tenant),
        )
        .route(
            "/api/v1/settings/entities/:id",
            axum::routing::put(settings::put_entity),
        )
        .route("/api/v1/settings/system", get(settings::system))
        .route("/api/v1/audit", get(audit::list))
        .route("/api/v1/users", get(users::list))
        .route("/api/v1/users/invite", post(users::invite))
        .route(
            "/api/v1/users/:id/reset-password",
            post(users::issue_password_reset),
        )
        .route("/api/v1/users/:id/roles", post(users::assign_role))
        .route("/api/v1/users/:id/2fa/reset", post(users::reset_2fa))
        .route(
            "/api/v1/users/:id",
            axum::routing::delete(users::deactivate),
        )
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
            "/api/v1/payroll/runs/:id/export.csv",
            get(payroll::export_payroll_csv),
        )
        .route(
            "/api/v1/leave/requests",
            get(leave::list).post(leave::create),
        )
        .route(
            "/api/v1/attendance",
            get(attendance::list).post(attendance::upsert),
        )
        .route(
            "/api/v1/attendance/holidays",
            get(attendance::list_holidays).post(attendance::upsert_holiday),
        )
        .route(
            "/api/v1/attendance/holidays/:id",
            axum::routing::delete(attendance::delete_holiday),
        )
        .route(
            "/api/v1/attendance/:id",
            axum::routing::delete(attendance::delete),
        )
        .route("/api/v1/attendance/import", post(attendance::import))
        .route("/api/v1/attendance/days", get(attendance::list_days))
        .route(
            "/api/v1/attendance/days/:id/approve",
            post(attendance::approve_day),
        )
        .route(
            "/api/v1/attendance/days/:id/correct",
            post(attendance::correct_day),
        )
        .route(
            "/api/v1/shifts",
            get(attendance::list_shifts).post(attendance::create_shift),
        )
        .route("/api/v1/shifts/:id/assign", post(attendance::assign_shift))
        .route(
            "/api/v1/settings/entities/:id/geofence",
            axum::routing::put(attendance::set_geofence),
        )
        .route("/api/v1/me/leave", get(leave::me_leave))
        .route("/api/v1/me/leave/:id/cancel", post(leave::me_cancel))
        .route("/api/v1/leave/balances", get(leave::balances))
        .route("/api/v1/leave/requests/:id/decision", post(leave::decide))
        .route("/api/v1/leave/approvals", get(leave::approvals))
        .route("/api/v1/ai/tools", get(ai::list_tools))
        .route("/api/v1/graphql", post(graphql::execute))
        .route(
            "/api/v1/mcp",
            post(mcp::post)
                .get(mcp::method_not_allowed)
                .delete(mcp::session_close),
        )
        .route(
            "/api/v1/ai/memory",
            get(memory::recall)
                .post(memory::remember)
                .delete(memory::purge),
        )
        .route("/api/v1/ai/memory/stats", get(memory::stats))
        .route("/api/v1/plugins", get(plugins::list))
        .route("/api/v1/plugins/:name/invoke", post(plugins::invoke))
        .route("/api/v1/ai/agents/run", post(ai::run_agent))
        .route(
            "/api/v1/payroll/payslips/:id/render",
            get(payroll::render_payslip),
        )
        .route(
            "/api/v1/payroll/payslips/:id/pdf",
            get(payroll::get_payslip_pdf),
        )
        .route(
            "/api/v1/payslip/template",
            get(payroll::get_template).put(payroll::put_template),
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
