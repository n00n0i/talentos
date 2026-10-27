use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Utc};

/// Shift schedule for a single work day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shift {
    pub start: NaiveTime,
    pub end: NaiveTime,
    pub break_minutes: i32,
    pub grace_minutes: i32,
}

/// Direction of a time-clock punch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PunchKind {
    In,
    Out,
}

/// A single time-clock punch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Punch {
    pub kind: PunchKind,
    pub at: DateTime<Utc>,
}

/// Classification of overtime for a given work day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtKind {
    Weekday,
    Weekend,
    Holiday,
}

/// Calculated attendance result for a work day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DaySummary {
    pub worked_minutes: i32,
    pub late_minutes: i32,
    pub early_leave_minutes: i32,
    pub ot_minutes: i32,
    pub ot_kind: OtKind,
    pub incomplete: bool,
}

/// Errors arising from attendance rule evaluation.
///
/// The current rule set is total: every valid input produces a summary, so
/// this enum is intentionally uninhabited. Future rules (e.g. maximum shift
/// length) can add variants here without changing service error wiring.
#[derive(Debug, thiserror::Error)]
pub enum AttendanceRuleError {}

/// Bangkok (Asia/Bangkok) offset: UTC+7, constant throughout the year.
pub fn bangkok_offset() -> FixedOffset {
    const OFFSET: FixedOffset = match FixedOffset::east_opt(7 * 60 * 60) {
        Some(offset) => offset,
        // 25_200 seconds (UTC+7) is inside FixedOffset's ±24h range, so this
        // branch is evaluated only if that invariant changes.
        None => panic!("UTC+7 is a valid fixed offset"),
    };
    OFFSET
}

/// Derive a daily attendance summary from a shift and a chronology of punches.
pub fn derive_day(
    shift: &Shift,
    punches: &[Punch],
    work_date: NaiveDate,
    is_holiday: bool,
    is_weekend: bool,
) -> Result<DaySummary, AttendanceRuleError> {
    let bkk = bangkok_offset();

    // Normalise every punch to Asia/Bangkok wall-clock time.
    let local_punches: Vec<(PunchKind, NaiveDateTime)> = punches
        .iter()
        .map(|p| (p.kind, p.at.with_timezone(&bkk).naive_local()))
        .collect();

    let mut pairs: Vec<(NaiveDateTime, NaiveDateTime)> = Vec::new();
    let mut pending_in: Option<NaiveDateTime> = None;

    for (kind, at) in local_punches {
        match kind {
            PunchKind::In => {
                // A duplicate In before an Out is ignored: keep the latest In.
                pending_in = Some(at);
            }
            PunchKind::Out => {
                if let Some(in_dt) = pending_in.take() {
                    pairs.push((in_dt, at));
                }
                // An Out with no pending In is an orphaned punch and is ignored.
            }
        }
    }

    let incomplete = pending_in.is_some();

    let raw_worked: i32 = pairs
        .iter()
        .map(|(a, b)| (*b - *a).num_minutes() as i32)
        .sum();
    // LPA §27: the rest break is only deducted from stints longer than 5 hours.
    let worked_minutes = if raw_worked > 300 {
        (raw_worked - shift.break_minutes).max(0)
    } else {
        raw_worked
    };

    let shift_start = work_date.and_time(shift.start);
    let shift_end = if shift.end > shift.start {
        work_date.and_time(shift.end)
    } else {
        (work_date + chrono::Duration::days(1)).and_time(shift.end)
    };
    let grace_start = shift_start + chrono::Duration::minutes(shift.grace_minutes.into());

    let late_minutes = pairs
        .first()
        .map(|(in_dt, _)| ((*in_dt - grace_start).num_minutes()).max(0) as i32)
        .unwrap_or(0);

    let early_leave_minutes = pairs
        .last()
        .map(|(_, out_dt)| ((shift_end - *out_dt).num_minutes()).max(0) as i32)
        .unwrap_or(0);

    let ot_kind = if is_holiday {
        OtKind::Holiday
    } else if is_weekend {
        OtKind::Weekend
    } else {
        OtKind::Weekday
    };

    let ot_minutes = match ot_kind {
        OtKind::Holiday | OtKind::Weekend => worked_minutes,
        // Weekday OT is time worked after the shift ends; early arrival is not OT.
        OtKind::Weekday => pairs
            .iter()
            .map(|(a, b)| (*b - (*a).max(shift_end)).num_minutes().max(0) as i32)
            .sum::<i32>()
            .min(worked_minutes),
    };

    Ok(DaySummary {
        worked_minutes,
        late_minutes,
        early_leave_minutes,
        ot_minutes,
        ot_kind,
        incomplete,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn time(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    fn utc(y: i32, mo: u32, d: u32, h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, m, 0).unwrap()
    }

    fn punch_in(at: DateTime<Utc>) -> Punch {
        Punch {
            kind: PunchKind::In,
            at,
        }
    }

    fn punch_out(at: DateTime<Utc>) -> Punch {
        Punch {
            kind: PunchKind::Out,
            at,
        }
    }

    #[test]
    fn normal_day_nine_to_six() {
        let shift = Shift {
            start: time(9, 0),
            end: time(18, 0),
            break_minutes: 60,
            grace_minutes: 0,
        };
        let punches = vec![
            punch_in(utc(2026, 10, 27, 2, 0)),   // 09:00 Bangkok
            punch_out(utc(2026, 10, 27, 11, 0)), // 18:00 Bangkok
        ];
        let summary = derive_day(&shift, &punches, date(2026, 10, 27), false, false).unwrap();
        assert_eq!(summary.worked_minutes, 480);
        assert_eq!(summary.late_minutes, 0);
        assert_eq!(summary.early_leave_minutes, 0);
        assert_eq!(summary.ot_minutes, 0);
        assert_eq!(summary.ot_kind, OtKind::Weekday);
        assert!(!summary.incomplete);
    }

    #[test]
    fn late_with_grace() {
        let shift = Shift {
            start: time(9, 0),
            end: time(18, 0),
            break_minutes: 60,
            grace_minutes: 5,
        };
        let punches = vec![
            punch_in(utc(2026, 10, 27, 2, 15)), // 09:15 Bangkok
            punch_out(utc(2026, 10, 27, 11, 0)),
        ];
        let summary = derive_day(&shift, &punches, date(2026, 10, 27), false, false).unwrap();
        assert_eq!(summary.late_minutes, 10);
    }

    #[test]
    fn early_leave() {
        let shift = Shift {
            start: time(9, 0),
            end: time(18, 0),
            break_minutes: 60,
            grace_minutes: 0,
        };
        let punches = vec![
            punch_in(utc(2026, 10, 27, 2, 0)),
            punch_out(utc(2026, 10, 27, 10, 0)), // 17:00 Bangkok
        ];
        let summary = derive_day(&shift, &punches, date(2026, 10, 27), false, false).unwrap();
        assert_eq!(summary.early_leave_minutes, 60);
    }

    #[test]
    fn overnight_shift_weekday_ot() {
        let shift = Shift {
            start: time(22, 0),
            end: time(6, 0),
            break_minutes: 60,
            grace_minutes: 0,
        };
        let punches = vec![
            punch_in(utc(2026, 10, 26, 15, 0)),  // 22:00 Bangkok
            punch_out(utc(2026, 10, 26, 23, 5)), // 06:05 Bangkok next day
        ];
        let summary = derive_day(&shift, &punches, date(2026, 10, 26), false, false).unwrap();
        assert_eq!(summary.worked_minutes, 425);
        assert_eq!(summary.ot_minutes, 5);
        assert_eq!(summary.ot_kind, OtKind::Weekday);
    }

    #[test]
    fn holiday_short_stint_is_all_ot_without_break() {
        let shift = Shift {
            start: time(9, 0),
            end: time(18, 0),
            break_minutes: 60,
            grace_minutes: 0,
        };
        let punches = vec![
            punch_in(utc(2026, 10, 27, 2, 0)),
            punch_out(utc(2026, 10, 27, 5, 0)), // 12:00 Bangkok, 3h worked
        ];
        let summary = derive_day(&shift, &punches, date(2026, 10, 27), true, false).unwrap();
        assert_eq!(summary.worked_minutes, 180);
        assert_eq!(summary.ot_minutes, 180);
        assert_eq!(summary.ot_kind, OtKind::Holiday);
    }

    #[test]
    fn early_arrival_and_late_leave_weekday_ot_is_after_shift_end() {
        let shift = Shift {
            start: time(9, 0),
            end: time(18, 0),
            break_minutes: 60,
            grace_minutes: 0,
        };
        let punches = vec![
            punch_in(utc(2026, 10, 27, 1, 50)),   // 08:50 Bangkok
            punch_out(utc(2026, 10, 27, 12, 30)), // 19:30 Bangkok
        ];
        let summary = derive_day(&shift, &punches, date(2026, 10, 27), false, false).unwrap();
        assert_eq!(summary.worked_minutes, 580);
        assert_eq!(summary.ot_minutes, 90);
        assert_eq!(summary.late_minutes, 0);
        assert_eq!(summary.early_leave_minutes, 0);
    }

    #[test]
    fn early_arrival_alone_is_not_ot() {
        let shift = Shift {
            start: time(9, 0),
            end: time(18, 0),
            break_minutes: 60,
            grace_minutes: 0,
        };
        let punches = vec![
            punch_in(utc(2026, 10, 27, 1, 50)),  // 08:50 Bangkok
            punch_out(utc(2026, 10, 27, 11, 0)), // 18:00 Bangkok
        ];
        let summary = derive_day(&shift, &punches, date(2026, 10, 27), false, false).unwrap();
        assert_eq!(summary.worked_minutes, 490);
        assert_eq!(summary.ot_minutes, 0);
    }

    #[test]
    fn duplicate_in_uses_latest_before_out() {
        let shift = Shift {
            start: time(9, 0),
            end: time(18, 0),
            break_minutes: 0,
            grace_minutes: 5,
        };
        let punches = vec![
            punch_in(utc(2026, 10, 27, 2, 0)),   // 09:00 Bangkok
            punch_in(utc(2026, 10, 27, 2, 5)),   // 09:05 Bangkok
            punch_out(utc(2026, 10, 27, 11, 0)), // 18:00 Bangkok
        ];
        let summary = derive_day(&shift, &punches, date(2026, 10, 27), false, false).unwrap();
        assert_eq!(summary.late_minutes, 0);
        assert_eq!(summary.worked_minutes, 535);
    }

    #[test]
    fn missing_out_is_incomplete() {
        let shift = Shift {
            start: time(9, 0),
            end: time(18, 0),
            break_minutes: 60,
            grace_minutes: 0,
        };
        let punches = vec![punch_in(utc(2026, 10, 27, 2, 0))];
        let summary = derive_day(&shift, &punches, date(2026, 10, 27), false, false).unwrap();
        assert!(summary.incomplete);
        assert_eq!(summary.worked_minutes, 0);
    }

    #[test]
    fn timezone_plus_seven_and_utc_normalised_to_bangkok() {
        let shift = Shift {
            start: time(9, 0),
            end: time(18, 0),
            break_minutes: 60,
            grace_minutes: 0,
        };
        let plus_seven = FixedOffset::east_opt(7 * 60 * 60).unwrap();
        let punches = vec![
            Punch {
                kind: PunchKind::In,
                at: plus_seven
                    .with_ymd_and_hms(2026, 10, 27, 9, 0, 0)
                    .unwrap()
                    .with_timezone(&Utc),
            },
            punch_out(utc(2026, 10, 27, 11, 0)),
        ];
        let summary = derive_day(&shift, &punches, date(2026, 10, 27), false, false).unwrap();
        assert_eq!(summary.worked_minutes, 480);
    }
}
