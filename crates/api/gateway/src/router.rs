use axum::http::{header, Method};
use axum::routing::{delete, get, patch, post};
use axum::Router;
use std::time::Duration;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::request_id::SetRequestIdLayer;
use tower_http::trace::TraceLayer;

use crate::handlers::{
    ai, attendance, audit, auth, calendar, claims, delegations, documents, employees, forms, goals,
    graphql, health, leave, loans, mcp, me, memory, notifications, onboarding, ot, payroll,
    performance, pf, plugins, privacy, recruitment, reports, settings, settlement, setup, training,
    users,
};
use crate::middleware::auth::auth_middleware;
use crate::middleware::logging::log_request;
use crate::state::AppState;

pub fn build(state: AppState) -> Router {
    let public = Router::new()
        .route("/api/v1/auth/login", post(auth::login))
        .route("/api/v1/auth/refresh", post(auth::refresh))
        .route("/api/v1/auth/accept-invite", post(users::accept_invite))
        .route("/api/v1/setup", post(setup::setup))
        .route(
            "/api/v1/auth/reset-password",
            post(users::accept_password_reset),
        );

    let protected = Router::new()
        .route("/api/v1/auth/me", get(auth::me))
        .route("/api/v1/auth/2fa/setup", post(auth::setup_2fa))
        .route("/api/v1/auth/2fa/enable", post(auth::enable_2fa))
        .route("/api/v1/auth/2fa/disable", post(auth::disable_2fa))
        .route(
            "/api/v1/auth/2fa/recovery-codes",
            post(auth::regenerate_recovery_codes),
        )
        .route("/api/v1/auth/sessions", get(auth::list_sessions))
        .route(
            "/api/v1/auth/sessions/revoke-all",
            post(auth::revoke_all_sessions),
        )
        .route("/api/v1/auth/sessions/:id", delete(auth::revoke_session))
        .route("/api/v1/me/payslips", get(me::payslips))
        .route("/api/v1/me/payslips/:id/acknowledge", post(me::acknowledge))
        .route("/api/v1/me/payslips/:id/render", get(me::render))
        .route("/api/v1/me/payslips/:id/50tawi", get(me::payslip_50tawi))
        .route("/api/v1/me/payslips/:id/pdf", get(me::pdf))
        .route("/api/v1/me/attendance", get(me::my_attendance))
        .route("/api/v1/me/attendance/punch", post(me::punch))
        .route("/api/v1/me/privacy", get(privacy::status))
        .route("/api/v1/me/privacy/consent", post(privacy::accept))
        .route("/api/v1/me/data-export", get(privacy::export))
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
        .route(
            "/api/v1/employees/:id/final-settlement",
            post(settlement::compute),
        )
        .route(
            "/api/v1/employees/:id/documents",
            get(documents::list).post(documents::create),
        )
        .route(
            "/api/v1/employees/:id/training",
            get(training::list).post(training::create),
        )
        .route("/api/v1/training/expiring", get(training::expiring))
        .route("/api/v1/training/:id", delete(training::remove))
        .route("/api/v1/me/training", get(training::me_list))
        .route(
            "/api/v1/employees/:id/onboarding",
            get(onboarding::list).post(onboarding::create),
        )
        .route(
            "/api/v1/employees/:id/onboarding/standard",
            post(onboarding::standard),
        )
        .route(
            "/api/v1/onboarding/:id",
            patch(onboarding::update).delete(onboarding::remove),
        )
        .route(
            "/api/v1/performance/cycles",
            get(performance::list_cycles).post(performance::create_cycle),
        )
        .route(
            "/api/v1/performance/cycles/:id/start",
            post(performance::start_cycle),
        )
        .route(
            "/api/v1/performance/cycles/:id/close",
            post(performance::close_cycle),
        )
        .route(
            "/api/v1/performance/cycles/:id",
            delete(performance::remove_cycle),
        )
        .route(
            "/api/v1/performance/cycles/:id/reviews",
            get(performance::list_reviews),
        )
        .route(
            "/api/v1/performance/reviews/:id",
            patch(performance::submit_review),
        )
        .route("/api/v1/me/reviews", get(performance::me_reviews))
        .route(
            "/api/v1/me/reviews/:id/acknowledge",
            post(performance::me_acknowledge),
        )
        .route(
            "/api/v1/employees/:id/goals",
            get(goals::list).post(goals::create),
        )
        .route(
            "/api/v1/goals/:id",
            patch(goals::update).delete(goals::remove),
        )
        .route("/api/v1/me/goals", get(goals::me_list))
        .route(
            "/api/v1/jobs",
            get(recruitment::list_jobs).post(recruitment::create_job),
        )
        .route(
            "/api/v1/jobs/:id",
            patch(recruitment::update_job).delete(recruitment::remove_job),
        )
        .route(
            "/api/v1/jobs/:id/candidates",
            get(recruitment::list_candidates).post(recruitment::create_candidate),
        )
        .route(
            "/api/v1/candidates/:id",
            patch(recruitment::update_candidate).delete(recruitment::remove_candidate),
        )
        .route(
            "/api/v1/candidates/:id/offers",
            get(recruitment::list_offers).post(recruitment::create_offer),
        )
        .route(
            "/api/v1/offers/:id",
            patch(recruitment::update_offer).delete(recruitment::remove_offer),
        )
        .route("/api/v1/offers/:id/pdf", get(recruitment::offer_pdf))
        .route(
            "/api/v1/candidates/:id/interviews",
            get(recruitment::list_interviews).post(recruitment::schedule_interview),
        )
        .route(
            "/api/v1/interviews/:id",
            patch(recruitment::update_interview).delete(recruitment::remove_interview),
        )
        .route("/api/v1/candidates/:id/history", get(recruitment::history))
        .route(
            "/api/v1/candidates/:id/cv",
            get(recruitment::download_cv)
                .post(recruitment::upload_cv)
                .layer(axum::extract::DefaultBodyLimit::max(6 * 1024 * 1024)),
        )
        .route("/api/v1/me/onboarding", get(onboarding::me_list))
        .route("/api/v1/me/onboarding/:id", patch(onboarding::me_update))
        .route("/api/v1/documents/expiring", get(documents::expiring))
        .route("/api/v1/documents/:id", delete(documents::remove))
        .route(
            "/api/v1/documents/:id/file",
            get(documents::download)
                .post(documents::upload)
                .layer(axum::extract::DefaultBodyLimit::max(6 * 1024 * 1024)),
        )
        .route("/api/v1/me/documents", get(documents::me_list))
        .route("/api/v1/me/documents/:id/file", get(documents::me_download))
        .route(
            "/api/v1/employees/:id/pf-rates",
            get(employees::list_pf_rates).post(employees::upsert_pf_rate),
        )
        .route("/api/v1/employees/import", post(employees::import))
        .route("/api/v1/final-settlements", get(settlement::list))
        .route("/api/v1/loans", get(loans::list).post(loans::create))
        .route("/api/v1/loans/:id/cancel", post(loans::cancel))
        .route("/api/v1/loans/:id/payoff", post(loans::payoff))
        .route("/api/v1/loans/:id/payments", get(loans::payments))
        .route("/api/v1/loans/:id/installments", get(loans::installments))
        .route(
            "/api/v1/loans/:id/installments/:run_id/reverse",
            post(loans::reverse),
        )
        .route("/api/v1/me/loans", get(loans::me_loans))
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
        .route("/api/v1/notifications", get(notifications::list))
        .route(
            "/api/v1/notifications/:id/retry",
            post(notifications::retry),
        )
        .route("/api/v1/users", get(users::list))
        .route("/api/v1/users/invite", post(users::invite))
        .route(
            "/api/v1/users/:id/reset-password",
            post(users::issue_password_reset),
        )
        .route("/api/v1/users/:id/roles", post(users::assign_role))
        .route("/api/v1/users/:id/2fa/reset", post(users::reset_2fa))
        .route(
            "/api/v1/users/:id/line",
            axum::routing::put(users::set_line_id),
        )
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
            "/api/v1/payroll/runs/:id/anomalies",
            get(payroll::get_payroll_anomalies),
        )
        .route(
            "/api/v1/payroll/runs/:id/anomalies/review",
            post(payroll::review_payroll_anomalies),
        )
        .route(
            "/api/v1/payroll/runs/:id/mark-paid",
            post(payroll::mark_paid),
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
            "/api/v1/payroll/runs/:id/export.gl.csv",
            get(reports::export_gl_csv),
        )
        .route(
            "/api/v1/payroll/runs/:id/pf-remittance.csv",
            get(reports::export_pf_remittance_csv),
        )
        .route(
            "/api/v1/payroll/runs/:id/pf-remittance/send",
            post(pf::send),
        )
        .route(
            "/api/v1/payroll/runs/:id/sso1-10.csv",
            get(forms::export_sso1_10_csv),
        )
        .route(
            "/api/v1/payroll/runs/:id/pnd1.csv",
            get(forms::export_pnd1_csv),
        )
        .route(
            "/api/v1/payroll/runs/:id/bank-payout.csv",
            get(forms::export_bank_payout_csv),
        )
        .route("/api/v1/reports/pnd1a.csv", get(forms::export_pnd1a_csv))
        .route(
            "/api/v1/payroll/payslips/:id/50tawi",
            get(forms::render_50tawi),
        )
        .route(
            "/api/v1/forms/:type",
            get(forms::get_form).put(forms::put_form),
        )
        .route("/api/v1/reports/headcount", get(reports::headcount))
        .route("/api/v1/reports/payroll-cost", get(reports::payroll_cost))
        .route("/api/v1/reports/ot-trend", get(reports::ot_trend))
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
        .route("/api/v1/calendar", get(calendar::list))
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
        .route(
            "/api/v1/leave/balances/import",
            post(leave::import_balances),
        )
        .route("/api/v1/leave/requests/:id/decision", post(leave::decide))
        .route("/api/v1/leave/approvals", get(leave::approvals))
        .route(
            "/api/v1/leave/approval-rules",
            get(leave::approval_rules_list).post(leave::approval_rules_create),
        )
        .route(
            "/api/v1/leave/approval-rules/:id",
            delete(leave::approval_rules_delete),
        )
        .route(
            "/api/v1/approval-delegations",
            get(delegations::list).post(delegations::create),
        )
        .route(
            "/api/v1/approval-delegations/candidates",
            get(delegations::candidates),
        )
        .route(
            "/api/v1/approval-delegations/:id",
            delete(delegations::delete),
        )
        .route("/api/v1/me/ot", get(ot::me_ot).post(ot::create_me_ot))
        .route("/api/v1/me/ot/:id/cancel", post(ot::me_cancel))
        .route("/api/v1/ot/approvals", get(ot::approvals))
        .route("/api/v1/ot/requests/:id/decision", post(ot::decide))
        .route(
            "/api/v1/me/claims",
            get(claims::me_claims).post(claims::create_claim),
        )
        .route("/api/v1/me/claims/:id/cancel", post(claims::me_cancel))
        .route(
            "/api/v1/me/claims/:id/receipt",
            get(claims::me_download_receipt)
                .post(claims::me_upload_receipt)
                .layer(axum::extract::DefaultBodyLimit::max(6 * 1024 * 1024)),
        )
        .route("/api/v1/claims/approvals", get(claims::approvals))
        .route("/api/v1/claims/:id/decision", post(claims::decide))
        .route(
            "/api/v1/claims/:id/receipt",
            get(claims::hr_download_receipt),
        )
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
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::HeaderName::from_static("x-request-id"),
        ])
        .max_age(Duration::from_secs(86400));

    Router::new()
        .route("/health", get(health::health))
        .route("/metrics", get(crate::metrics::endpoint))
        .merge(public)
        .merge(protected)
        .layer(cors)
        .layer(axum::middleware::from_fn(crate::metrics::track))
        .layer(axum::middleware::from_fn(log_request))
        .layer(SetRequestIdLayer::new(
            header::HeaderName::from_static("x-request-id"),
            tower_http::request_id::MakeRequestUuid,
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
