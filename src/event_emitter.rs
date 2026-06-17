use crate::event_contracts::{
    AlertRecord, DeliveryAttemptRecord, JsonLineRecord, SenderStatusRecord, SourceStatusRecord,
};
use anyhow::Result;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

pub trait EventEmitter {
    fn emit_alert(&mut self, record: &AlertRecord) -> Result<()>;
    fn emit_delivery_attempt(&mut self, record: &DeliveryAttemptRecord) -> Result<()>;
    fn emit_sender_status(&mut self, record: &SenderStatusRecord) -> Result<()>;
    fn emit_source_status(&mut self, record: &SourceStatusRecord) -> Result<()>;
}

pub struct FileEventEmitter {
    path: PathBuf,
}

impl FileEventEmitter {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn append_record<T: JsonLineRecord>(&mut self, record: &T) -> Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{}", record.to_json_line())?;
        Ok(())
    }
}

impl EventEmitter for FileEventEmitter {
    fn emit_alert(&mut self, record: &AlertRecord) -> Result<()> {
        self.append_record(record)
    }

    fn emit_delivery_attempt(&mut self, record: &DeliveryAttemptRecord) -> Result<()> {
        self.append_record(record)
    }

    fn emit_sender_status(&mut self, record: &SenderStatusRecord) -> Result<()> {
        self.append_record(record)
    }

    fn emit_source_status(&mut self, record: &SourceStatusRecord) -> Result<()> {
        self.append_record(record)
    }
}

pub fn warn_event_write_failure(error: anyhow::Error) {
    log::warn!("Failed to write event record: {}", error);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alert::AlertSignificance;
    use crate::event_contracts::{
        DeliveryAttemptStatus, SenderStatusRecord, SourceState, SourceStatusRecord,
        EVENT_CONTRACT_SCHEMA_VERSION,
    };
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_event_log_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "project-sentinel-event-emitter-test-{}-{}.jsonl",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn file_event_emitter_appends_one_json_line_per_record() {
        let path = temp_event_log_path();
        let mut emitter = FileEventEmitter::new(path.clone());
        let alert = AlertRecord {
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
            message_text: "test alert".to_string(),
        };
        let attempt = DeliveryAttemptRecord {
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
        let status = SenderStatusRecord {
            schema_version: EVENT_CONTRACT_SCHEMA_VERSION,
            record_type: "sender_status".to_string(),
            timestamp_unix_secs: 1002,
            sender: "meshtastic".to_string(),
            configured: true,
            required: true,
            ready: true,
            last_success_unix_secs: None,
            last_failure_unix_secs: None,
            error: None,
        };
        let source_status = SourceStatusRecord {
            schema_version: EVENT_CONTRACT_SCHEMA_VERSION,
            record_type: "source_status".to_string(),
            timestamp_unix_secs: 1003,
            source: "nws_api".to_string(),
            state: SourceState::Healthy,
            last_success_unix_secs: Some(1003),
            last_failure_unix_secs: None,
            last_decoded_message_unix_secs: None,
            last_accepted_alert_unix_secs: None,
            error: None,
        };

        emitter.emit_alert(&alert).unwrap();
        emitter.emit_delivery_attempt(&attempt).unwrap();
        emitter.emit_sender_status(&status).unwrap();
        emitter.emit_source_status(&source_status).unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], alert.to_json_line());
        assert_eq!(lines[1], attempt.to_json_line());
        assert_eq!(lines[2], status.to_json_line());
        assert_eq!(lines[3], source_status.to_json_line());

        let _ = fs::remove_file(path);
    }
}
