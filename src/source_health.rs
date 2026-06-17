use crate::event_contracts::{SourceState, SourceStatusRecord, EVENT_CONTRACT_SCHEMA_VERSION};

pub const SOURCE_HEALTH_DEGRADED_AFTER_SECS: u64 = 5 * 60;
pub const SOURCE_HEALTH_OFFLINE_AFTER_SECS: u64 = 15 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    NwsApi,
    SameRadio,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NwsApi => "nws_api",
            Self::SameRadio => "same_radio",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceHealthInput {
    pub source: SourceKind,
    pub now_unix_secs: u64,
    pub last_success_unix_secs: Option<u64>,
    pub last_failure_unix_secs: Option<u64>,
    pub last_decoded_message_unix_secs: Option<u64>,
    pub last_accepted_alert_unix_secs: Option<u64>,
    pub error: Option<String>,
}

impl SourceHealthInput {
    #[cfg(test)]
    pub fn nws_api_success(now_unix_secs: u64) -> Self {
        Self {
            source: SourceKind::NwsApi,
            now_unix_secs,
            last_success_unix_secs: Some(now_unix_secs),
            last_failure_unix_secs: None,
            last_decoded_message_unix_secs: None,
            last_accepted_alert_unix_secs: None,
            error: None,
        }
    }

    #[cfg(test)]
    pub fn nws_api_failure(now_unix_secs: u64, error: String) -> Self {
        Self {
            source: SourceKind::NwsApi,
            now_unix_secs,
            last_success_unix_secs: None,
            last_failure_unix_secs: Some(now_unix_secs),
            last_decoded_message_unix_secs: None,
            last_accepted_alert_unix_secs: None,
            error: Some(error),
        }
    }

    pub fn same_radio(
        now_unix_secs: u64,
        last_decoded_message_unix_secs: Option<u64>,
        last_accepted_alert_unix_secs: Option<u64>,
    ) -> Self {
        Self {
            source: SourceKind::SameRadio,
            now_unix_secs,
            last_success_unix_secs: last_decoded_message_unix_secs,
            last_failure_unix_secs: None,
            last_decoded_message_unix_secs,
            last_accepted_alert_unix_secs,
            error: None,
        }
    }
}

pub fn source_state(now_unix_secs: u64, last_observed_unix_secs: Option<u64>) -> SourceState {
    let Some(last_observed_unix_secs) = last_observed_unix_secs else {
        return SourceState::Unknown;
    };

    let age_secs = now_unix_secs.saturating_sub(last_observed_unix_secs);
    if age_secs <= SOURCE_HEALTH_DEGRADED_AFTER_SECS {
        SourceState::Healthy
    } else if age_secs <= SOURCE_HEALTH_OFFLINE_AFTER_SECS {
        SourceState::Degraded
    } else {
        SourceState::Offline
    }
}

pub fn source_status_record(input: SourceHealthInput) -> SourceStatusRecord {
    let last_observed = match input.source {
        SourceKind::NwsApi => input.last_success_unix_secs,
        SourceKind::SameRadio => input
            .last_decoded_message_unix_secs
            .or(input.last_accepted_alert_unix_secs),
    };

    SourceStatusRecord {
        schema_version: EVENT_CONTRACT_SCHEMA_VERSION,
        record_type: "source_status".to_string(),
        timestamp_unix_secs: input.now_unix_secs,
        source: input.source.as_str().to_string(),
        state: source_state(input.now_unix_secs, last_observed),
        last_success_unix_secs: input.last_success_unix_secs,
        last_failure_unix_secs: input.last_failure_unix_secs,
        last_decoded_message_unix_secs: input.last_decoded_message_unix_secs,
        last_accepted_alert_unix_secs: input.last_accepted_alert_unix_secs,
        error: input.error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_state_is_unknown_without_success() {
        assert_eq!(source_state(1_000, None), SourceState::Unknown);
    }

    #[test]
    fn api_state_is_healthy_for_recent_success() {
        assert_eq!(source_state(1_000, Some(760)), SourceState::Healthy);
    }

    #[test]
    fn api_success_record_is_healthy() {
        let record = source_status_record(SourceHealthInput::nws_api_success(1_000));

        assert_eq!(record.source, "nws_api");
        assert_eq!(record.state, SourceState::Healthy);
        assert_eq!(record.last_success_unix_secs, Some(1_000));
        assert_eq!(record.error, None);
    }

    #[test]
    fn api_state_is_degraded_after_five_minutes_without_success() {
        assert_eq!(source_state(1_000, Some(699)), SourceState::Degraded);
    }

    #[test]
    fn api_state_is_offline_after_fifteen_minutes_without_success() {
        assert_eq!(source_state(1_000, Some(99)), SourceState::Offline);
    }

    #[test]
    fn same_state_uses_last_decoded_message() {
        let record = source_status_record(SourceHealthInput::same_radio(1_000, Some(800), None));

        assert_eq!(record.source, "same_radio");
        assert_eq!(record.state, SourceState::Healthy);
        assert_eq!(record.last_decoded_message_unix_secs, Some(800));
    }

    #[test]
    fn same_state_uses_last_accepted_alert_when_decode_time_is_missing() {
        let record = source_status_record(SourceHealthInput::same_radio(1_000, None, Some(500)));

        assert_eq!(record.state, SourceState::Degraded);
        assert_eq!(record.last_accepted_alert_unix_secs, Some(500));
    }

    #[test]
    fn failure_record_preserves_error_summary() {
        let record = source_status_record(SourceHealthInput::nws_api_failure(
            1_000,
            "network down".into(),
        ));

        assert_eq!(record.source, "nws_api");
        assert_eq!(record.state, SourceState::Unknown);
        assert_eq!(record.last_failure_unix_secs, Some(1_000));
        assert_eq!(record.error.as_deref(), Some("network down"));
    }
}
