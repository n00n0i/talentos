//! Recruitment pipeline: jobs, candidates, stages, validation, tenant scope.
//! Requires DATABASE_URL; skips when unset.

use sqlx::PgPool;
use talentos_db::TenantPool;
use talentos_hr_runner::recruitment::{
    create_candidate, create_job, delete_candidate, delete_job, list_candidates, list_jobs,
    set_candidate_stage, set_job_status, CandidateInput, JobInput, RecruitmentError,
};
use talentos_kernel::TenantId;

mod common;
use common::{cleanup, database_url, seed_tenant_entities, user_id_of};

fn job(title: &str) -> JobInput {
    JobInput {
        title: title.into(),
        department: Some("Engineering".into()),
        employment_type: "full_time".into(),
        headcount: 2,
        description: Some("Build things".into()),
    }
}

fn candidate(first: &str) -> CandidateInput {
    CandidateInput {
        first_name: first.into(),
        last_name: "ทดสอบ".into(),
        email: Some(format!("{first}@example.com")),
        phone: Some("0800000000".into()),
        source: Some("referral".into()),
        note: None,
    }
}

#[tokio::test]
async fn job_and_candidate_pipeline() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;
    let tenant = TenantId(tenant_id);

    let opened = create_job(&pool, tenant, user_id, &job("Senior Backend"))
        .await
        .expect("job");
    assert_eq!(opened.status, "open");
    assert_eq!(opened.candidate_count, 0);

    let alice = create_candidate(&pool, tenant, user_id, opened.id, &candidate("Alice"))
        .await
        .expect("candidate A");
    let bob = create_candidate(&pool, tenant, user_id, opened.id, &candidate("Bob"))
        .await
        .expect("candidate B");
    assert_eq!(alice.stage, "applied");

    let listed = list_candidates(&pool, tenant, opened.id)
        .await
        .expect("list candidates");
    assert_eq!(listed.len(), 2);

    let moved = set_candidate_stage(&pool, tenant, alice.id, "interview")
        .await
        .expect("move stage");
    assert_eq!(moved.stage, "interview");
    let ordered = list_candidates(&pool, tenant, opened.id)
        .await
        .expect("ordered candidates");
    assert_eq!(
        ordered[0].id, bob.id,
        "applied comes first in pipeline order"
    );
    assert!(ordered
        .iter()
        .any(|c| c.id == alice.id && c.stage == "interview"));

    set_candidate_stage(&pool, tenant, alice.id, "hired")
        .await
        .expect("hire");
    let jobs = list_jobs(&pool, tenant).await.expect("jobs");
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].candidate_count, 2);
    assert_eq!(jobs[0].hired_count, 1);

    set_job_status(&pool, tenant, opened.id, false)
        .await
        .expect("close");
    assert_eq!(
        list_jobs(&pool, tenant).await.expect("jobs")[0].status,
        "closed"
    );
    set_job_status(&pool, tenant, opened.id, true)
        .await
        .expect("reopen");

    delete_candidate(&pool, tenant, bob.id)
        .await
        .expect("delete candidate");
    delete_job(&pool, tenant, opened.id)
        .await
        .expect("delete job");
    assert!(matches!(
        list_candidates(&pool, tenant, opened.id).await,
        Err(RecruitmentError::JobNotFound)
    ));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn validation_and_tenant_isolation() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;
    let tenant = TenantId(tenant_id);

    let mut bad = job("  ");
    bad.title = "  ".into();
    assert!(matches!(
        create_job(&pool, tenant, user_id, &bad).await,
        Err(RecruitmentError::InvalidTitle)
    ));
    let mut bad = job("ok");
    bad.employment_type = "gig".into();
    assert!(matches!(
        create_job(&pool, tenant, user_id, &bad).await,
        Err(RecruitmentError::InvalidEmploymentType)
    ));
    let mut bad = job("ok");
    bad.headcount = 0;
    assert!(matches!(
        create_job(&pool, tenant, user_id, &bad).await,
        Err(RecruitmentError::InvalidHeadcount)
    ));

    let opened = create_job(&pool, tenant, user_id, &job("Dev"))
        .await
        .expect("job");
    let mut bad = candidate("NoLast");
    bad.last_name = " ".into();
    assert!(matches!(
        create_candidate(&pool, tenant, user_id, opened.id, &bad).await,
        Err(RecruitmentError::InvalidName)
    ));
    let mut bad = candidate("LongMail");
    bad.email = Some(format!("{}@x", "a".repeat(200)));
    assert!(matches!(
        create_candidate(&pool, tenant, user_id, opened.id, &bad).await,
        Err(RecruitmentError::InvalidEmail)
    ));
    assert!(matches!(
        set_candidate_stage(&pool, tenant, opened.id, "manager").await,
        Err(RecruitmentError::InvalidStage)
    ));

    let candidate = create_candidate(&pool, tenant, user_id, opened.id, &candidate("Eve"))
        .await
        .expect("candidate");

    let (other_tenant, _) = seed_tenant_entities(&raw, 1).await;
    let other = TenantId(other_tenant);
    assert!(list_jobs(&pool, other)
        .await
        .expect("other jobs")
        .is_empty());
    assert!(matches!(
        list_candidates(&pool, other, opened.id).await,
        Err(RecruitmentError::JobNotFound)
    ));
    assert!(matches!(
        set_candidate_stage(&pool, other, candidate.id, "interview").await,
        Err(RecruitmentError::CandidateNotFound)
    ));
    assert!(matches!(
        delete_job(&pool, other, opened.id).await,
        Err(RecruitmentError::JobNotFound)
    ));

    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(other_tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let visible: i64 = sqlx::query_scalar("SELECT count(*) FROM candidates")
        .fetch_one(&mut *tx)
        .await
        .expect("rls count");
    let _ = tx.rollback().await;
    assert_eq!(visible, 0, "RLS hides other tenants without a filter");

    cleanup(&raw, tenant_id).await;
    cleanup(&raw, other_tenant).await;
}
