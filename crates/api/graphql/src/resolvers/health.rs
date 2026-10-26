use async_graphql::{Object, SimpleObject};

#[derive(SimpleObject)]
pub struct HealthStatus {
    pub status: String,
    pub version: String,
}

#[derive(Default)]
pub struct HealthResolver;

#[Object]
impl HealthResolver {
    async fn health(&self) -> HealthStatus {
        HealthStatus {
            status: "ok".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }
}