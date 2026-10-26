use temporal_sdk::{Worker, Activity};

pub async fn register(worker: &Worker<'_>) -> anyhow::Result<()> {
    // Register all activities here
    worker.register_fn(crate::workflows::calculate_employee_activity);
    Ok(())
}