//! Memory behaviour against real PostgreSQL (RLS included).
//!
//! Skips when `DATABASE_URL` is unset, so a bare `cargo test` still passes.

use std::sync::Arc;

use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use talentos_memory::{Layer, LayerCount, MemoryStore, NewMemory};
use uuid::Uuid;

async fn store() -> Option<(MemoryStore, TenantId, TenantId)> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let pool = Arc::new(TenantPool::connect(&url, 4).await.expect("connect"));

    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let mut tx = pool.pool().begin().await.expect("begin");
    for (id, label) in [(a, "mem-a"), (b, "mem-b")] {
        sqlx::query("INSERT INTO tenants (id, name, slug) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(label)
            .bind(format!("{label}-{}", &id.to_string()[..8]))
            .execute(&mut *tx)
            .await
            .expect("tenant");
    }
    tx.commit().await.expect("commit");

    Some((MemoryStore::new(pool, None), TenantId(a), TenantId(b)))
}

#[tokio::test]
async fn saves_and_recalls_by_tenant() {
    let Some((store, tenant, other)) = store().await else {
        eprintln!("SKIP: DATABASE_URL unset");
        return;
    };

    let mut m = NewMemory::new(Layer::Semantic, "ค่าประกันสังคมสูงสุด 750 บาทต่อเดือน");
    m.importance = 0.9;
    let id = store.remember(tenant, None, m).await.expect("remember");

    let hits = store
        .recall(tenant, "ประกันสังคม", &Layer::ALL, 5)
        .await
        .expect("recall");
    assert!(
        hits.iter().any(|h| h.id == id),
        "remembered row must come back: {hits:?}"
    );
    assert_eq!(hits[0].layer, Layer::Semantic);
    assert!(hits[0].score > 0.0);

    // RLS: another tenant sees nothing, even with the same query.
    let leaked = store
        .recall(other, "ประกันสังคม", &Layer::ALL, 5)
        .await
        .expect("recall other");
    assert!(leaked.is_empty(), "tenant isolation broken: {leaked:?}");
}

#[tokio::test]
async fn layer_filter_and_expiry_are_enforced() {
    let Some((store, tenant, _)) = store().await else {
        eprintln!("SKIP: DATABASE_URL unset");
        return;
    };

    store
        .remember(
            tenant,
            None,
            NewMemory::new(Layer::Procedural, "ปิดรอบเงินเดือน: ตรวจสลิปแล้วอนุมัติ"),
        )
        .await
        .expect("procedural");
    store
        .remember(
            tenant,
            None,
            NewMemory::new(Layer::Episodic, "รอบเดือนตุลาคมจ่ายช้า 1 วัน"),
        )
        .await
        .expect("episodic");

    let only_episodic = store
        .recall(tenant, "รอบเดือน", &[Layer::Episodic], 10)
        .await
        .expect("recall");
    assert!(only_episodic.iter().all(|h| h.layer == Layer::Episodic));
    assert!(!only_episodic.is_empty());

    // Expire one row directly, then assert it is hidden and purgeable.
    store
        .remember(
            tenant,
            None,
            NewMemory {
                ttl_seconds: None,
                ..NewMemory::new(Layer::Semantic, "ข้อเท็จจริงที่หมดอายุแล้ว")
            },
        )
        .await
        .expect("remember expiring");
    let pool = store.pool_for_test();
    sqlx::query(
        "UPDATE ai_memory SET expires_at = now() - interval '1 second'
          WHERE tenant_id = $1 AND content = 'ข้อเท็จจริงที่หมดอายุแล้ว'",
    )
    .bind(tenant.0)
    .execute(pool)
    .await
    .expect("expire");

    let hidden = store
        .recall(tenant, "ข้อเท็จจริง", &Layer::ALL, 10)
        .await
        .expect("recall");
    assert!(hidden.iter().all(|h| h.content != "ข้อเท็จจริงที่หมดอายุแล้ว"));

    assert_eq!(store.forget_expired(tenant).await.expect("purge"), 1);
}

#[tokio::test]
async fn stats_count_live_rows_per_layer() {
    let Some((store, tenant, _)) = store().await else {
        eprintln!("SKIP: DATABASE_URL unset");
        return;
    };

    store
        .remember(
            tenant,
            None,
            NewMemory::new(Layer::Working, "task: ตรวจสลิป"),
        )
        .await
        .expect("working");
    store
        .remember(
            tenant,
            None,
            NewMemory::new(Layer::Working, "task: อนุมัติรอบ"),
        )
        .await
        .expect("working 2");

    let stats: Vec<LayerCount> = store.stats(tenant).await.expect("stats");
    let working = stats
        .iter()
        .find(|s| s.layer == "working")
        .expect("working count");
    assert_eq!(working.count, 2);
}
