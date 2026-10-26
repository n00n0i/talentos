//! Tenant-aware Postgres pool. Sole job: guarantee every tenant-scoped query runs
//! with `app.current_tenant_id` set, so RLS cannot leak rows across tenants.

use futures_util::future::BoxFuture;
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::{Postgres, Transaction};
use talentos_kernel::TenantContext;

pub struct TenantPool {
    inner: PgPool,
}

impl TenantPool {
    pub async fn connect(url: &str, max_conns: u32) -> Result<Self, sqlx::Error> {
        let inner = PgPoolOptions::new()
            .max_connections(max_conns)
            .connect(url)
            .await?;
        Ok(Self { inner })
    }

    pub fn pool(&self) -> &PgPool {
        &self.inner
    }

    /// Raw connection WITHOUT the tenant GUC.
    ///
    /// BYPASSRLS-only: use exclusively for the login / refresh-token bootstrap lookup,
    /// where the tenant is not yet known and the query must filter explicitly by
    /// (tenant_slug_or_id, email). Never call this for any other purpose — it skips RLS.
    pub async fn acquire_raw(&self) -> Result<sqlx::pool::PoolConnection<Postgres>, sqlx::Error> {
        self.inner.acquire().await
    }

    /// Begin a transaction scoped to one tenant.
    ///
    /// `SET LOCAL` reverts at COMMIT/ROLLBACK, so a pooled connection never
    /// carries another tenant's context. Every tenant-scoped query must run
    /// here — RLS is FORCE-enabled, so a missing GUC reads as "no rows".
    pub async fn begin_tenant(
        &self,
        tenant_id: talentos_kernel::TenantId,
    ) -> Result<Transaction<'static, Postgres>, sqlx::Error> {
        let mut tx = self.inner.begin().await?;
        sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
            .await?;
        Ok(tx)
    }

    /// Run `f` inside a transaction whose first statement binds the tenant GUC.
    /// SET LOCAL reverts at COMMIT/ROLLBACK, so no leakage to the next borrower even
    /// though the physical connection is pooled.
    pub async fn with_tenant_tx<F, T>(
        &self,
        ctx: &TenantContext,
        f: F,
    ) -> Result<T, talentos_authz::AuthError>
    where
        F: for<'a> FnOnce(
            &mut Transaction<'a, Postgres>,
        ) -> BoxFuture<'a, Result<T, talentos_authz::AuthError>>,
    {
        let mut tx = self.inner.begin().await.map_err(db_err)?;
        // Must be the first statement in the tx — see critical invariant, spec §1.2.
        sqlx::query("SET LOCAL app.current_tenant_id = $1")
            .bind(ctx.tenant_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        let out = f(&mut tx).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(out)
    }

    /// Health probe: SELECT 1 with caller-supplied timeout handled outside.
    pub async fn ping(&self) -> Result<(), sqlx::Error> {
        sqlx::query("SELECT 1")
            .execute(&self.inner)
            .await
            .map(|_| ())
    }
}

fn db_err(e: sqlx::Error) -> talentos_authz::AuthError {
    tracing::error!(error = %e, "database error");
    // B7/M8: DB failures are 500s, never 401s — outages must not masquerade as bad passwords.
    talentos_authz::AuthError::Internal(format!("db: {}", e))
}

pub mod pii;
