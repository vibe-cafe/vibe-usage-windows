//! Source priority for the Windows Claude quota card.
use super::{
    ProviderRateLimit, RateLimitEmptyReason, RateLimitLiveFailure, RateLimitProvider,
    RateLimitSource, RateLimitStatus,
};

pub fn resolve_read(
    now: f64,
    live: Result<ProviderRateLimit, RateLimitLiveFailure>,
    cache: impl FnOnce() -> Option<ProviderRateLimit>,
    desktop: impl FnOnce() -> Option<ProviderRateLimit>,
) -> ProviderRateLimit {
    let failure = match live {
        Ok(answer) => return answer,
        Err(failure) => failure,
    };
    let policy = failure == RateLimitLiveFailure::ClaudeBlockedByPolicy;
    if !policy {
        if let Some(cached) = cache() {
            return after_live_failure(failure, Some(cached));
        }
    }
    if let Some(mut snapshot) = desktop() {
        if snapshot.status == RateLimitStatus::Ok {
            snapshot.live_failure = Some(failure);
            return snapshot;
        }
        if failure == RateLimitLiveFailure::ClaudeBinaryNotFound {
            return snapshot;
        }
    }
    if policy {
        nonessential_traffic_disabled_snapshot(now)
    } else {
        after_live_failure(failure, None)
    }
}

/// The card while the organization's policy forces nonessential traffic off:
/// an empty state that names the policy. No source, no failure, no numbers.
pub fn nonessential_traffic_disabled_snapshot(now: f64) -> ProviderRateLimit {
    let mut snapshot =
        ProviderRateLimit::empty(RateLimitProvider::ClaudeCode, RateLimitStatus::NoData);
    snapshot.empty_reason = Some(RateLimitEmptyReason::NonessentialTrafficDisabled);
    snapshot.fetched_at = Some(now);
    snapshot
}

/// What the card shows after the live read failed: Claude Code's own cache
/// when it has a usable entry, annotated with why live failed; otherwise the
/// failure itself. No program at all is not something a retry fixes, so it
/// stays an empty state; every other failure is retryable, as on macOS.
pub fn after_live_failure(
    failure: RateLimitLiveFailure,
    cached: Option<ProviderRateLimit>,
) -> ProviderRateLimit {
    if let Some(mut snapshot) = cached {
        snapshot.live_failure = Some(failure);
        return snapshot;
    }
    // Nothing to retry: no program, or a read paused on purpose.
    if failure == RateLimitLiveFailure::ClaudeBinaryNotFound || failure.is_pause() {
        return ProviderRateLimit::failed(
            RateLimitProvider::ClaudeCode,
            RateLimitStatus::NoData,
            None,
            failure,
        );
    }
    ProviderRateLimit::failed(
        RateLimitProvider::ClaudeCode,
        RateLimitStatus::RetryableError,
        Some(RateLimitSource::Live),
        failure,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use RateLimitLiveFailure::*;

    fn sample(source: RateLimitSource) -> ProviderRateLimit {
        let mut card = ProviderRateLimit::empty(RateLimitProvider::ClaudeCode, RateLimitStatus::Ok);
        card.source = Some(source);
        card
    }

    #[test]
    fn live_answer_including_no_plan_skips_both_offline_sources() {
        let mut no_plan =
            ProviderRateLimit::empty(RateLimitProvider::ClaudeCode, RateLimitStatus::NoData);
        no_plan.empty_reason = Some(RateLimitEmptyReason::SessionWithoutPlanLimits);
        for live in [sample(RateLimitSource::Live), no_plan] {
            assert_eq!(
                resolve_read(
                    0.,
                    Ok(live.clone()),
                    || panic!("cache"),
                    || panic!("Desktop")
                ),
                live
            );
        }
    }

    #[test]
    fn failure_tries_account_cache_then_desktop_and_keeps_the_live_reason() {
        let cached = resolve_read(
            0.,
            Err(ClaudeTimedOut),
            || Some(sample(RateLimitSource::Cache)),
            || panic!("Desktop"),
        );
        assert_eq!(cached.source, Some(RateLimitSource::Cache));
        assert_eq!(cached.live_failure, Some(ClaudeTimedOut));
        for reason in [ClaudeBinaryNotFound, ClaudeTimedOut, EgressUnconfirmed] {
            let fallback = resolve_read(
                0.,
                Err(reason),
                || None,
                || Some(sample(RateLimitSource::DesktopRecord)),
            );
            assert_eq!(fallback.status, RateLimitStatus::Ok);
            assert_eq!(fallback.source, Some(RateLimitSource::DesktopRecord));
            assert_eq!(fallback.live_failure, Some(reason));
        }
    }

    #[test]
    fn broken_desktop_record_explains_absence_without_masking_a_pause_or_retry() {
        let stale = || {
            Some(ProviderRateLimit::failed(
                RateLimitProvider::ClaudeCode,
                RateLimitStatus::NoData,
                Some(RateLimitSource::DesktopRecord),
                DesktopRecordStale,
            ))
        };
        assert_eq!(
            resolve_read(0., Err(ClaudeBinaryNotFound), || None, stale).live_failure,
            Some(DesktopRecordStale)
        );
        assert_eq!(
            resolve_read(0., Err(ClaudeTimedOut), || None, stale).status,
            RateLimitStatus::RetryableError
        );
        assert_eq!(
            resolve_read(0., Err(EgressUnconfirmed), || None, stale).live_failure,
            Some(EgressUnconfirmed)
        );
    }

    #[test]
    fn forced_policy_permits_only_a_valid_desktop_record() {
        let card = resolve_read(
            0.,
            Err(ClaudeBlockedByPolicy),
            || panic!("cache"),
            || Some(sample(RateLimitSource::DesktopRecord)),
        );
        assert_eq!(card.live_failure, Some(ClaudeBlockedByPolicy));
        let empty = resolve_read(0., Err(ClaudeBlockedByPolicy), || panic!("cache"), || None);
        assert_eq!(
            empty.empty_reason,
            Some(RateLimitEmptyReason::NonessentialTrafficDisabled)
        );
    }
}
