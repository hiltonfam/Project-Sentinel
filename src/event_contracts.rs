use crate::alert::AlertSignificance;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const EVENT_CONTRACT_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
pub struct AlertRecord {
    pub schema_version: u16,
    pub record_type: String,
    pub alert_id: String,
    pub timestamp_unix_secs: u64,
    pub source: String,
    pub event: String,
    pub significance: AlertSignificance,
    pub originator: String,
    pub callsign: String,
    pub is_national: bool,
    pub is_test: bool,
    pub location_codes: Vec<String>,
    pub location_names: Vec<String>,
    pub message_text: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
pub struct DeliveryAttemptRecord {
    pub schema_version: u16,
    pub record_type: String,
    pub alert_id: String,
    pub timestamp_unix_secs: u64,
    pub sender: String,
    pub required: bool,
    pub channel: Option<u32>,
    pub status: DeliveryAttemptStatus,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
pub enum DeliveryAttemptStatus {
    Success,
    Failure,
    Skipped,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
pub struct SenderStatusRecord {
    pub schema_version: u16,
    pub record_type: String,
    pub timestamp_unix_secs: u64,
    pub sender: String,
    pub configured: bool,
    pub required: bool,
    pub ready: bool,
    pub last_success_unix_secs: Option<u64>,
    pub last_failure_unix_secs: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
pub struct SystemStatusRecord {
    pub schema_version: u16,
    pub record_type: String,
    pub timestamp_unix_secs: u64,
    pub hostname: String,
    pub sentinel_version: String,
    pub uptime_secs: Option<u64>,
    pub disk_free_bytes: Option<u64>,
    pub network_available: Option<bool>,
    pub notes: Vec<String>,
}

pub trait JsonLineRecord: Sized + Serialize + for<'de> Deserialize<'de> {
    fn to_json_line(&self) -> String {
        serde_json::to_string(self).expect("event contract serialization should not fail")
    }

    fn from_json_line(line: &str) -> Result<Self> {
        serde_json::from_str(line).context("invalid event contract JSON")
    }
}

impl JsonLineRecord for AlertRecord {}
impl JsonLineRecord for DeliveryAttemptRecord {}
impl JsonLineRecord for SenderStatusRecord {}
impl JsonLineRecord for SystemStatusRecord {}

#[cfg(test)]
mod tests {
    use super::*;

    fn alert_record() -> AlertRecord {
        AlertRecord {
            schema_version: EVENT_CONTRACT_SCHEMA_VERSION,
            record_type: "alert".to_string(),
            alert_id: "alert-123".to_string(),
            timestamp_unix_secs: 1000,
            source: "same".to_string(),
            event: "Tornado Warning".to_string(),
            significance: AlertSignificance::Warning,
            originator: "National Weather Service".to_string(),
            callsign: "KXYZ".to_string(),
            is_national: false,
            is_test: false,
            location_codes: vec!["006085".to_string()],
            location_names: vec!["Central Santa Clara".to_string()],
            message_text: "line 1\n\"quoted\"".to_string(),
        }
    }

    #[test]
    fn alert_record_serializes_to_stable_json_line() {
        let record = alert_record();

        assert_eq!(
            record.to_json_line(),
            "{\"schema_version\":1,\"record_type\":\"alert\",\"alert_id\":\"alert-123\",\"timestamp_unix_secs\":1000,\"source\":\"same\",\"event\":\"Tornado Warning\",\"significance\":\"Warning\",\"originator\":\"National Weather Service\",\"callsign\":\"KXYZ\",\"is_national\":false,\"is_test\":false,\"location_codes\":[\"006085\"],\"location_names\":[\"Central Santa Clara\"],\"message_text\":\"line 1\\n\\\"quoted\\\"\"}"
        );
        assert!(!record.to_json_line().contains('\n'));
        assert_eq!(
            AlertRecord::from_json_line(&record.to_json_line()).unwrap(),
            record
        );
    }

    #[test]
    fn delivery_attempt_record_serializes_to_stable_json_line() {
        let record = DeliveryAttemptRecord {
            schema_version: EVENT_CONTRACT_SCHEMA_VERSION,
            record_type: "delivery_attempt".to_string(),
            alert_id: "alert-123".to_string(),
            timestamp_unix_secs: 1001,
            sender: "meshtastic".to_string(),
            required: true,
            channel: Some(0),
            status: DeliveryAttemptStatus::Success,
            error: None,
        };

        assert_eq!(
            record.to_json_line(),
            "{\"schema_version\":1,\"record_type\":\"delivery_attempt\",\"alert_id\":\"alert-123\",\"timestamp_unix_secs\":1001,\"sender\":\"meshtastic\",\"required\":true,\"channel\":0,\"status\":\"Success\",\"error\":null}"
        );
        assert!(!record.to_json_line().contains('\n'));
        assert_eq!(
            DeliveryAttemptRecord::from_json_line(&record.to_json_line()).unwrap(),
            record
        );
    }

    #[test]
    fn sender_status_record_serializes_to_stable_json_line() {
        let record = SenderStatusRecord {
            schema_version: EVENT_CONTRACT_SCHEMA_VERSION,
            record_type: "sender_status".to_string(),
            timestamp_unix_secs: 1002,
            sender: "discord".to_string(),
            configured: true,
            required: false,
            ready: false,
            last_success_unix_secs: Some(900),
            last_failure_unix_secs: Some(1001),
            error: Some("webhook unavailable".to_string()),
        };

        assert_eq!(
            record.to_json_line(),
            "{\"schema_version\":1,\"record_type\":\"sender_status\",\"timestamp_unix_secs\":1002,\"sender\":\"discord\",\"configured\":true,\"required\":false,\"ready\":false,\"last_success_unix_secs\":900,\"last_failure_unix_secs\":1001,\"error\":\"webhook unavailable\"}"
        );
        assert!(!record.to_json_line().contains('\n'));
        assert_eq!(
            SenderStatusRecord::from_json_line(&record.to_json_line()).unwrap(),
            record
        );
    }

    #[test]
    fn system_status_record_serializes_to_stable_json_line() {
        let record = SystemStatusRecord {
            schema_version: EVENT_CONTRACT_SCHEMA_VERSION,
            record_type: "system_status".to_string(),
            timestamp_unix_secs: 1003,
            hostname: "sentinel-pi".to_string(),
            sentinel_version: "0.8.0".to_string(),
            uptime_secs: Some(3600),
            disk_free_bytes: Some(1048576),
            network_available: Some(false),
            notes: vec!["offline mode".to_string()],
        };

        assert_eq!(
            record.to_json_line(),
            "{\"schema_version\":1,\"record_type\":\"system_status\",\"timestamp_unix_secs\":1003,\"hostname\":\"sentinel-pi\",\"sentinel_version\":\"0.8.0\",\"uptime_secs\":3600,\"disk_free_bytes\":1048576,\"network_available\":false,\"notes\":[\"offline mode\"]}"
        );
        assert!(!record.to_json_line().contains('\n'));
        assert_eq!(
            SystemStatusRecord::from_json_line(&record.to_json_line()).unwrap(),
            record
        );
    }

    #[test]
    fn malformed_event_contract_json_returns_error() {
        assert!(AlertRecord::from_json_line("not json").is_err());
    }
}
