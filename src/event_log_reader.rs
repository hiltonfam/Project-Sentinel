use crate::event_contracts::{
    AlertRecord, DeliveryAttemptRecord, JsonLineRecord, SenderStatusRecord,
};
use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub const MAX_DISPLAY_RECORDS: usize = 500;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct DashboardData {
    pub alerts: Vec<AlertRecord>,
    pub delivery_attempts_by_alert: HashMap<String, Vec<DeliveryAttemptRecord>>,
    pub sender_statuses: Vec<SenderStatusRecord>,
    pub malformed_lines: usize,
    pub parsed_records: usize,
    pub truncated_records: usize,
    pub read_error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RecordType {
    record_type: String,
}

pub fn read_dashboard_data(path: &Path, max_records: usize) -> DashboardData {
    match File::open(path) {
        Ok(file) => read_dashboard_lines(BufReader::new(file).lines(), max_records),
        Err(e) => DashboardData {
            read_error: Some(format!("Unable to read event log: {}", e)),
            ..DashboardData::default()
        },
    }
}

pub fn read_dashboard_lines<I>(lines: I, max_records: usize) -> DashboardData
where
    I: IntoIterator<Item = Result<String, std::io::Error>>,
{
    let mut data = DashboardData::default();
    let mut latest_sender_statuses: HashMap<String, (usize, SenderStatusRecord)> = HashMap::new();

    for line_result in lines {
        let line = match line_result {
            Ok(line) => line,
            Err(e) => {
                data.malformed_lines += 1;
                data.read_error = Some(format!("Unable to read event log line: {}", e));
                continue;
            }
        };

        if data.parsed_records >= max_records {
            data.truncated_records += 1;
            continue;
        }

        let record_type = match serde_json::from_str::<RecordType>(&line) {
            Ok(record_type) => record_type.record_type,
            Err(_) => {
                data.malformed_lines += 1;
                continue;
            }
        };

        match record_type.as_str() {
            "alert" => match AlertRecord::from_json_line(&line) {
                Ok(record) => {
                    data.parsed_records += 1;
                    data.alerts.push(record);
                }
                Err(_) => data.malformed_lines += 1,
            },
            "delivery_attempt" => match DeliveryAttemptRecord::from_json_line(&line) {
                Ok(record) => {
                    data.parsed_records += 1;
                    data.delivery_attempts_by_alert
                        .entry(record.alert_id.clone())
                        .or_default()
                        .push(record);
                }
                Err(_) => data.malformed_lines += 1,
            },
            "sender_status" => match SenderStatusRecord::from_json_line(&line) {
                Ok(record) => {
                    data.parsed_records += 1;
                    latest_sender_statuses
                        .insert(record.sender.clone(), (data.parsed_records, record));
                }
                Err(_) => data.malformed_lines += 1,
            },
            _ => {
                data.malformed_lines += 1;
            }
        }
    }

    let mut statuses: Vec<(usize, SenderStatusRecord)> =
        latest_sender_statuses.into_values().collect();
    statuses.sort_by_key(|(order, _record)| *order);
    data.sender_statuses = statuses
        .into_iter()
        .map(|(_order, record)| record)
        .collect();

    data.alerts.sort_by_key(|record| record.timestamp_unix_secs);
    data.alerts.reverse();
    data.alerts.truncate(max_records);

    data
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alert::AlertSignificance;
    use crate::event_contracts::{DeliveryAttemptStatus, EVENT_CONTRACT_SCHEMA_VERSION};

    fn lines(values: &[&str]) -> Vec<Result<String, std::io::Error>> {
        values
            .iter()
            .map(|value| Ok((*value).to_string()))
            .collect()
    }

    fn alert_line(alert_id: &str, timestamp: u64) -> String {
        AlertRecord {
            schema_version: EVENT_CONTRACT_SCHEMA_VERSION,
            record_type: "alert".to_string(),
            alert_id: alert_id.to_string(),
            timestamp_unix_secs: timestamp,
            source: "same".to_string(),
            event: "Tornado Warning".to_string(),
            significance: AlertSignificance::Warning,
            originator: "National Weather Service".to_string(),
            callsign: "KXYZ".to_string(),
            is_national: false,
            is_test: false,
            location_codes: vec!["006085".to_string()],
            location_names: vec!["Central Santa Clara".to_string()],
            message_text: "test alert".to_string(),
        }
        .to_json_line()
    }

    fn delivery_line(alert_id: &str, sender: &str) -> String {
        DeliveryAttemptRecord {
            schema_version: EVENT_CONTRACT_SCHEMA_VERSION,
            record_type: "delivery_attempt".to_string(),
            alert_id: alert_id.to_string(),
            timestamp_unix_secs: 11,
            sender: sender.to_string(),
            required: sender == "meshtastic",
            channel: Some(0),
            status: DeliveryAttemptStatus::Success,
            error: None,
        }
        .to_json_line()
    }

    fn sender_status_line(sender: &str, ready: bool, timestamp: u64) -> String {
        SenderStatusRecord {
            schema_version: EVENT_CONTRACT_SCHEMA_VERSION,
            record_type: "sender_status".to_string(),
            timestamp_unix_secs: timestamp,
            sender: sender.to_string(),
            configured: true,
            required: sender == "meshtastic",
            ready,
            last_success_unix_secs: if ready { Some(timestamp) } else { None },
            last_failure_unix_secs: if ready { None } else { Some(timestamp) },
            error: if ready {
                None
            } else {
                Some("unavailable".to_string())
            },
        }
        .to_json_line()
    }

    #[test]
    fn parses_mixed_jsonl_records() {
        let alert = alert_line("alert-1", 10);
        let delivery = delivery_line("alert-1", "meshtastic");
        let status = sender_status_line("meshtastic", true, 12);

        let data = read_dashboard_lines(lines(&[&alert, &delivery, &status]), MAX_DISPLAY_RECORDS);

        assert_eq!(data.parsed_records, 3);
        assert_eq!(data.alerts.len(), 1);
        assert_eq!(data.delivery_attempts_by_alert["alert-1"].len(), 1);
        assert_eq!(data.sender_statuses.len(), 1);
        assert_eq!(data.malformed_lines, 0);
    }

    #[test]
    fn malformed_lines_are_skipped_and_counted() {
        let alert = alert_line("alert-1", 10);

        let data = read_dashboard_lines(lines(&["not json", &alert]), MAX_DISPLAY_RECORDS);

        assert_eq!(data.parsed_records, 1);
        assert_eq!(data.malformed_lines, 1);
        assert_eq!(data.alerts.len(), 1);
    }

    #[test]
    fn delivery_attempts_are_grouped_by_alert_id() {
        let first = delivery_line("alert-1", "meshtastic");
        let second = delivery_line("alert-1", "discord");
        let third = delivery_line("alert-2", "lxmf");

        let data = read_dashboard_lines(lines(&[&first, &second, &third]), MAX_DISPLAY_RECORDS);

        assert_eq!(data.delivery_attempts_by_alert["alert-1"].len(), 2);
        assert_eq!(data.delivery_attempts_by_alert["alert-2"].len(), 1);
    }

    #[test]
    fn latest_sender_status_wins() {
        let old = sender_status_line("discord", false, 10);
        let new = sender_status_line("discord", true, 11);

        let data = read_dashboard_lines(lines(&[&old, &new]), MAX_DISPLAY_RECORDS);

        assert_eq!(data.sender_statuses.len(), 1);
        assert_eq!(data.sender_statuses[0].sender, "discord");
        assert!(data.sender_statuses[0].ready);
        assert_eq!(data.sender_statuses[0].timestamp_unix_secs, 11);
    }

    #[test]
    fn caps_displayed_records() {
        let first = alert_line("alert-1", 10);
        let second = alert_line("alert-2", 11);

        let data = read_dashboard_lines(lines(&[&first, &second]), 1);

        assert_eq!(data.parsed_records, 1);
        assert_eq!(data.truncated_records, 1);
        assert_eq!(data.alerts.len(), 1);
    }
}
