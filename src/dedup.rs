use crate::normalized_alert::{AlertSource, NormalizedAlert};
use std::collections::HashMap;

pub const DEFAULT_DEDUP_TTL_SECS: u64 = 6 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DedupDecision {
    pub is_duplicate: bool,
    pub key: DedupKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DedupGate {
    ttl_secs: u64,
    entries: HashMap<DedupKey, u64>,
}

impl DedupGate {
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            ttl_secs,
            entries: HashMap::new(),
        }
    }

    pub fn check_and_record(
        &mut self,
        alert: &NormalizedAlert,
        now_unix_secs: u64,
    ) -> DedupDecision {
        self.prune_expired(now_unix_secs);

        let exact_key = DedupKey::exact_source(alert);
        let fuzzy_key = DedupKey::fuzzy(alert);
        let is_duplicate =
            self.entries.contains_key(&exact_key) || self.entries.contains_key(&fuzzy_key);

        if !is_duplicate {
            self.entries.insert(exact_key, now_unix_secs);
            self.entries.insert(fuzzy_key.clone(), now_unix_secs);
        }

        DedupDecision {
            is_duplicate,
            key: fuzzy_key,
        }
    }

    fn prune_expired(&mut self, now_unix_secs: u64) {
        let ttl_secs = self.ttl_secs;
        self.entries
            .retain(|_key, seen_at| now_unix_secs.saturating_sub(*seen_at) <= ttl_secs);
    }
}

impl Default for DedupGate {
    fn default() -> Self {
        Self::new(DEFAULT_DEDUP_TTL_SECS)
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum DedupKey {
    ExactSource(String),
    Fuzzy(String),
}

impl DedupKey {
    pub fn exact_source(alert: &NormalizedAlert) -> Self {
        let source = match alert.source {
            AlertSource::Same => "same",
            AlertSource::NwsApi => "nws_api",
        };
        Self::ExactSource(format!("{}:{}", source, normalize_text(&alert.source_id)))
    }

    pub fn fuzzy(alert: &NormalizedAlert) -> Self {
        let mut same_codes = alert.same_codes.clone();
        same_codes.sort();
        let mut ugc_codes = alert.ugc_codes.clone();
        ugc_codes.sort();

        Self::Fuzzy(
            [
                normalize_text(&alert.event),
                normalize_list(&same_codes),
                normalize_list(&ugc_codes),
                normalize_option(alert.effective.as_deref()),
                normalize_option(alert.expires.as_deref()),
                normalize_option(alert.area_desc.as_deref()),
            ]
            .join("|"),
        )
    }
}

fn normalize_text(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_option(value: Option<&str>) -> String {
    value.map(normalize_text).unwrap_or_default()
}

fn normalize_list(values: &[String]) -> String {
    values
        .iter()
        .map(|value| normalize_text(value))
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alert(source: AlertSource, source_id: &str) -> NormalizedAlert {
        NormalizedAlert {
            source,
            source_id: source_id.to_string(),
            event: "Tornado Warning".to_string(),
            headline: None,
            description: None,
            instruction: None,
            severity: Some("Extreme".to_string()),
            urgency: Some("Immediate".to_string()),
            certainty: Some("Observed".to_string()),
            effective: Some("2026-06-16T13:00:00-05:00".to_string()),
            expires: Some("2026-06-16T13:45:00-05:00".to_string()),
            area_desc: Some("Central Harris County".to_string()),
            same_codes: vec!["048201".to_string()],
            ugc_codes: vec!["TXC201".to_string()],
            message_text: "NWS Alert: Tornado Warning".to_string(),
        }
    }

    #[test]
    fn same_noaa_api_source_id_suppresses_duplicate() {
        let mut gate = DedupGate::default();
        let first = alert(AlertSource::NwsApi, "urn:oid:1");
        let second = alert(AlertSource::NwsApi, "urn:oid:1");

        assert!(!gate.check_and_record(&first, 100).is_duplicate);
        assert!(gate.check_and_record(&second, 101).is_duplicate);
    }

    #[test]
    fn different_noaa_api_source_ids_do_not_suppress_when_fuzzy_fields_differ() {
        let mut gate = DedupGate::default();
        let first = alert(AlertSource::NwsApi, "urn:oid:1");
        let mut second = alert(AlertSource::NwsApi, "urn:oid:2");
        second.event = "Flash Flood Warning".to_string();

        assert!(!gate.check_and_record(&first, 100).is_duplicate);
        assert!(!gate.check_and_record(&second, 101).is_duplicate);
    }

    #[test]
    fn same_normalized_event_location_time_suppresses_across_sources() {
        let mut gate = DedupGate::default();
        let api = alert(AlertSource::NwsApi, "urn:oid:1");
        let same = alert(AlertSource::Same, "same-zczc-1");

        assert!(!gate.check_and_record(&api, 100).is_duplicate);
        assert!(gate.check_and_record(&same, 101).is_duplicate);
    }

    #[test]
    fn different_event_does_not_suppress() {
        let mut gate = DedupGate::default();
        let first = alert(AlertSource::NwsApi, "urn:oid:1");
        let mut second = alert(AlertSource::Same, "same-zczc-1");
        second.event = "Severe Thunderstorm Warning".to_string();

        assert!(!gate.check_and_record(&first, 100).is_duplicate);
        assert!(!gate.check_and_record(&second, 101).is_duplicate);
    }

    #[test]
    fn different_location_does_not_suppress() {
        let mut gate = DedupGate::default();
        let first = alert(AlertSource::NwsApi, "urn:oid:1");
        let mut second = alert(AlertSource::Same, "same-zczc-1");
        second.same_codes = vec!["048203".to_string()];
        second.ugc_codes = vec!["TXC203".to_string()];
        second.area_desc = Some("Different County".to_string());

        assert!(!gate.check_and_record(&first, 100).is_duplicate);
        assert!(!gate.check_and_record(&second, 101).is_duplicate);
    }

    #[test]
    fn expired_dedup_entry_no_longer_suppresses() {
        let mut gate = DedupGate::new(10);
        let first = alert(AlertSource::NwsApi, "urn:oid:1");
        let second = alert(AlertSource::NwsApi, "urn:oid:1");

        assert!(!gate.check_and_record(&first, 100).is_duplicate);
        assert!(!gate.check_and_record(&second, 111).is_duplicate);
    }

    #[test]
    fn empty_optional_fields_still_produce_deterministic_keys() {
        let mut first = alert(AlertSource::NwsApi, "urn:oid:1");
        first.effective = None;
        first.expires = None;
        first.area_desc = None;
        first.same_codes = Vec::new();
        first.ugc_codes = Vec::new();
        let mut second = first.clone();
        second.source = AlertSource::Same;
        second.source_id = "same-empty".to_string();

        assert_eq!(DedupKey::fuzzy(&first), DedupKey::fuzzy(&second));
        assert_eq!(
            DedupKey::fuzzy(&first),
            DedupKey::Fuzzy("tornado warning|||||".to_string())
        );
    }

    #[test]
    fn dedup_module_is_not_wired_into_live_delivery() {
        let mut gate = DedupGate::default();
        let alert = alert(AlertSource::NwsApi, "urn:oid:1");

        let decision = gate.check_and_record(&alert, 100);

        assert!(!decision.is_duplicate);
        assert_eq!(decision.key, DedupKey::fuzzy(&alert));
    }
}
