//! Attendance v2 schema + RLS smoke tests against PostgreSQL.
//!
//! Skips when `DATABASE_URL` is unset so `cargo test` still passes offline.

use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
async fn another_tenant_cannot_see_shift_rows() {
    let Some(url) = std::env::var("DATABASE_URL").ok() else {
        eprintln!("SKIP: DATABASE_URL unset");
        return;
    };

    let pool = sqlx::PgPool::connect(&url).await.expect("connect");

    let tenant_a = Uuid::new_v4();
    let tenant_b = Uuid::new_v4();
    let entity_a = Uuid::new_v4();
    let shift_id = Uuid::new_v4();

    // Seed tenant A with an entity and one work shift.
    {
        let mut tx = pool.begin().await.expect("begin");
        sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
            .bind(tenant_a.to_string())
            .execute(&mut *tx)
            .await
            .expect("set tenant");
        sqlx::query("INSERT INTO tenants (id, name, slug) VALUES ($1, $2, $3)")
            .bind(tenant_a)
            .bind("tenant-a")
            .bind(format!("a-{}", &tenant_a.to_string()[..8]))
            .execute(&mut *tx)
            .await
            .expect("tenant");
        sqlx::query("INSERT INTO entities (id, tenant_id, name) VALUES ($1, $2, 'Main')")
            .bind(entity_a)
            .bind(tenant_a)
            .execute(&mut *tx)
            .await
            .expect("entity");
        sqlx::query(
            "INSERT INTO work_shifts (id, tenant_id, entity_id, name, start_time, end_time)
             VALUES ($1, $2, $3, 'morning', '08:00', '17:00')",
        )
        .bind(shift_id)
        .bind(tenant_a)
        .bind(entity_a)
        .execute(&mut *tx)
        .await
        .expect("shift");
        tx.commit().await.expect("commit");
    }

    // Seed tenant B with no shifts.
    {
        let mut tx = pool.begin().await.expect("begin");
        sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
            .bind(tenant_b.to_string())
            .execute(&mut *tx)
            .await
            .expect("set tenant");
        sqlx::query("INSERT INTO tenants (id, name, slug) VALUES ($1, $2, $3)")
            .bind(tenant_b)
            .bind("tenant-b")
            .bind(format!("b-{}", &tenant_b.to_string()[..8]))
            .execute(&mut *tx)
            .await
            .expect("tenant");
        tx.commit().await.expect("commit");
    }

    async fn count_shifts(pool: &sqlx::PgPool, tenant_id: Uuid) -> i64 {
        let mut tx = pool.begin().await.expect("begin");
        sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
            .await
            .expect("set tenant");
        let row = sqlx::query("SELECT count(*)::int8 FROM work_shifts")
            .fetch_one(&mut *tx)
            .await
            .expect("count");
        tx.commit().await.expect("commit");
        row.get::<i64, _>(0)
    }

    assert_eq!(
        count_shifts(&pool, tenant_a).await,
        1,
        "tenant A must see its own shift"
    );
    assert_eq!(
        count_shifts(&pool, tenant_b).await,
        0,
        "tenant B must not see tenant A's shift"
    );
}
