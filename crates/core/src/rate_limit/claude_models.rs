//! Claude Code reports named weekly allowances in the same raw `limits`
//! list in its get_usage response and its local utilization cache.

use super::{claude::parse_iso8601_epoch, ProviderRateLimit, RateLimitMeter, RateLimitSource};
use serde_json::Value;
use std::hash::{BuildHasher, RandomState};
use std::sync::OnceLock;

pub fn weekly_meters(raw: Option<&Value>, now: f64) -> Option<Vec<RateLimitMeter>> {
    let raw = raw?;
    let rows = raw.as_array().into_iter().flatten().filter_map(|row| {
        if row.get("group")?.as_str()? != "weekly" {
            return None;
        }
        let name = row.pointer("/scope/model/display_name")?.as_str()?.trim();
        if name.is_empty() {
            return None;
        }
        let resets_at = row
            .get("resets_at")
            .and_then(|v| v.as_f64().or_else(|| parse_iso8601_epoch(v.as_str()?)));
        if resets_at.is_some_and(|at| at <= now) {
            return None;
        }
        Some(RateLimitMeter {
            id: name.into(),
            label: name.into(),
            utilization: row.get("percent")?.as_f64()?,
            resets_at,
            window_duration: Some(7.0 * 86_400.0),
            breakdown: Vec::new(),
        })
    });
    Some(rows.collect())
}

/// Only comparable within this process. No profile data in snapshots or diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccountScope(u64);

impl AccountScope {
    pub fn new(email: Option<&str>, organization: Option<&str>) -> Option<Self> {
        static SEED: OnceLock<RandomState> = OnceLock::new();
        let email = email.filter(|s| !s.is_empty())?;
        let key = (email, organization.filter(|s| !s.is_empty()));
        Some(Self(SEED.get_or_init(RandomState::new).hash_one(key)))
    }
}

/// The last named allowances observed live. A newer cache may withdraw a
/// model but its percentage can only be used for that cache's own snapshot.
pub struct LiveModelHistory {
    account: AccountScope,
    plan: Option<String>,
    observed_at: f64,
    meters: Vec<RateLimitMeter>,
}

impl LiveModelHistory {
    pub fn reconcile(last: &mut Option<Self>, snapshot: &mut ProviderRateLimit, now: f64) {
        let Some(account) = snapshot.claude_account else {
            // An unverified cache or a failed read cannot supply an identity.
            // Keep the live observation private until a later read verifies it.
            if snapshot.live_failure.is_none() && snapshot.source != Some(RateLimitSource::Cache) {
                *last = None;
            }
            return;
        };
        if last.as_ref().is_some_and(|read| {
            read.account != account
                || matches!(
                    (&read.plan, &snapshot.plan_label),
                    (Some(previous), Some(current)) if previous != current
                )
        }) {
            *last = None;
        }
        if let Some(meters) = &snapshot.model_scoped_meters {
            if snapshot.source == Some(RateLimitSource::Live) {
                *last = Some(Self {
                    account,
                    plan: snapshot.plan_label.clone(),
                    observed_at: snapshot.data_as_of.unwrap_or(now),
                    meters: meters.clone(),
                });
            } else if let Some(read) = last {
                if snapshot.data_as_of.is_some_and(|at| at >= read.observed_at) {
                    read.meters
                        .retain(|m| meters.iter().any(|reported| reported.id == m.id));
                }
            }
        } else if let Some(read) = last {
            snapshot.model_scoped_meters = Some(
                read.meters
                    .iter()
                    .filter(|m| m.resets_at.is_some_and(|at| at > now))
                    .cloned()
                    .collect(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RateLimitProvider, RateLimitStatus};
    use serde_json::json;

    #[test]
    fn weekly_rows_use_the_reported_name_percent_and_optional_reset() {
        let now = 1_785_297_599.0;
        let raw = json!([
            {"group":"weekly", "scope":{"model":{"display_name":"Past"}}, "percent":98, "resets_at":now - 1.0},
            {"group":"weekly", "scope":{"model":{"display_name":"Due"}}, "percent":99, "resets_at":now},
            {"group":"weekly", "scope":{"model":{"display_name":" Fable "}}, "percent":25.5, "resets_at":"2026-07-29T04:00:00Z"},
            {"group":"weekly", "scope":{"model":{"display_name":"Second"}}, "percent":0, "resets_at":1785297600},
            {"group":"weekly", "scope":{"model":{"display_name":"Undated"}}, "percent":12, "resets_at":null},
            {"group":"weekly", "percent":31},
            {"group":"daily", "scope":{"model":{"display_name":"Other"}}, "percent":42}
        ]);
        let meters = weekly_meters(Some(&raw), now).unwrap();
        let rows: Vec<_> = meters
            .iter()
            .map(|m| (m.label.as_str(), m.utilization, m.resets_at))
            .collect();
        assert_eq!(
            rows,
            [
                ("Fable", 25.5, Some(1_785_297_600.0)),
                ("Second", 0.0, Some(1_785_297_600.0)),
                ("Undated", 12.0, None),
            ]
        );
        assert!(meters.iter().all(|m| m.window_duration == Some(604_800.0)));
        assert_eq!(weekly_meters(Some(&json!([])), now), Some(vec![]));
        assert_eq!(weekly_meters(None, now), None);
    }

    #[test]
    fn only_an_unexpired_live_value_can_fill_an_omission_on_the_same_account() {
        let account = AccountScope::new(Some("a@example.invalid"), None);
        let mut live = ProviderRateLimit::empty(RateLimitProvider::ClaudeCode, RateLimitStatus::Ok);
        live.source = Some(RateLimitSource::Live);
        live.claude_account = account;
        live.plan_label = Some("Max".into());
        live.data_as_of = Some(100.0);
        let limits = json!([
            {"group":"weekly","scope":{"model":{"display_name":"Fable"}},"percent":30,"resets_at":200},
            {"group":"weekly","scope":{"model":{"display_name":"Undated"}},"percent":10}
        ]);
        live.model_scoped_meters = weekly_meters(Some(&limits), 100.0);
        let mut missing = live.clone();
        missing.model_scoped_meters = None;
        let mut history = None;
        LiveModelHistory::reconcile(&mut history, &mut live, 100.0);

        let mut cached = missing.clone();
        cached.source = Some(RateLimitSource::Cache);
        cached.claude_account = AccountScope::new(Some("a@example.invalid"), Some(""));
        LiveModelHistory::reconcile(&mut history, &mut cached, 150.0);
        let models = cached.model_scoped_meters.unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].label, "Fable");
        assert_eq!(models[0].utilization, 30.0);

        let mut expired = missing.clone();
        LiveModelHistory::reconcile(&mut history, &mut expired, 200.0);
        assert_eq!(expired.model_scoped_meters, Some(vec![]));

        let mut pro = missing.clone();
        pro.plan_label = Some("Pro".into());
        LiveModelHistory::reconcile(&mut history, &mut pro, 150.0);
        assert!(pro.model_scoped_meters.is_none());
        LiveModelHistory::reconcile(&mut history, &mut live, 150.0);

        let mut switched = missing.clone();
        switched.claude_account = AccountScope::new(Some("a@example.invalid"), Some("Other org"));
        LiveModelHistory::reconcile(&mut history, &mut switched, 150.0);
        assert!(switched.model_scoped_meters.is_none());
        assert!(history.is_none());
    }

    #[test]
    fn explicit_lists_withdraw_models_and_cache_percentages_are_never_carried() {
        let mut last = None;
        let mut live = ProviderRateLimit::empty(RateLimitProvider::ClaudeCode, RateLimitStatus::Ok);
        live.source = Some(RateLimitSource::Live);
        live.claude_account = AccountScope::new(Some("a@example.invalid"), None);
        live.data_as_of = Some(100.0);
        let mut missing = live.clone();
        let limits = json!([
            {"group":"weekly","scope":{"model":{"display_name":"Fable"}},"percent":30,"resets_at":200}
        ]);
        live.model_scoped_meters = weekly_meters(Some(&limits), 100.0);
        LiveModelHistory::reconcile(&mut last, &mut live, 100.0);
        let mut cached = live.clone();
        cached.source = Some(RateLimitSource::Cache);
        cached.data_as_of = Some(110.0);
        cached.model_scoped_meters.as_mut().unwrap()[0].utilization = 45.0;
        LiveModelHistory::reconcile(&mut last, &mut cached, 120.0);
        LiveModelHistory::reconcile(&mut last, &mut missing, 120.0);
        assert_eq!(missing.model_scoped_meters, live.model_scoped_meters);

        cached.model_scoped_meters = Some(vec![]);
        LiveModelHistory::reconcile(&mut last, &mut cached, 120.0);
        missing.model_scoped_meters = None;
        LiveModelHistory::reconcile(&mut last, &mut missing, 120.0);
        assert_eq!(missing.model_scoped_meters, Some(vec![]));
    }
}
