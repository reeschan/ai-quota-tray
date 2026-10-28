use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::model::Snapshot;
use crate::money::parse_major_to_cents;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Alert {
    pub provider: String,
    pub key: String,
    pub message: String,
}

/// Fire when a reported percentage or balance crosses a configured line.
///
/// A latch stops repeat notifications until the value falls back under the line.
/// Nothing is emitted for snapshots that have no real window or balance.
pub fn crossing_alerts(
    current: &Snapshot,
    used_pct_threshold: Option<f64>,
    balance_below: Option<&str>,
    balance_currency: &str,
    latched: &BTreeSet<String>,
) -> (Vec<Alert>, BTreeSet<String>) {
    let mut next = BTreeSet::new();
    let mut observed = BTreeSet::new();
    let mut alerts = Vec::new();
    if let Some(threshold) = used_pct_threshold {
        for window in &current.windows {
            let Some(used) = window.used_pct else {
                continue;
            };
            let key = format!("{}|pct|{}", current.provider, window.label);
            observed.insert(key.clone());
            if used >= threshold {
                next.insert(key.clone());
                if !latched.contains(&key) {
                    alerts.push(Alert {
                        provider: current.provider.clone(),
                        key: key.clone(),
                        message: format!(
                            "{} の「{}」が {used:.0}% です（しきい値 {threshold:.0}%）",
                            current.display_name, window.label
                        ),
                    });
                }
            }
        }
    }
    if let (Some(limit_text), Some(balance)) = (balance_below, current.balance.as_ref()) {
        if balance.currency.eq_ignore_ascii_case(balance_currency) {
            if let (Ok(limit), Ok(amount)) = (
                parse_major_to_cents(limit_text),
                parse_major_to_cents(&balance.amount),
            ) {
                let key = format!("{}|balance", current.provider);
                observed.insert(key.clone());
                if amount < limit {
                    next.insert(key.clone());
                    if !latched.contains(&key) {
                        alerts.push(Alert {
                            provider: current.provider.clone(),
                            key,
                            message: format!(
                                "{} の残高が {} {} を下回りました（現在 {} {}）",
                                current.display_name,
                                limit_text,
                                balance.currency,
                                balance.amount,
                                balance.currency
                            ),
                        });
                    }
                }
            }
        }
    }
    // A partial snapshot must not look like a recovery. Keep latches for
    // enabled metrics that this snapshot did not report.
    let pct_prefix = format!("{}|pct|", current.provider);
    let balance_key = format!("{}|balance", current.provider);
    for key in latched {
        let enabled = if key.starts_with(&pct_prefix) {
            used_pct_threshold.is_some()
        } else if key == &balance_key {
            balance_below.is_some()
        } else {
            false
        };
        if enabled && !observed.contains(key) {
            next.insert(key.clone());
        }
    }
    (alerts, next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Source, UsageWindow};
    use crate::money::Money;
    use chrono::{TimeZone, Utc};

    fn snap() -> Snapshot {
        let mut snapshot = Snapshot::bare(
            "deepseek",
            Source::Official,
            Utc.with_ymd_and_hms(2026, 10, 28, 0, 0, 0).unwrap(),
        );
        snapshot.windows.push(UsageWindow {
            label: "5時間".into(),
            used_pct: Some(81.0),
            resets_at: None,
        });
        snapshot.balance = Some(Money::from_cents(400, "USD"));
        snapshot
    }

    #[test]
    fn alerts_once_until_the_value_recovers() {
        let snapshot = snap();
        let (alerts, latched) =
            crossing_alerts(&snapshot, Some(80.0), Some("5.00"), "USD", &BTreeSet::new());
        assert_eq!(alerts.len(), 2);
        let (again, _) = crossing_alerts(&snapshot, Some(80.0), Some("5.00"), "USD", &latched);
        assert!(again.is_empty());

        let mut recovered = snapshot.clone();
        recovered.windows[0].used_pct = Some(10.0);
        recovered.balance = Some(Money::from_cents(2_000, "USD"));
        let (_, cleared) = crossing_alerts(&recovered, Some(80.0), Some("5.00"), "USD", &latched);
        assert!(cleared.is_empty());
    }

    #[test]
    fn omitted_percentage_keeps_the_existing_latch() {
        let snapshot = snap();
        let (_, latched) = crossing_alerts(&snapshot, Some(80.0), None, "USD", &BTreeSet::new());
        let mut partial = snapshot.clone();
        partial.windows[0].used_pct = None;
        let (alerts, next) = crossing_alerts(&partial, Some(80.0), None, "USD", &latched);
        assert!(alerts.is_empty());
        assert!(next.contains("deepseek|pct|5時間"));
    }

    #[test]
    fn ignores_other_currencies_and_missing_percentages() {
        let mut snapshot = snap();
        snapshot.balance = Some(Money::from_cents(10, "CNY"));
        snapshot.windows[0].used_pct = None;
        let (alerts, _) =
            crossing_alerts(&snapshot, Some(80.0), Some("5.00"), "USD", &BTreeSet::new());
        assert!(alerts.is_empty());
    }
}
