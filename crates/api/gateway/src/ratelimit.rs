use std::time::Duration;

use fred::prelude::*;

/// Fixed-window counter per key. Returns Err(retry_after_secs) when over `max` within `window`.
/// Two windows enforced by the caller: IP-based and email-based (spec §6.5).
///
/// **Fail-open**: if Redis is unreachable, allow the request through but log loudly.
/// Availability > perfect throttling — a brute-force attacker gets one free pass, not an outage.
pub async fn check(redis: &Client, key: &str, max: u32, window: Duration) -> Result<(), u64> {
    let count = match redis.incr::<u32, _>(key).await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "redis rate-limit error — failing open");
            return Ok(()); // B4: allow through on Redis failure
        }
    };

    if count == 1 {
        // First hit in this window: set expiry. Shortcut: non-atomic INCR+EXPIRE; acceptable
        // because a crash between them only leaves an extra-until-restart key, not a bypass.
        // Upgrade to Lua script if strict atomicity ever matters.
        let _: () = redis
            .expire(key, window.as_secs() as i64, None)
            .await
            .unwrap_or(());
    }

    if count > max {
        let ttl: Option<u64> = redis.ttl::<u64, _>(key).await.ok();
        return Err(ttl.unwrap_or(window.as_secs()));
    }
    Ok(())
}

/// Clear the failure counter after a successful login (both windows).
pub async fn reset(redis: &Client, keys: &[String]) {
    for k in keys {
        let _: () = redis.unlink::<(), _>(k).await.unwrap_or(());
    }
}
