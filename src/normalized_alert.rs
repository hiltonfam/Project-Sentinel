use crate::nws_client::NwsAlertFeature;
use anyhow::{Context, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlertSource {
    Same,
    NwsApi,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedAlert {
    pub source: AlertSource,
    pub source_id: String,
    pub event: String,
    pub headline: Option<String>,
    pub description: Option<String>,
    pub instruction: Option<String>,
    pub severity: Option<String>,
    pub urgency: Option<String>,
    pub certainty: Option<String>,
    pub effective: Option<String>,
    pub expires: Option<String>,
    pub area_desc: Option<String>,
    pub same_codes: Vec<String>,
    pub ugc_codes: Vec<String>,
    pub message_text: String,
}

impl NormalizedAlert {
    pub fn from_nws_alert(feature: &NwsAlertFeature) -> Result<Self> {
        let properties = &feature.properties;
        let source_id = properties
            .id
            .as_deref()
            .unwrap_or(&feature.id)
            .trim()
            .to_string();
        let event = properties
            .event
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .context("NWS alert event is required for normalization")?
            .to_string();
        let same_codes = properties
            .geocode
            .as_ref()
            .and_then(|geocode| geocode.same.clone())
            .unwrap_or_default();
        let ugc_codes = properties
            .geocode
            .as_ref()
            .and_then(|geocode| geocode.ugc.clone())
            .unwrap_or_default();

        let mut alert = Self {
            source: AlertSource::NwsApi,
            source_id,
            event,
            headline: properties.headline.clone(),
            description: properties.description.clone(),
            instruction: properties.instruction.clone(),
            severity: properties.severity.clone(),
            urgency: properties.urgency.clone(),
            certainty: properties.certainty.clone(),
            effective: properties.effective.clone(),
            expires: properties.expires.clone(),
            area_desc: properties.area_desc.clone(),
            same_codes,
            ugc_codes,
            message_text: String::new(),
        };
        alert.message_text = format_nws_message(&alert);

        Ok(alert)
    }
}

pub fn format_nws_message(alert: &NormalizedAlert) -> String {
    let mut lines = vec![format!("NWS Alert: {}", alert.event)];

    push_optional_line(&mut lines, "Area", alert.area_desc.as_deref());
    push_optional_line(&mut lines, "Severity", alert.severity.as_deref());
    push_optional_line(&mut lines, "Urgency", alert.urgency.as_deref());
    push_optional_line(&mut lines, "Certainty", alert.certainty.as_deref());
    push_optional_line(&mut lines, "Headline", alert.headline.as_deref());
    push_optional_line(&mut lines, "Description", alert.description.as_deref());
    push_optional_line(&mut lines, "Instruction", alert.instruction.as_deref());

    lines.join("\n")
}

fn push_optional_line(lines: &mut Vec<String>, label: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        lines.push(format!("{}: {}", label, value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nws_client::{parse_active_alerts, NwsAlertCollection};

    const MINIMAL_ACTIVE_ALERTS: &str =
        include_str!("../tests/fixtures/nws_active_alerts_minimal.json");
    const MISSING_OPTIONAL_ACTIVE_ALERTS: &str =
        include_str!("../tests/fixtures/nws_active_alerts_missing_optional.json");
    const MISSING_EVENT_ACTIVE_ALERTS: &str =
        include_str!("../tests/fixtures/nws_active_alerts_missing_event.json");

    fn first_alert(fixture: &str) -> NwsAlertFeature {
        parse_active_alerts(fixture)
            .unwrap()
            .features
            .into_iter()
            .next()
            .unwrap()
    }

    #[test]
    fn full_nws_alert_normalizes_correctly() {
        let feature = first_alert(MINIMAL_ACTIVE_ALERTS);

        let alert = NormalizedAlert::from_nws_alert(&feature).unwrap();

        assert_eq!(alert.source, AlertSource::NwsApi);
        assert_eq!(alert.source_id, "urn:oid:1");
        assert_eq!(alert.event, "Tornado Warning");
        assert_eq!(
            alert.headline.as_deref(),
            Some("Tornado Warning issued June 16 at 1:00PM CDT")
        );
        assert_eq!(
            alert.description.as_deref(),
            Some("A tornado warning is in effect.")
        );
        assert_eq!(alert.instruction.as_deref(), Some("Take shelter now."));
        assert_eq!(alert.severity.as_deref(), Some("Extreme"));
        assert_eq!(alert.urgency.as_deref(), Some("Immediate"));
        assert_eq!(alert.certainty.as_deref(), Some("Observed"));
        assert_eq!(
            alert.effective.as_deref(),
            Some("2026-06-16T13:00:00-05:00")
        );
        assert_eq!(alert.expires.as_deref(), Some("2026-06-16T13:45:00-05:00"));
        assert_eq!(alert.area_desc.as_deref(), Some("Central Harris County"));
    }

    #[test]
    fn missing_optional_fields_still_normalize() {
        let feature = first_alert(MISSING_OPTIONAL_ACTIVE_ALERTS);

        let alert = NormalizedAlert::from_nws_alert(&feature).unwrap();

        assert_eq!(alert.event, "Severe Thunderstorm Warning");
        assert_eq!(alert.headline, None);
        assert_eq!(alert.description, None);
        assert_eq!(alert.same_codes, Vec::<String>::new());
        assert_eq!(alert.ugc_codes, Vec::<String>::new());
    }

    #[test]
    fn missing_event_returns_clear_error() {
        let feature = first_alert(MISSING_EVENT_ACTIVE_ALERTS);

        let error = NormalizedAlert::from_nws_alert(&feature).unwrap_err();

        assert!(error
            .to_string()
            .contains("NWS alert event is required for normalization"));
    }

    #[test]
    fn properties_id_is_preferred_over_feature_id() {
        let feature = first_alert(MINIMAL_ACTIVE_ALERTS);

        let alert = NormalizedAlert::from_nws_alert(&feature).unwrap();

        assert_eq!(alert.source_id, "urn:oid:1");
    }

    #[test]
    fn feature_id_fallback_works() {
        let collection: NwsAlertCollection = parse_active_alerts(
            r#"{
                "features": [{
                    "id": "https://api.weather.gov/alerts/fallback",
                    "properties": {
                        "event": "Flood Warning"
                    }
                }]
            }"#,
        )
        .unwrap();

        let alert = NormalizedAlert::from_nws_alert(&collection.features[0]).unwrap();

        assert_eq!(alert.source_id, "https://api.weather.gov/alerts/fallback");
    }

    #[test]
    fn same_and_ugc_codes_are_preserved() {
        let feature = first_alert(MINIMAL_ACTIVE_ALERTS);

        let alert = NormalizedAlert::from_nws_alert(&feature).unwrap();

        assert_eq!(alert.same_codes, vec!["048201".to_string()]);
        assert_eq!(alert.ugc_codes, vec!["TXC201".to_string()]);
    }

    #[test]
    fn message_formatter_omits_missing_fields() {
        let feature = first_alert(MISSING_OPTIONAL_ACTIVE_ALERTS);

        let alert = NormalizedAlert::from_nws_alert(&feature).unwrap();

        assert_eq!(alert.message_text, "NWS Alert: Severe Thunderstorm Warning");
        assert!(!alert.message_text.contains("Headline:"));
        assert!(!alert.message_text.contains("Description:"));
    }

    #[test]
    fn message_formatter_preserves_deterministic_order() {
        let feature = first_alert(MINIMAL_ACTIVE_ALERTS);

        let alert = NormalizedAlert::from_nws_alert(&feature).unwrap();

        assert_eq!(
            alert.message_text,
            "NWS Alert: Tornado Warning\n\
             Area: Central Harris County\n\
             Severity: Extreme\n\
             Urgency: Immediate\n\
             Certainty: Observed\n\
             Headline: Tornado Warning issued June 16 at 1:00PM CDT\n\
             Description: A tornado warning is in effect.\n\
             Instruction: Take shelter now."
        );
    }

    #[test]
    fn normalization_does_not_invoke_fanout_or_sender_behavior() {
        let feature = first_alert(MINIMAL_ACTIVE_ALERTS);

        let alert = NormalizedAlert::from_nws_alert(&feature).unwrap();

        assert_eq!(alert.source, AlertSource::NwsApi);
        assert_eq!(
            alert.message_text.lines().next(),
            Some("NWS Alert: Tornado Warning")
        );
    }
}
