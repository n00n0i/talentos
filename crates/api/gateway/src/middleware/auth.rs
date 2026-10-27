use axum::{
    extract::{Request, State},
    http::header::AUTHORIZATION,
    middleware::Next,
    response::{IntoResponse, Response},
};

use talentos_authz::jwt;
use talentos_kernel::TenantContext;

use crate::dto::responses::ErrorResponse;
use crate::state::AppState;

/// Extract Bearer token → verify → inject TenantContext into request extensions.
/// Skipped for public routes by router nesting; if reached without a valid token, short-circuit 401.
// axum's middleware signature fixes `Result<Response, Response>`; the Err arm is a
// ready-to-send response, so boxing it would only add indirection.
#[allow(clippy::result_large_err)]
pub async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, Response> {
    let header = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| unauthorized_response("token_malformed"))?;

    let token = header
        .strip_prefix("Bearer ")
        .ok_or_else(|| unauthorized_response("token_malformed"))?;

    let ctx: TenantContext =
        jwt::verify_access_token(token, &state.jwt_secret).map_err(auth_error_response)?;

    req.extensions_mut().insert(ctx);
    Ok(next.run(req).await)
}

/// Typed extractor so handlers can't forget auth: if a handler takes `Authed`,
/// the middleware must have populated TenantContext (spec §5).
pub struct Authed(pub TenantContext);

#[async_trait::async_trait]
impl<S: Send + Sync> axum::extract::FromRequestParts<S> for Authed {
    type Rejection = Response;
    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<TenantContext>()
            .cloned()
            .map(Authed)
            .ok_or_else(|| unauthorized_response("token_malformed"))
    }
}

/// RBAC gate for a handler. A valid token only proves *who* the caller is; each
/// handler still has to prove the role may do this. Missing permission is 403,
/// never 401 (the caller is authenticated, just not allowed).
// axum responses are the error value so handlers can `return forbidden`; boxing
// would only add indirection, same trade-off as `auth_middleware` above.
#[allow(clippy::result_large_err)]
pub fn require(ctx: &TenantContext, resource: &str, action: &str) -> Result<(), Response> {
    if talentos_authz::has_permission(&ctx.roles, resource, action) {
        return Ok(());
    }
    Err((
        axum::http::StatusCode::FORBIDDEN,
        axum::Json(ErrorResponse {
            error: "forbidden".into(),
            message: "Insufficient permission".into(),
            details: vec![],
            request_id: uuid::Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response())
}

fn unauthorized_response(code: &str) -> Response {
    (
        axum::http::StatusCode::UNAUTHORIZED,
        axum::Json(ErrorResponse {
            error: code.into(),
            message: "Invalid or missing access token".into(),
            details: vec![],
            request_id: uuid::Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

fn auth_error_response(e: talentos_authz::AuthError) -> Response {
    (
        axum::http::StatusCode::from_u16(e.http_status()).unwrap(),
        axum::Json(ErrorResponse {
            error: e.error_code().into(),
            message: safe_message(&e),
            details: vec![],
            request_id: uuid::Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

/// Never leak internal detail to clients (spec §4.5).
fn safe_message(e: &talentos_authz::AuthError) -> String {
    use talentos_authz::AuthError::*;
    match e {
        InvalidCredentials | AccountDisabled => "Invalid email or password".into(),
        TokenExpired => "Access token expired".into(),
        TokenMalformed => "Invalid access token".into(),
        TokenSignatureInvalid => "Invalid access token".into(),
        InsufficientPermission { .. } => "Insufficient permission".into(),
        RateLimited => "Too many attempts".into(),
        Internal(_) => "An unexpected error occurred".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use talentos_kernel::{TenantId, UserId};

    fn ctx(roles: &[&str]) -> TenantContext {
        TenantContext {
            tenant_id: TenantId(uuid::Uuid::nil()),
            entity_id: None,
            user_id: UserId(uuid::Uuid::nil()),
            roles: roles.iter().map(|r| r.to_string()).collect(),
        }
    }

    #[test]
    fn hr_admin_passes_payroll_and_payslip_gates() {
        assert!(require(&ctx(&["hr_admin"]), "payroll_run", "approve").is_ok());
        assert!(require(&ctx(&["hr_admin"]), "payslip", "view_all").is_ok());
    }

    #[test]
    fn employee_is_refused_all_payslips_and_payroll_runs() {
        assert!(require(&ctx(&["employee"]), "payslip", "view_all").is_err());
        assert!(require(&ctx(&["employee"]), "payroll_run", "calculate").is_err());
        assert!(require(&ctx(&["employee"]), "payslip", "view_self").is_ok());
    }

    #[test]
    fn manager_cannot_approve_payroll_but_can_view_team() {
        assert!(require(&ctx(&["manager"]), "payroll_run", "approve").is_err());
        assert!(require(&ctx(&["manager"]), "leave_request", "view_team").is_ok());
    }

    #[test]
    fn hr_admin_can_manage_attendance_shifts_employee_cannot() {
        assert!(require(&ctx(&["hr_admin"]), "attendance", "shifts").is_ok());
        assert!(require(&ctx(&["payroll_manager"]), "attendance", "shifts").is_ok());
        assert!(require(&ctx(&["employee"]), "attendance", "shifts").is_err());
        assert!(require(&ctx(&["manager"]), "attendance", "shifts").is_err());
    }
}
