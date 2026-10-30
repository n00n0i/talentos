//! Team calendar: one month view of holidays, leave and OT per day.
//!
//! Days with no holiday, leave or OT are omitted from the result; the client
//! renders an empty state for a month with no activity.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use serde::Serialize;
use sqlx::Row;
use talentos_db::TenantPool;
use talentos_kernel::TenantId;

/// Longest range the calendar endpoint accepts, in days.
pub const MAX_CALENDAR_RANGE_DAYS: i64 = 62;

#[derive(Debug, Serialize)]
pub struct CalendarHoliday {
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct CalendarLeave {
    pub employee_code: String,
    pub employee_name: String,
    pub leave_type: String,
    pub label_th: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct CalendarOt {
    pub employee_code: String,
    pub employee_name: String,
    pub minutes: i32,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct CalendarDay {
    pub date: NaiveDate,
    pub holidays: Vec<CalendarHoliday>,
    pub leave: Vec<CalendarLeave>,
    pub ot: Vec<CalendarOt>,
}

#[derive(Debug, thiserror::Error)]
pub enum CalendarError {
    #[error("date range must be at most 62 days")]
    TooWide,
    #[error("from must be on or before to")]
    BadRange,
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

type DayBucket = (Vec<CalendarHoliday>, Vec<CalendarLeave>, Vec<CalendarOt>);

/// Holidays, leave and OT overlapping `[from, to]`, grouped by day ascending.
pub async fn calendar(
    pool: &TenantPool,
    tenant_id: TenantId,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<CalendarDay>, CalendarError> {
    if to < from {
        return Err(CalendarError::BadRange);
    }
    if (to - from).num_days() + 1 > MAX_CALENDAR_RANGE_DAYS {
        return Err(CalendarError::TooWide);
    }

    let mut tx = pool.begin_tenant(tenant_id).await?;

    let holidays = sqlx::query(
        "SELECT holiday_date, name_th FROM tenant_holidays
          WHERE tenant_id = $1 AND holiday_date BETWEEN $2 AND $3",
    )
    .bind(tenant_id.0)
    .bind(from)
    .bind(to)
    .fetch_all(&mut *tx)
    .await?;

    let leave = sqlx::query(
        r#"SELECT lr.leave_type, lr.start_date, lr.end_date, lr.status,
                  e.employee_code, e.first_name_th, e.last_name_th,
                  COALESCE(
                      (SELECT lt.label_th FROM leave_types lt
                        WHERE lt.name = lr.leave_type
                          AND (lt.tenant_id IS NULL OR lt.tenant_id = $1)
                        ORDER BY (lt.tenant_id IS NOT NULL) DESC LIMIT 1),
                      lr.leave_type
                  ) AS label_th
             FROM leave_requests lr
             JOIN employees e ON e.id = lr.employee_id
            WHERE lr.tenant_id = $1
              AND lr.status IN ('pending', 'approved')
              AND lr.start_date <= $3 AND lr.end_date >= $2
            ORDER BY lr.start_date"#,
    )
    .bind(tenant_id.0)
    .bind(from)
    .bind(to)
    .fetch_all(&mut *tx)
    .await?;

    let ot = sqlx::query(
        r#"SELECT o.work_date, o.minutes, o.status,
                  e.employee_code, e.first_name_th, e.last_name_th
             FROM ot_requests o
             JOIN employees e ON e.id = o.employee_id
            WHERE o.tenant_id = $1
              AND o.status IN ('pending', 'approved')
              AND o.work_date BETWEEN $2 AND $3
            ORDER BY o.work_date"#,
    )
    .bind(tenant_id.0)
    .bind(from)
    .bind(to)
    .fetch_all(&mut *tx)
    .await?;

    let _ = tx.rollback().await;

    let mut buckets: BTreeMap<NaiveDate, DayBucket> = BTreeMap::new();

    for row in holidays {
        let date: NaiveDate = row.try_get("holiday_date")?;
        let entry = buckets.entry(date).or_default();
        entry.0.push(CalendarHoliday {
            name: row.try_get("name_th")?,
        });
    }

    for row in leave {
        let start: NaiveDate = row.try_get("start_date")?;
        let entry = buckets.entry(start).or_default();
        entry.1.push(CalendarLeave {
            employee_code: row.try_get("employee_code")?,
            employee_name: format!(
                "{} {}",
                row.try_get::<String, _>("first_name_th")?,
                row.try_get::<String, _>("last_name_th")?
            ),
            leave_type: row.try_get("leave_type")?,
            label_th: row.try_get("label_th")?,
            start_date: start,
            end_date: row.try_get("end_date")?,
            status: row.try_get("status")?,
        });
    }

    for row in ot {
        let date: NaiveDate = row.try_get("work_date")?;
        let entry = buckets.entry(date).or_default();
        entry.2.push(CalendarOt {
            employee_code: row.try_get("employee_code")?,
            employee_name: format!(
                "{} {}",
                row.try_get::<String, _>("first_name_th")?,
                row.try_get::<String, _>("last_name_th")?
            ),
            minutes: row.try_get("minutes")?,
            status: row.try_get("status")?,
        });
    }

    Ok(buckets
        .into_iter()
        .map(|(date, (holidays, leave, ot))| CalendarDay {
            date,
            holidays,
            leave,
            ot,
        })
        .collect())
}
