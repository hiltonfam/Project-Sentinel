use crate::alert::{Alert, AlertSignificance};
use anyhow::Result;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub trait Spooler {
    fn spool(&mut self, record: &SpoolRecord) -> Result<()>;
}

pub struct FileSpooler {
    path: PathBuf,
}

impl FileSpooler {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Spooler for FileSpooler {
    fn spool(&mut self, record: &SpoolRecord) -> Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{}", record.to_json_line())?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpoolRecord {
    pub timestamp_unix_secs: u64,
    pub sender: String,
    pub channel: u32,
    pub error: String,
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

impl SpoolRecord {
    pub fn from_failure(sender: &str, alert: &Alert, channel: u32, error: &str) -> Self {
        Self::from_failure_at(
            sender,
            alert,
            channel,
            error,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        )
    }

    pub fn from_failure_at(
        sender: &str,
        alert: &Alert,
        channel: u32,
        error: &str,
        timestamp_unix_secs: u64,
    ) -> Self {
        Self {
            timestamp_unix_secs,
            sender: sender.to_string(),
            channel,
            error: error.to_string(),
            event: alert.event.clone(),
            significance: alert.significance,
            originator: alert.originator.clone(),
            callsign: alert.callsign.clone(),
            is_national: alert.is_national,
            is_test: alert.is_test,
            location_codes: alert.location_codes.clone(),
            location_names: alert.location_names.clone(),
            message_text: alert.message_text.clone(),
        }
    }

    pub fn to_json_line(&self) -> String {
        format!(
            "{{\"timestamp_unix_secs\":{},\"sender\":\"{}\",\"channel\":{},\"error\":\"{}\",\"event\":\"{}\",\"significance\":\"{:?}\",\"originator\":\"{}\",\"callsign\":\"{}\",\"is_national\":{},\"is_test\":{},\"location_codes\":{},\"location_names\":{},\"message_text\":\"{}\"}}",
            self.timestamp_unix_secs,
            json_escape(&self.sender),
            self.channel,
            json_escape(&self.error),
            json_escape(&self.event),
            self.significance,
            json_escape(&self.originator),
            json_escape(&self.callsign),
            self.is_national,
            self.is_test,
            json_string_array(&self.location_codes),
            json_string_array(&self.location_names),
            json_escape(&self.message_text),
        )
    }
}

fn json_string_array(values: &[String]) -> String {
    let values = values
        .iter()
        .map(|value| format!("\"{}\"", json_escape(value)))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{}]", values)
}

fn json_escape(value: &str) -> String {
    let mut escaped = String::new();

    for ch in value.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            c if c.is_control() => escaped.push_str(&format!("\\u{:04x}", c as u32)),
            c => escaped.push(c),
        }
    }

    escaped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alert::AlertSignificance;
    use std::fs;

    fn test_alert(message_text: String) -> Alert {
        Alert::new(
            "Tornado Warning".to_string(),
            AlertSignificance::Warning,
            "National Weather Service".to_string(),
            "KXYZ".to_string(),
            false,
            vec!["006085".to_string()],
            vec!["Central Santa Clara".to_string()],
            message_text,
        )
    }

    #[test]
    fn spool_record_serializes_to_one_escaped_json_line() {
        let alert = test_alert("line 1\n\"quoted\" \\ path".to_string());
        let record = SpoolRecord::from_failure_at(
            "discord",
            &alert,
            2,
            "failed \"badly\"\nwithout url",
            123,
        );

        assert_eq!(
            record.to_json_line(),
            "{\"timestamp_unix_secs\":123,\"sender\":\"discord\",\"channel\":2,\"error\":\"failed \\\"badly\\\"\\nwithout url\",\"event\":\"Tornado Warning\",\"significance\":\"Warning\",\"originator\":\"National Weather Service\",\"callsign\":\"KXYZ\",\"is_national\":false,\"is_test\":false,\"location_codes\":[\"006085\"],\"location_names\":[\"Central Santa Clara\"],\"message_text\":\"line 1\\n\\\"quoted\\\" \\\\ path\"}"
        );
        assert!(!record.to_json_line().contains('\n'));
    }

    #[test]
    fn file_spooler_appends_records() {
        let path = std::env::temp_dir().join(format!(
            "project-sentinel-spool-test-{}-{}.jsonl",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let alert = test_alert("alert text".to_string());
        let record = SpoolRecord::from_failure_at("discord", &alert, 0, "failed", 456);
        let mut spooler = FileSpooler::new(path.clone());

        spooler.spool(&record).unwrap();
        spooler.spool(&record).unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        let expected = format!("{}\n{}\n", record.to_json_line(), record.to_json_line());
        assert_eq!(contents, expected);

        let _ = fs::remove_file(path);
    }
}
