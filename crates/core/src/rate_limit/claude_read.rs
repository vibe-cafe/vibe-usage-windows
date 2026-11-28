//! Source priority for the Windows Claude quota card.
use super::{
    ProviderRateLimit, RateLimitEmptyReason, RateLimitLiveFailure, RateLimitProvider,
    RateLimitSource, RateLimitStatus,
};
use crate::claude_code::LaunchOrigin;

/// Desktop's bundled CLI can have a different login from the Desktop app.
pub fn candidate_answer(
    origin: LaunchOrigin,
    answer: ProviderRateLimit,
) -> Result<ProviderRateLimit, RateLimitLiveFailure> {
    if origin == LaunchOrigin::Desktop
        && answer.empty_reason == Some(RateLimitEmptyReason::SessionWithoutPlanLimits)
    {
        return Err(RateLimitLiveFailure::ClaudeNoWindows);
    }
    Ok(answer)
}

/// With no failure yet, return only usable numbers for the initial preview.
pub fn offline_snapshot(
    failure: Option<RateLimitLiveFailure>,
    cache: impl FnOnce() -> Option<ProviderRateLimit>,
    desktop: impl FnOnce() -> Option<ProviderRateLimit>,
) -> Option<ProviderRateLimit> {
    if failure != Some(RateLimitLiveFailure::ClaudeBlockedByPolicy) {
        if let Some(mut cached) = cache() {
            cached.live_failure = failure;
            return Some(cached);
        }
    }
    let mut snapshot = desktop()?;
    if snapshot.status == RateLimitStatus::Ok {
        snapshot.live_failure = failure;
    } else if failure != Some(RateLimitLiveFailure::ClaudeBinaryNotFound) {
        return None;
    }
    Some(snapshot)
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

pub fn empty_after_failure(now: f64, failure: RateLimitLiveFailure) -> ProviderRateLimit {
    if failure == RateLimitLiveFailure::ClaudeBlockedByPolicy {
        return nonessential_traffic_disabled_snapshot(now);
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
    fn desktop_no_plan_allows_fallback_but_standalone_no_plan_is_final() {
        let mut no_plan =
            ProviderRateLimit::empty(RateLimitProvider::ClaudeCode, RateLimitStatus::NoData);
        no_plan.empty_reason = Some(RateLimitEmptyReason::SessionWithoutPlanLimits);
        assert_eq!(
            candidate_answer(LaunchOrigin::LocalBin, no_plan.clone()),
            Ok(no_plan.clone())
        );
        let live = sample(RateLimitSource::Live);
        assert_eq!(
            candidate_answer(LaunchOrigin::Desktop, live.clone()),
            Ok(live)
        );
        let failure = candidate_answer(LaunchOrigin::Desktop, no_plan).unwrap_err();
        assert_eq!(failure, ClaudeNoWindows);
        let desktop = sample(RateLimitSource::DesktopRecord);
        let shown = offline_snapshot(Some(failure), || None, || Some(desktop)).unwrap();
        assert_eq!(shown.source, Some(RateLimitSource::DesktopRecord));
        assert_eq!(shown.live_failure, Some(failure));
    }

    #[test]
    fn preview_prefers_cache_then_valid_desktop_numbers() {
        let cache = sample(RateLimitSource::Cache);
        let desktop = sample(RateLimitSource::DesktopRecord);
        assert_eq!(
            offline_snapshot(None, || Some(cache.clone()), || panic!("Desktop")),
            Some(cache)
        );
        assert_eq!(
            offline_snapshot(None, || None, || Some(desktop.clone())),
            Some(desktop)
        );
        let stale = ProviderRateLimit::failed(
            RateLimitProvider::ClaudeCode,
            RateLimitStatus::NoData,
            Some(RateLimitSource::DesktopRecord),
            DesktopRecordStale,
        );
        assert!(offline_snapshot(None, || None, || Some(stale)).is_none());
    }

    #[test]
    fn failure_tries_account_cache_then_desktop_and_keeps_the_live_reason() {
        let cached = offline_snapshot(
            Some(ClaudeTimedOut),
            || Some(sample(RateLimitSource::Cache)),
            || panic!("Desktop"),
        )
        .unwrap();
        assert_eq!(cached.source, Some(RateLimitSource::Cache));
        assert_eq!(cached.live_failure, Some(ClaudeTimedOut));
        for reason in [
            ClaudeBinaryNotFound,
            ClaudeNoWindows,
            ClaudeTimedOut,
            EgressUnconfirmed,
        ] {
            let fallback = offline_snapshot(
                Some(reason),
                || None,
                || Some(sample(RateLimitSource::DesktopRecord)),
            )
            .unwrap();
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
            offline_snapshot(Some(ClaudeBinaryNotFound), || None, stale)
                .unwrap()
                .live_failure,
            Some(DesktopRecordStale)
        );
        assert!(offline_snapshot(Some(ClaudeTimedOut), || None, stale).is_none());
        assert!(offline_snapshot(Some(EgressUnconfirmed), || None, stale).is_none());
    }

    #[test]
    fn forced_policy_permits_only_a_valid_desktop_record() {
        let card = offline_snapshot(
            Some(ClaudeBlockedByPolicy),
            || panic!("cache"),
            || Some(sample(RateLimitSource::DesktopRecord)),
        )
        .unwrap();
        assert_eq!(card.live_failure, Some(ClaudeBlockedByPolicy));
        assert!(
            offline_snapshot(Some(ClaudeBlockedByPolicy), || panic!("cache"), || None).is_none()
        );
        let empty = empty_after_failure(0., ClaudeBlockedByPolicy);
        assert_eq!(
            empty.empty_reason,
            Some(RateLimitEmptyReason::NonessentialTrafficDisabled)
        );
    }
}
