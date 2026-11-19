//! When the daily image is checked (WALL-03, REL-04, SRS 7.6, ADR 0029). Pure, so it is tested
//! with a fake clock (MAINT-01).
//!
//! The check runs once a day after local midnight:
//! - **No new image yet:** if the feed still offers the current image, it checks up to
//!   [`SAME_IMAGE_RETRIES`] more times that day, [`SAME_IMAGE_GAP_MS`] apart (WALL-03).
//! - **A failure:** it backs off, [`FIRST_BACK_OFF_MS`] at first and doubling (REL-04).
//! - **The budget:** at most [`MAX_ATTEMPTS_A_DAY`] network attempts a day (REL-04).
//! - **A new day:** it starts the day's counts again.
//! - **Unavailable:** [`UNAVAILABLE_AFTER_MS`] of failures make Settings say so, with Try again
//!   (SRS 7.6).
//!
//! The log of what the checks did, [`Checks`], is kept in the image cache.

use meegone_core_data::{ImageJob, JobNetwork};
use meegone_lockimage_data::Checks;

const MINUTE_MS: u64 = 60_000;
const HOUR_MS: u64 = 60 * MINUTE_MS;
const DAY_MS: u64 = 24 * HOUR_MS;

/// Network attempts a day, at most (REL-04).
pub const MAX_ATTEMPTS_A_DAY: u32 = 6;
/// Checks after the first, when the feed still offers the current image (WALL-03).
pub const SAME_IMAGE_RETRIES: u32 = 3;
/// Time between checks that find no new image, at least (WALL-03).
pub const SAME_IMAGE_GAP_MS: u64 = 2 * HOUR_MS;
/// The wait after the first failure; it doubles with each failure after it (REL-04).
pub const FIRST_BACK_OFF_MS: u64 = 15 * MINUTE_MS;
/// How long a due check waits before it looks at the battery and the network again. The
/// system's scheduler waits for the network and the battery itself, so this only keeps the job
/// from running again at once when MeeGone and the system disagree (WALL-04, PWR-04).
pub const WAIT_MS: u64 = 15 * MINUTE_MS;
/// Failures for this long make the daily image unavailable, with Try again (SRS 7.6).
pub const UNAVAILABLE_AFTER_MS: u64 = 3 * DAY_MS;

/// The local time zone's offset from UTC at a time, as `SystemConfigAdapter::utc_offset_ms`
/// gives it.
pub type Offset<'a> = &'a dyn Fn(u64) -> i64;

/// How the daily image's checks are going, for Settings (WALL-16, SRS 7.6).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum CheckStatus {
    /// Nothing has failed since the feed last answered.
    #[default]
    Fine,
    /// The last attempt failed for this reason; the job will try again (REL-04).
    Retrying(String),
    /// Every attempt has failed for [`UNAVAILABLE_AFTER_MS`], the last for this reason:
    /// "Daily image unavailable", with Try again (SRS 7.6). The job still tries.
    Unavailable(String),
}

/// The local day `at_ms` falls on, in days since 1 January 1970.
pub fn local_day(at_ms: u64, offset: Offset<'_>) -> u64 {
    let local = i64::try_from(at_ms)
        .unwrap_or(i64::MAX)
        .saturating_add(offset(at_ms));
    u64::try_from(local.div_euclid(DAY_MS as i64)).unwrap_or(0)
}

/// When local `day` starts: its midnight, or where the clock skips midnight, the first moment
/// on that day. The offset is asked at midnight itself, so it is right on the days the clocks
/// change.
pub fn day_start_ms(day: u64, offset: Offset<'_>) -> u64 {
    let utc = day.saturating_mul(DAY_MS);
    let back = |at: u64| shift(utc, -offset(at));
    let first = back(utc);
    let second = back(first);
    [second, first]
        .into_iter()
        .filter(|&at| local_day(at, offset) == day)
        .min()
        .unwrap_or(second.max(first))
}

fn shift(at_ms: u64, by_ms: i64) -> u64 {
    at_ms.saturating_add_signed(by_ms)
}

/// `checks` as they stand on `now_ms`'s local day: on a new day, the day's counts start again.
/// The failure streak and its problem carry on (SRS 7.6).
pub fn today(checks: &Checks, now_ms: u64, offset: Offset<'_>) -> Checks {
    let day = local_day(now_ms, offset);
    if checks.day == day {
        return checks.clone();
    }
    Checks {
        day,
        attempts: 0,
        same: 0,
        updated: false,
        failures: 0,
        ..checks.clone()
    }
}

/// When the next check is due, given what the checks did (WALL-03, REL-04). At or before
/// `now_ms` means now.
pub fn due_ms(checks: &Checks, now_ms: u64, offset: Offset<'_>) -> u64 {
    let checks = today(checks, now_ms, offset);
    let tomorrow = day_start_ms(checks.day + 1, offset);
    let done =
        checks.updated || checks.attempts >= MAX_ATTEMPTS_A_DAY || checks.same > SAME_IMAGE_RETRIES;
    let last = checks.last_attempt_ms.unwrap_or(0);
    let next = if done {
        tomorrow
    } else if checks.failures > 0 {
        last.saturating_add(back_off_ms(checks.failures))
    } else if checks.same > 0 {
        last.saturating_add(SAME_IMAGE_GAP_MS)
    } else {
        // Today's first check: due since midnight.
        day_start_ms(checks.day, offset)
    };
    // A retry that would come after midnight gives way to the next day's check.
    next.min(tomorrow)
}

/// The wait after `failures` failures in a row: 15 minutes, then 30, 60 and so on (REL-04).
pub fn back_off_ms(failures: u32) -> u64 {
    FIRST_BACK_OFF_MS.saturating_mul(1 << failures.saturating_sub(1).min(32))
}

/// The job to schedule for the next check: when it is due, on the network allowed (WALL-04).
pub fn next_job(
    checks: &Checks,
    now_ms: u64,
    offset: Offset<'_>,
    use_mobile_data: bool,
) -> ImageJob {
    ImageJob {
        not_before_ms: due_ms(checks, now_ms, offset),
        network: if use_mobile_data {
            JobNetwork::Any
        } else {
            JobNetwork::Unmetered
        },
    }
}

/// Counts an attempt at `now_ms`, before any request. It counts as failed until the feed
/// answers, so an attempt cut short keeps to the budget and the back-off (REL-04).
pub fn begin(checks: &mut Checks, now_ms: u64, offset: Offset<'_>) {
    *checks = today(checks, now_ms, offset);
    checks.attempts += 1;
    checks.failures += 1;
    checks.last_attempt_ms = Some(now_ms);
    checks.failing_since_ms.get_or_insert(now_ms);
}

/// The feed answered: with a new image that was cached (`new`), or with none (WALL-03). Either
/// way, nothing is failing.
pub fn answered(checks: &mut Checks, new: bool) {
    if new {
        checks.updated = true;
    } else {
        checks.same += 1;
    }
    checks.failures = 0;
    checks.failing_since_ms = None;
    checks.problem = None;
}

/// The attempt failed, for `problem` in plain words (WALL-16).
pub fn failed(checks: &mut Checks, problem: String) {
    checks.problem = Some(problem);
}

/// Try again: the failure streak and its back-off are forgotten, so the next check is due at
/// once, within the day's budget (SRS 7.6, REL-04).
pub fn try_again(checks: &mut Checks) {
    checks.failures = 0;
    checks.failing_since_ms = None;
    checks.problem = None;
}

/// How the checks are going at `now_ms` (WALL-16, SRS 7.6).
pub fn status(checks: &Checks, now_ms: u64) -> CheckStatus {
    let Some(problem) = checks.problem.clone() else {
        return CheckStatus::Fine;
    };
    let failing_for = checks
        .failing_since_ms
        .map_or(0, |since| now_ms.saturating_sub(since));
    if failing_for >= UNAVAILABLE_AFTER_MS {
        CheckStatus::Unavailable(problem)
    } else {
        CheckStatus::Retrying(problem)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 19 November 2026, 00:00 UTC.
    const NOV_19: u64 = 1_795_046_400_000;
    /// Bangkok, all year.
    const UTC_7: i64 = 7 * HOUR_MS as i64;

    fn utc(_: u64) -> i64 {
        0
    }

    fn bangkok(_: u64) -> i64 {
        UTC_7
    }

    /// London on 29 March 2026: GMT until 01:00 UTC, then BST, an hour ahead.
    fn london(at_ms: u64) -> i64 {
        const BST_FROM: u64 = 1_774_746_000_000;
        if at_ms >= BST_FROM { HOUR_MS as i64 } else { 0 }
    }

    /// What the checks did after `steps` at the times given, all on one day, starting from
    /// none: `true` for an answer with a new image, `false` for one without, `None` for a
    /// failure.
    fn after(steps: &[(u64, Option<bool>)], offset: Offset<'_>) -> Checks {
        let mut checks = Checks::default();
        for &(at, result) in steps {
            begin(&mut checks, at, offset);
            match result {
                Some(new) => answered(&mut checks, new),
                None => failed(&mut checks, "No connection".into()),
            }
        }
        checks
    }

    #[test]
    fn the_first_check_is_due_after_local_midnight() {
        // WALL-03: in Bangkok, midnight is 17:00 UTC the day before.
        let bangkok_midnight = NOV_19 - UTC_7 as u64;
        assert_eq!(
            day_start_ms(local_day(NOV_19, &bangkok), &bangkok),
            bangkok_midnight
        );
        assert_eq!(
            due_ms(&Checks::default(), NOV_19, &bangkok),
            bangkok_midnight
        );

        // Once today's image is in, the next check is tomorrow's midnight there.
        let checks = after(&[(NOV_19, Some(true))], &bangkok);
        assert_eq!(due_ms(&checks, NOV_19, &bangkok), bangkok_midnight + DAY_MS);
        // In UTC the same moment is midnight itself.
        let checks = after(&[(NOV_19, Some(true))], &utc);
        assert_eq!(due_ms(&checks, NOV_19, &utc), NOV_19 + DAY_MS);
    }

    #[test]
    fn midnight_follows_the_clocks_changing() {
        // The day the clocks go forward has 23 hours: the next midnight is 23:00 UTC.
        let day_start = 1_774_742_400_000; // 29 March 2026, 00:00 GMT
        let noon = day_start + 11 * HOUR_MS;
        assert_eq!(day_start_ms(local_day(noon, &london), &london), day_start);

        let checks = after(&[(noon, Some(true))], &london);

        assert_eq!(due_ms(&checks, noon, &london), day_start + 23 * HOUR_MS);
    }

    #[test]
    fn with_no_new_image_it_checks_three_more_times_two_hours_apart() {
        // WALL-03
        let mut steps = vec![(NOV_19, Some(false))];
        let mut checks = after(&steps, &utc);
        for retry in 1..=SAME_IMAGE_RETRIES {
            let due = due_ms(&checks, NOV_19, &utc);
            assert_eq!(due, NOV_19 + u64::from(retry) * SAME_IMAGE_GAP_MS);
            steps.push((due, Some(false)));
            checks = after(&steps, &utc);
        }

        // Then not again until tomorrow.
        assert_eq!(due_ms(&checks, NOV_19 + 6 * HOUR_MS, &utc), NOV_19 + DAY_MS);
        assert_eq!(checks.attempts, 4);
    }

    #[test]
    fn a_new_image_on_a_retry_ends_the_day() {
        // WALL-03
        let checks = after(
            &[
                (NOV_19, Some(false)),
                (NOV_19 + SAME_IMAGE_GAP_MS, Some(true)),
            ],
            &utc,
        );

        assert_eq!(due_ms(&checks, NOV_19 + 3 * HOUR_MS, &utc), NOV_19 + DAY_MS);
    }

    #[test]
    fn failures_back_off_from_15_minutes_doubling_and_stop_at_6_a_day() {
        // REL-04
        let mut steps = Vec::new();
        let mut at = NOV_19;
        let mut waits = Vec::new();
        loop {
            steps.push((at, None));
            let checks = after(&steps, &utc);
            let due = due_ms(&checks, at, &utc);
            if due >= NOV_19 + DAY_MS {
                assert_eq!(checks.attempts, MAX_ATTEMPTS_A_DAY);
                break;
            }
            waits.push((due - at) / MINUTE_MS);
            at = due;
        }

        assert_eq!(waits, [15, 30, 60, 120, 240]);
    }

    #[test]
    fn an_answer_ends_the_back_off() {
        // REL-04: after an answer with no new image, the next check is two hours on.
        let checks = after(
            &[(NOV_19, None), (NOV_19 + 15 * MINUTE_MS, Some(false))],
            &utc,
        );

        assert_eq!(checks.failures, 0);
        assert_eq!(
            due_ms(&checks, NOV_19 + 15 * MINUTE_MS, &utc),
            NOV_19 + 15 * MINUTE_MS + SAME_IMAGE_GAP_MS
        );
    }

    #[test]
    fn a_retry_due_after_midnight_gives_way_to_the_next_days_check() {
        // WALL-03: a check at 23:00 that finds no new image isn't retried at 01:00; the day's
        // first check is due at midnight.
        let late = NOV_19 + 23 * HOUR_MS;
        let checks = after(&[(late, Some(false))], &utc);

        assert_eq!(due_ms(&checks, late, &utc), NOV_19 + DAY_MS);
    }

    #[test]
    fn a_new_day_starts_its_counts_again() {
        // REL-04: six failures yesterday don't count today.
        let steps: Vec<_> = (0..6).map(|n| (NOV_19 + n * HOUR_MS, None)).collect();
        let checks = after(&steps, &utc);
        let tomorrow = NOV_19 + DAY_MS + HOUR_MS;

        let fresh = today(&checks, tomorrow, &utc);

        assert_eq!(fresh.day, checks.day + 1);
        assert_eq!((fresh.attempts, fresh.same, fresh.failures), (0, 0, 0));
        assert_eq!(due_ms(&checks, tomorrow, &utc), NOV_19 + DAY_MS);
        // The failure streak carries on, for Settings (SRS 7.6).
        assert_eq!(fresh.failing_since_ms, Some(NOV_19));
        assert_eq!(fresh.problem.as_deref(), Some("No connection"));
    }

    #[test]
    fn an_attempt_cut_short_counts_as_a_failure() {
        // REL-04: begun but never finished, as when MeeGone stops part-way.
        let mut checks = Checks::default();
        begin(&mut checks, NOV_19, &utc);

        assert_eq!(checks.attempts, 1);
        assert_eq!(due_ms(&checks, NOV_19, &utc), NOV_19 + FIRST_BACK_OFF_MS);
        // It isn't shown as a problem: there is no reason to give.
        assert_eq!(status(&checks, NOV_19), CheckStatus::Fine);
    }

    #[test]
    fn three_days_of_failures_make_the_daily_image_unavailable() {
        // SRS 7.6
        let mut checks = after(&[(NOV_19, None)], &utc);
        assert_eq!(
            status(&checks, NOV_19),
            CheckStatus::Retrying("No connection".into())
        );

        let three_days = NOV_19 + UNAVAILABLE_AFTER_MS;
        checks = today(&checks, three_days, &utc);
        begin(&mut checks, three_days, &utc);
        failed(&mut checks, "The feed's format has changed".into());
        assert_eq!(
            status(&checks, three_days - 1),
            CheckStatus::Retrying("The feed's format has changed".into())
        );
        assert_eq!(
            status(&checks, three_days),
            CheckStatus::Unavailable("The feed's format has changed".into())
        );

        // An answer from the feed ends it.
        answered(&mut checks, false);
        assert_eq!(status(&checks, three_days), CheckStatus::Fine);
    }

    #[test]
    fn try_again_forgets_the_failures_but_not_the_days_budget() {
        // SRS 7.6, REL-04
        let mut checks = after(&[(NOV_19, None), (NOV_19 + 15 * MINUTE_MS, None)], &utc);

        try_again(&mut checks);

        assert_eq!(status(&checks, NOV_19), CheckStatus::Fine);
        assert_eq!(due_ms(&checks, NOV_19 + HOUR_MS, &utc), NOV_19);
        assert_eq!(checks.attempts, 2);

        let steps: Vec<_> = (0..6).map(|n| (NOV_19 + n * HOUR_MS, None)).collect();
        let mut spent = after(&steps, &utc);
        try_again(&mut spent);
        assert_eq!(due_ms(&spent, NOV_19 + 6 * HOUR_MS, &utc), NOV_19 + DAY_MS);
    }

    #[test]
    fn the_job_waits_for_an_unmetered_network_unless_mobile_data_is_allowed() {
        // WALL-04
        let checks = Checks::default();

        assert_eq!(
            next_job(&checks, NOV_19, &utc, false),
            ImageJob {
                not_before_ms: NOV_19,
                network: JobNetwork::Unmetered,
            }
        );
        assert_eq!(
            next_job(&checks, NOV_19, &utc, true).network,
            JobNetwork::Any
        );
    }

    #[test]
    fn the_back_off_never_overflows() {
        assert_eq!(back_off_ms(1), FIRST_BACK_OFF_MS);
        assert_eq!(back_off_ms(5), 16 * FIRST_BACK_OFF_MS);
        assert!(back_off_ms(u32::MAX) > DAY_MS);
    }
}
