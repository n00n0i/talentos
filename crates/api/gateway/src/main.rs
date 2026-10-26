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

    // Parse REDIS_URL so non-local deployments work.
    let redis_config = Config::from_url(&redis_url).expect("invalid REDIS_URL");
    let redis = Client::new(redis_config, None, None, None);
    redis.init().await.expect("Redis init failed");
    redis
        .ping::<String>(None)
        .await
        .expect("Redis connect failed");

    let state = AppState {
        pool: pool.clone(),
        jwt_secret: Arc::from(jwt_secret.as_bytes()),
        redis,
        access_ttl_secs: access_ttl,
        refresh_ttl_days,
        allowed_origins,
        started_at: std::time::Instant::now(),
    };

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
