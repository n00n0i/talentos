//! Ops commands shipped in the gateway binary:
//!
//! ```text
//! talentos-gateway provision --slug acme --name "ACME Co" \
//!   --email admin@acme.co --entity "ACME Main" [--tax-id 0105...]
//!   # password comes from ADMIN_PASSWORD (>= 12 chars)
//!
//! talentos-gateway pii-encrypt
//! ```
//!
//! Provisioning is deliberately a CLI, not an HTTP surface: it crosses tenant
//! boundaries and must use the owner connection.

use std::collections::HashMap;

use sqlx::{postgres::PgPoolOptions, PgPool, Row};
use talentos_authz::password;
use talentos_db::pii::{is_encrypted, PiiCipher};
use uuid::Uuid;

type CliResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug)]
struct ProvisionArgs {
    slug: String,
    name: String,
    email: String,
    entity: String,
    tax_id: Option<String>,
}

fn parse(raw: &[String]) -> CliResult<ProvisionArgs> {
    let mut flags: HashMap<&str, String> = HashMap::new();
    let mut iter = raw.iter();
    while let Some(key) = iter.next() {
        let Some(name) = key.strip_prefix("--") else {
            return Err(format!("unexpected argument '{key}'").into());
        };
        let value = iter
            .next()
            .ok_or_else(|| format!("--{name} needs a value"))?;
        flags.insert(name, value.clone());
    }

    let take = |name: &str| -> CliResult<String> {
        flags
            .get(name)
            .cloned()
            .ok_or_else(|| format!("--{name} is required").into())
    };

    let slug = take("slug")?;
    if !slug
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        || !(2..=40).contains(&slug.len())
    {
        return Err("--slug must be 2..40 chars of a-z, 0-9, '-'".into());
    }
    let name = take("name")?;
    Ok(ProvisionArgs {
        slug,
        entity: flags
            .get("entity")
            .cloned()
            .unwrap_or_else(|| format!("{name} Main")),
        name,
        email: take("email")?,
        tax_id: flags.get("tax-id").cloned(),
    })
}

async fn admin_pool() -> CliResult<PgPool> {
    // Never fall back to the app role: provisioning/migration must not be
    // constrained (or silently half-applied) by RLS.
    let url = std::env::var("DATABASE_ADMIN_URL")
        .map_err(|_| "DATABASE_ADMIN_URL (owner connection) must be set")?;
    Ok(PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await?)
}

pub async fn provision(raw: &[String]) -> CliResult<()> {
    let args = parse(raw)?;
    let admin_password = std::env::var("ADMIN_PASSWORD")
        .map_err(|_| "ADMIN_PASSWORD must be set (>= 12 characters)")?;
    if admin_password.chars().count() < 12 {
        return Err("ADMIN_PASSWORD must be at least 12 characters".into());
    }
    let pool = admin_pool().await?;
    let ids = provision_in(&pool, &args, &admin_password).await?;
    println!(
        "{}",
        serde_json::json!({
            "tenant_id": ids.0,
            "entity_id": ids.1,
            "user_id": ids.2,
            "slug": args.slug,
            "email": args.email,
            "note": "sign in with the configured password, then rotate it"
        })
    );
    Ok(())
}

async fn provision_in(
    pool: &PgPool,
    args: &ProvisionArgs,
    admin_password: &str,
) -> CliResult<(Uuid, Uuid, Uuid)> {
    let tenant_id = Uuid::new_v4();
    let entity_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();

    let hash = {
        let password = admin_password.to_string();
        tokio::task::spawn_blocking(move || password::hash_password(&password))
            .await
            .map_err(|e| format!("password hashing task failed: {e}"))??
    };

    let mut tx = pool.begin().await?;
    // Works with either the owner or the NOBYPASSRLS app role.
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await?;

    let inserted = sqlx::query("INSERT INTO tenants (id, name, slug) VALUES ($1, $2, $3)")
        .bind(tenant_id)
        .bind(args.name.trim())
        .bind(&args.slug)
        .execute(&mut *tx)
        .await;
    if let Err(e) = inserted {
        if e.as_database_error()
            .and_then(|db| db.code())
            .is_some_and(|c| c == "23505")
        {
            return Err(format!("tenant slug '{}' already exists", args.slug).into());
        }
        return Err(e.into());
    }

    sqlx::query("INSERT INTO entities (id, tenant_id, name, tax_id) VALUES ($1, $2, $3, $4)")
        .bind(entity_id)
        .bind(tenant_id)
        .bind(args.entity.trim())
        .bind(args.tax_id.as_deref())
        .execute(&mut *tx)
        .await?;

    let email = args.email.trim().to_lowercase();
    sqlx::query(
        "INSERT INTO users (id, tenant_id, email, password_hash, full_name_th, is_active)
         VALUES ($1, $2, $3, $4, $5, TRUE)",
    )
    .bind(user_id)
    .bind(tenant_id)
    .bind(&email)
    .bind(&hash)
    .bind("ผู้ดูแลระบบ")
    .execute(&mut *tx)
    .await?;

    let role_id: i32 = sqlx::query_scalar(
        "INSERT INTO roles (tenant_id, name) VALUES ($1, 'hr_admin')
         ON CONFLICT (tenant_id, name) DO UPDATE SET name = EXCLUDED.name
         RETURNING id",
    )
    .bind(tenant_id)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO user_roles (user_id, role_id, entity_id) VALUES ($1, $2, $3)
         ON CONFLICT (user_id, role_id) DO UPDATE SET entity_id = EXCLUDED.entity_id",
    )
    .bind(user_id)
    .bind(role_id)
    .bind(entity_id)
    .execute(&mut *tx)
    .await?;

    let _ = sqlx::query(
        r#"INSERT INTO audit_log
             (tenant_id, actor_type, actor_id, action, resource_type, resource_id, after_state)
           VALUES ($1, 'system', 'cli', 'tenant.provision', 'tenant', $2, $3)"#,
    )
    .bind(tenant_id)
    .bind(tenant_id.to_string())
    .bind(serde_json::json!({ "slug": args.slug, "email": email }))
    .execute(&mut *tx)
    .await;

    tx.commit().await?;
    Ok((tenant_id, entity_id, user_id))
}

/// Apply embedded migrations, then make sure the runtime roles exist and can
/// read/write. Migrations use the owner connection; the gateway itself does not.
pub async fn migrate() -> CliResult<()> {
    let pool = admin_pool().await?;
    sqlx::migrate!("../../../migrations").run(&pool).await?;
    ensure_roles(&pool).await?;
    println!(
        "{}",
        serde_json::json!({ "migrations": "applied", "roles": "ready" })
    );
    Ok(())
}

async fn ensure_roles(pool: &PgPool) -> CliResult<()> {
    let app_password = std::env::var("APP_DB_PASSWORD")
        .map_err(|_| "APP_DB_PASSWORD must be set (password for talentos_app)")?;
    let auth_password = std::env::var("AUTH_DB_PASSWORD")
        .map_err(|_| "AUTH_DB_PASSWORD must be set (password for talentos_auth)")?;
    if app_password.len() < 12 || auth_password.len() < 12 {
        return Err("APP_DB_PASSWORD/AUTH_DB_PASSWORD must be at least 12 characters".into());
    }

    sqlx::raw_sql("CREATE EXTENSION IF NOT EXISTS pgcrypto")
        .execute(pool)
        .await?;

    for (role, password, bypass) in [
        ("talentos_app", &app_password, false),
        ("talentos_auth", &auth_password, true),
    ] {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = $1)")
                .bind(role)
                .fetch_one(pool)
                .await?;
        // Utility statements cannot take bind parameters; quote_literal makes the
        // one interpolated value safe. Existing roles are synced to the env
        // password so a changed .env never drifts away from the database.
        let literal: String = sqlx::query_scalar("SELECT quote_literal($1)")
            .bind(password)
            .fetch_one(pool)
            .await?;
        let bypass_clause = if bypass { "BYPASSRLS" } else { "NOBYPASSRLS" };
        let sql = if exists {
            format!("ALTER ROLE {role} WITH LOGIN PASSWORD {literal} {bypass_clause}")
        } else {
            format!("CREATE ROLE {role} LOGIN PASSWORD {literal} NOSUPERUSER {bypass_clause}")
        };
        sqlx::raw_sql(&sql).execute(pool).await?;
    }

    sqlx::raw_sql(
        "GRANT USAGE ON SCHEMA public TO talentos_app, talentos_auth;
         GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO talentos_app, talentos_auth;
         GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO talentos_app, talentos_auth;
         ALTER DEFAULT PRIVILEGES IN SCHEMA public
             GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO talentos_app, talentos_auth;
         ALTER DEFAULT PRIVILEGES IN SCHEMA public
             GRANT USAGE, SELECT ON SEQUENCES TO talentos_app, talentos_auth;",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// One-off migration for rows written before encryption was enabled.
pub async fn encrypt_existing_pii() -> CliResult<()> {
    let cipher =
        PiiCipher::from_env()?.ok_or("PII_ENCRYPTION_KEY must be set (64 hex characters)")?;
    let pool = admin_pool().await?;
    let rows =
        sqlx::query("SELECT id, national_id, sso_number, bank_account_number FROM employees")
            .fetch_all(&pool)
            .await?;

    let (mut rows_changed, mut fields_changed) = (0u64, 0u64);
    for row in &rows {
        let id: Uuid = row.get("id");
        let national: Option<String> = row.get("national_id");
        let sso: Option<String> = row.get("sso_number");
        let bank: Option<String> = row.get("bank_account_number");

        let encrypt = |value: Option<String>| -> CliResult<Option<String>> {
            match value {
                Some(v) if !v.is_empty() && !is_encrypted(&v) => Ok(Some(cipher.encrypt(&v)?)),
                other => Ok(other),
            }
        };
        let (n, s, b) = (encrypt(national)?, encrypt(sso)?, encrypt(bank)?);
        if n == row.get::<Option<String>, _>("national_id")
            && s == row.get::<Option<String>, _>("sso_number")
            && b == row.get::<Option<String>, _>("bank_account_number")
        {
            continue;
        }
        sqlx::query(
            "UPDATE employees SET national_id = $1, sso_number = $2, bank_account_number = $3,
                    updated_at = now() WHERE id = $4",
        )
        .bind(&n)
        .bind(&s)
        .bind(&b)
        .bind(id)
        .execute(&pool)
        .await?;
        rows_changed += 1;
        fields_changed += [n, s, b].iter().filter(|v| v.is_some()).count() as u64;
    }

    println!(
        "{}",
        serde_json::json!({ "rows_changed": rows_changed, "fields": fields_changed })
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(slug: &str) -> ProvisionArgs {
        ProvisionArgs {
            slug: slug.to_string(),
            name: "CLI Test Co".into(),
            email: format!("{slug}@example.com"),
            entity: "CLI Main".into(),
            tax_id: None,
        }
    }

    #[tokio::test]
    async fn provisions_a_tenant_with_an_hr_admin() {
        let Ok(pool) = admin_pool().await else {
            eprintln!("SKIP: DATABASE_ADMIN_URL/DATABASE_URL unset");
            return;
        };
        let slug = format!("cli-{}", &Uuid::new_v4().to_string()[..8]);
        let (tenant_id, entity_id, user_id) = provision_in(&pool, &args(&slug), "CliPassword123!")
            .await
            .expect("provision");

        let users: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE tenant_id = $1")
            .bind(tenant_id)
            .fetch_one(&pool)
            .await
            .expect("users");
        assert_eq!(users, 1);
        let roles: String = sqlx::query_scalar(
            "SELECT r.name FROM roles r JOIN user_roles ur ON ur.role_id = r.id
              WHERE ur.user_id = $1",
        )
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .expect("role");
        assert_eq!(roles, "hr_admin");
        let entity: Uuid = sqlx::query_scalar("SELECT id FROM entities WHERE tenant_id = $1")
            .bind(tenant_id)
            .fetch_one(&pool)
            .await
            .expect("entity");
        assert_eq!(entity, entity_id);

        // A duplicate slug is rejected, not silently recreated.
        assert!(provision_in(&pool, &args(&slug), "CliPassword123!")
            .await
            .is_err());

        sqlx::query("DELETE FROM tenants WHERE id = $1")
            .bind(tenant_id)
            .execute(&pool)
            .await
            .expect("cleanup");
    }
}
