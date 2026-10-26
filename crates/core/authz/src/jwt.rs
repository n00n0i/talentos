use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use rand::Rng;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use talents_kernel::{EntityId, TenantContext, TenantId, UserId};

use crate::error::AuthError;

const ISS: &str = "talentos.auth";
const AUD: &str = "talentos.api";
const LEEWAY_SECS: u64 = 30;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub tid: String,
    pub eid: Option<String>,
    pub roles: Vec<String>,
    pub iat: i64,
    pub exp: i64,
    pub nbf: i64,
    pub iss: String,
    pub aud: String,
    pub jti: String,
}

/// Issue an HS256 access token from a verified TenantContext.
pub fn issue_access_token(
    ctx: &TenantContext,
    secret: &[u8],
    ttl_secs: i64,
) -> Result<String, AuthError> {
    let now = Utc::now();
    let claims = Claims {
        sub: ctx.user_id.to_string(),
        tid: ctx.tenant_id.to_string(),
        eid: ctx.entity_id.map(|e| e.to_string()),
        roles: ctx.roles.clone(),
        iat: now.timestamp(),
        exp: (now + Duration::seconds(ttl_secs)).timestamp(),
        nbf: now.timestamp(),
        iss: ISS.into(),
        aud: AUD.into(),
        jti: Uuid::new_v4().to_string(),
    };
    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret),
    )
    .map_err(|_| AuthError::TokenMalformed)
}

/// Verify signature, issuer, audience and time bounds (±30s leeway), then rebuild TenantContext.
pub fn verify_access_token(token: &str, secret: &[u8]) -> Result<TenantContext, AuthError> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_issuer(&[ISS]);
    validation.set_audience(&[AUD]);
    validation.leeway = LEEWAY_SECS;
    // Required custom claims per spec §2 — enforce presence.
    validation.required_spec_claims = ["exp".into()].into_iter().collect();
    validation.validate_exp = true;

    let data = decode::<Claims>(token, &DecodingKey::from_secret(secret), &validation)
        .map_err(|e| match e.kind() {
            jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::TokenExpired,
            jsonwebtoken::errors::ErrorKind::ImmatureSignature => AuthError::TokenMalformed,
            jsonwebtoken::errors::ErrorKind::InvalidSignature
            | jsonwebtoken::errors::ErrorKind::InvalidToken
            | jsonwebtoken::errors::ErrorKind::Base64(_)
            | jsonwebtoken::errors::ErrorKind::Json(_)
            | jsonwebtoken::errors::ErrorKind::Utf8(_) => AuthError::TokenSignatureInvalid,
            _ => AuthError::TokenMalformed,
        })?;

    let c = data.claims;
    let user_id = Uuid::parse_str(&c.sub).map_err(|_| AuthError::TokenMalformed)?;
    let tenant_id = Uuid::parse_str(&c.tid).map_err(|_| AuthError::TokenMalformed)?;
    let entity_id = match &c.eid {
        Some(s) => Some(Uuid::parse_str(s).map_err(|_| AuthError::TokenMalformed)?),
        None => None,
    };

    Ok(TenantContext {
        tenant_id: TenantId(tenant_id),
        entity_id: entity_id.map(EntityId),
        user_id: UserId(user_id),
        roles: c.roles,
    })
}

/// Generate an opaque refresh token: random 256-bit hex (64 chars). Not a JWT.
pub fn generate_refresh_token() -> String {
    let mut buf = [0u8; 32];
    rand::rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx() -> TenantContext {
        TenantContext {
            tenant_id: TenantId(Uuid::new_v4()),
            entity_id: Some(EntityId(Uuid::new_v4())),
            user_id: UserId(Uuid::new_v4()),
            roles: vec!["hr_admin".into()],
        }
    }

    const SECRET: &[u8] = b"unit-test-secret-that-is-long-enough-32bytes";

    #[test]
    fn round_trip_issue_verify() {
        let ctx = test_ctx();
        let token = issue_access_token(&ctx, SECRET, 900).unwrap();
        let got = verify_access_token(&token, SECRET).unwrap();
        assert_eq!(got.tenant_id, ctx.tenant_id);
        assert_eq!(got.user_id, ctx.user_id);
        assert_eq!(got.entity_id, ctx.entity_id);
        assert_eq!(got.roles, ctx.roles);
    }

    #[test]
    fn cross_entity_admin_null_eid_round_trips() {
        let mut ctx = test_ctx();
        ctx.entity_id = None;
        let token = issue_access_token(&ctx, SECRET, 900).unwrap();
        let got = verify_access_token(&token, SECRET).unwrap();
        assert_eq!(got.entity_id, None);
    }

    #[test]
    fn expired_token_rejected() {
        let ctx = test_ctx();
        // Negative TTL → exp in the past, beyond 30s leeway.
        let token = issue_access_token(&ctx, SECRET, -120).unwrap();
        assert!(matches!(
            verify_access_token(&token, SECRET),
            Err(AuthError::TokenExpired)
        ));
    }

    #[test]
    fn tampered_payload_rejected() {
        let ctx = test_ctx();
        let token = issue_access_token(&ctx, SECRET, 900).unwrap();
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
        // Flip one char in the payload segment.
        let mut payload = parts[1].to_string();
        let idx = payload.find('A').or_else(|| payload.find('a')).unwrap_or(0);
        let ch = &payload[idx..idx + 1];
        let replacement = if ch == "A" { "B" } else { "C" };
        payload.replace_range(idx..idx + 1, replacement);
        let tampered = format!("{}.{}.{}", parts[0], payload, parts[2]);
        assert!(verify_access_token(&tampered, SECRET).is_err());
    }

    #[test]
    fn wrong_key_rejected() {
        let ctx = test_ctx();
        let token = issue_access_token(&ctx, SECRET, 900).unwrap();
        let err = verify_access_token(&token, b"a-completely-different-secret-key-32byt").unwrap_err();
        assert!(matches!(err, AuthError::TokenSignatureInvalid));
    }

    #[test]
    fn garbage_string_is_malformed() {
        assert!(verify_access_token("not-a-jwt-at-all", SECRET).is_err());
    }

    #[test]
    fn slightly_future_nbf_accepted_within_leeway() {
        // TC-24: iat/nbf = now + 20s must still validate (leeway is 30s).
        let ctx = test_ctx();
        let now = Utc::now();
        let claims = Claims {
            sub: ctx.user_id.to_string(),
            tid: ctx.tenant_id.to_string(),
            eid: ctx.entity_id.map(|e| e.to_string()),
            roles: ctx.roles.clone(),
            iat: (now + Duration::seconds(20)).timestamp(),
            exp: (now + Duration::seconds(900)).timestamp(),
            nbf: (now + Duration::seconds(20)).timestamp(),
            iss: ISS.into(),
            aud: AUD.into(),
            jti: Uuid::new_v4().to_string(),
        };
        let token = encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(SECRET),
        )
        .unwrap();
        assert!(verify_access_token(&token, SECRET).is_ok());
    }

    #[test]
    fn refresh_token_shape() {
        let t = generate_refresh_token();
        assert_eq!(t.len(), 64);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(t, generate_refresh_token());
    }
}