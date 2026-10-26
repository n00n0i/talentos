mod cli;
mod dto;
mod handlers;
mod middleware;
mod ratelimit;
mod router;
mod state;

use std::net::SocketAddr;
use std::sync::Arc;

use fred::prelude::*;

use state::AppState;
use talentos_db::TenantPool;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=debug".into()),
        )
        .json()
        .init();

    match std::env::args().nth(1).as_deref() {
        Some("provision") => {
            let args: Vec<String> = std::env::args().skip(2).collect();
            if let Err(e) = cli::provision(&args).await {
                eprintln!("provision failed: {e}");
                std::process::exit(1);
            }
            return;
        }
        Some("migrate") => {
            if let Err(e) = cli::migrate().await {
                eprintln!("migrate failed: {e}");
                std::process::exit(1);
            }
            return;
        }
        Some("pii-encrypt") => {
            if let Err(e) = cli::encrypt_existing_pii().await {
                eprintln!("pii-encrypt failed: {e}");
                std::process::exit(1);
            }
            return;
        }
        _ => {}
    }

    let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
    let jwt_secret = std::env::var("AUTH_JWT_SECRET").expect("AUTH_JWT_SECRET must be set");
    if jwt_secret.len() < 32 {
        eprintln!(
            "FATAL: AUTH_JWT_SECRET must be >= 32 bytes, got {}",
            jwt_secret.len()
        );
        std::process::exit(1);
    }
    let bind_addr: SocketAddr = std::env::var("GATEWAY_PORT")
        .unwrap_or_else(|_| "8080".into())
        .parse::<u16>()
        .map(|p| SocketAddr::from(([0, 0, 0, 0], p)))
        .expect("invalid GATEWAY_PORT");
    let access_ttl: i64 = std::env::var("AUTH_ACCESS_TTL_SECS")
        .unwrap_or_else(|_| "900".into())
        .parse()
        .expect("AUTH_ACCESS_TTL_SECS must be integer");
    let refresh_ttl_days: i64 = std::env::var("AUTH_REFRESH_TTL_DAYS")
        .unwrap_or_else(|_| "7".into())
        .parse()
        .expect("AUTH_REFRESH_TTL_DAYS must be integer");
    let max_conns: u32 = std::env::var("DB_MAX_CONNS")
        .unwrap_or_else(|_| "10".into())
        .parse()
        .expect("DB_MAX_CONNS must be integer");
    let allowed_origins: Vec<String> = std::env::var("AUTH_ALLOWED_ORIGINS")
        .unwrap_or_else(|_| "http://localhost:3000".into())
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();

    let pool = Arc::new(
        TenantPool::connect(&db_url, max_conns)
            .await
            .expect("DB connect failed"),
    );

    // Auth pool bypasses RLS by design; see scripts/init-db.sql.
    // Required explicitly: falling back to the tenant URL would silently break login.
    let auth_db_url = std::env::var("DATABASE_AUTH_URL")
        .expect("DATABASE_AUTH_URL must be set (role with BYPASSRLS, used only by login/refresh)");
    let auth_pool = sqlx::PgPool::connect(&auth_db_url)
        .await
        .expect("DB auth connect failed");

    // Parse REDIS_URL so non-local deployments work.
    let redis_config = Config::from_url(&redis_url).expect("invalid REDIS_URL");
    let redis = Client::new(redis_config, None, None, None);
    redis.init().await.expect("Redis init failed");
    redis
        .ping::<String>(None)
        .await
        .expect("Redis connect failed");

    let pii = match talentos_db::pii::PiiCipher::from_env() {
        Ok(Some(cipher)) => Some(Arc::new(cipher)),
        Ok(None) => {
            tracing::warn!("PII_ENCRYPTION_KEY is unset: new employee PII is stored as plaintext");
            None
        }
        Err(e) => {
            eprintln!("FATAL: {e}");
            std::process::exit(1);
        }
    };

    let state = AppState {
        pool: pool.clone(),
        auth_pool: auth_pool.clone(),
        jwt_secret: Arc::from(jwt_secret.as_bytes()),
        redis,
        access_ttl_secs: access_ttl,
        refresh_ttl_days,
        allowed_origins,
        graphql_schema: Arc::new(talentos_graphql::build_schema()),
        pii,
        memory: Arc::new(talentos_memory::MemoryStore::new(
            pool.clone(),
            talentos_model_gw::HuakatiClient::new().ok().map(Arc::new),
        )),
        started_at: std::time::Instant::now(),
    };

    // Crash recovery: re-run payroll runs abandoned mid-calculation.
    // shortcut: 60s poll in-process; move to a Temporal schedule when the worker lands.
    {
        let recovery_auth = auth_pool.clone();
        let recovery_pool = pool.clone();
        let stale_after: i64 = std::env::var("PAYROLL_STUCK_AFTER_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(600);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                ticker.tick().await;
                match talentos_payroll_runner::recover_stuck_runs(
                    &recovery_auth,
                    &recovery_pool,
                    stale_after,
                )
                .await
                {
                    Ok(0) => {}
                    Ok(n) => tracing::info!(count = n, "recovered stuck payroll runs"),
                    Err(e) => tracing::warn!(error = %e, "payroll recovery sweep failed"),
                }
            }
        });
    }

    let app = router::build(state);

    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .expect("failed to bind");
    tracing::info!(%bind_addr, "gateway listening");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .expect("server error");
}

async fn shutdown_signal() {
    let ctrl_c = async { tokio::signal::ctrl_c().await.ok() };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("sigterm")
            .recv()
            .await
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<Option<()>>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutting down gracefully");
}
