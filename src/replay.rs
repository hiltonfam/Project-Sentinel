use crate::sender::Sender;
use crate::spool::{FileSpooler, SpoolRecord, Spooler};
use anyhow::Result;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

const REPLAYABLE_SENDERS: [&str; 3] = ["discord", "lxmf", "meshcore"];

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ReplaySummary {
    pub parsed_records: usize,
    pub replayed_records: usize,
    pub failed_records: usize,
    pub malformed_lines: usize,
    pub skipped_meshtastic_records: usize,
    pub skipped_unconfigured_records: usize,
    pub skipped_unknown_sender_records: usize,
}

pub fn replay_spool_file(
    spool_path: &Path,
    senders: Vec<Box<dyn Sender>>,
    failed_output_path: Option<&Path>,
) -> Result<ReplaySummary> {
    let file = File::open(spool_path)?;
    let reader = BufReader::new(file);
    let mut failed_spooler = failed_output_path
        .map(|path| Box::new(FileSpooler::new(path.to_path_buf())) as Box<dyn Spooler>);

    match failed_spooler {
        Some(ref mut spooler) => {
            replay_spool_lines(reader.lines(), senders, Some(spooler.as_mut()))
        }
        None => replay_spool_lines(reader.lines(), senders, None),
    }
}

pub fn replay_spool_lines<I>(
    lines: I,
    senders: Vec<Box<dyn Sender>>,
    mut failed_spooler: Option<&mut dyn Spooler>,
) -> Result<ReplaySummary>
where
    I: IntoIterator<Item = std::io::Result<String>>,
{
    let mut summary = ReplaySummary::default();
    let mut senders = senders
        .into_iter()
        .map(|sender| (sender.name().to_string(), sender))
        .collect::<HashMap<_, _>>();

    for line in lines {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let record = match SpoolRecord::from_json_line(line) {
            Ok(record) => record,
            Err(e) => {
                summary.malformed_lines += 1;
                log::warn!("Skipping malformed spool record: {}", e);
                continue;
            }
        };

        summary.parsed_records += 1;

        if record.sender == "meshtastic" {
            summary.skipped_meshtastic_records += 1;
            continue;
        }

        if !REPLAYABLE_SENDERS.contains(&record.sender.as_str()) {
            summary.skipped_unknown_sender_records += 1;
            continue;
        }

        let Some(sender) = senders.get_mut(&record.sender) else {
            summary.skipped_unconfigured_records += 1;
            continue;
        };

        let alert = record.to_alert();
        if let Err(e) = sender.send_alert(&alert, record.channel) {
            summary.failed_records += 1;
            log::warn!("Replay failed for {} sender: {}", record.sender, e);

            if let Some(spooler) = failed_spooler.as_deref_mut() {
                let mut failed_record = record;
                failed_record.error = format!("Replay failed: {}", e);
                if let Err(spool_error) = spooler.spool(&failed_record) {
                    log::warn!("Failed to write replay failure record: {}", spool_error);
                }
            }

            continue;
        }

        summary.replayed_records += 1;
    }

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alert::{Alert, AlertSignificance};
    use anyhow::{anyhow, Result};
    use std::cell::RefCell;
    use std::fs;
    use std::rc::Rc;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct RecordingSender {
        label: &'static str,
        calls: Rc<RefCell<Vec<(String, u32)>>>,
        send_error: Option<&'static str>,
    }

    impl RecordingSender {
        fn new(label: &'static str, calls: Rc<RefCell<Vec<(String, u32)>>>) -> Self {
            Self {
                label,
                calls,
                send_error: None,
            }
        }

        fn with_send_error(mut self, error: &'static str) -> Self {
            self.send_error = Some(error);
            self
        }
    }

    impl Sender for RecordingSender {
        fn name(&self) -> &'static str {
            self.label
        }

        fn check_ready(&self) -> Result<()> {
            Ok(())
        }

        fn send_alert(&mut self, alert: &Alert, channel: u32) -> Result<()> {
            self.calls
                .borrow_mut()
                .push((alert.message_text.clone(), channel));

            if let Some(error) = self.send_error {
                Err(anyhow!(error))
            } else {
                Ok(())
            }
        }
    }

    struct RecordingSpooler {
        records: Rc<RefCell<Vec<SpoolRecord>>>,
    }

    impl Spooler for RecordingSpooler {
        fn spool(&mut self, record: &SpoolRecord) -> Result<()> {
            self.records.borrow_mut().push(record.clone());
            Ok(())
        }
    }

    fn test_alert(message_text: &str) -> Alert {
        Alert::new(
            "Tornado Warning".to_string(),
            AlertSignificance::Warning,
            "National Weather Service".to_string(),
            "KXYZ".to_string(),
            false,
            vec!["006085".to_string()],
            vec!["Central Santa Clara".to_string()],
            message_text.to_string(),
        )
    }

    fn record(sender: &str, message_text: &str, channel: u32) -> String {
        SpoolRecord::from_failure_at(sender, &test_alert(message_text), channel, "failed", 123)
            .to_json_line()
    }

    fn lines(values: &[&str]) -> Vec<std::io::Result<String>> {
        values
            .iter()
            .map(|value| Ok((*value).to_string()))
            .collect()
    }

    #[test]
    fn malformed_spool_lines_are_skipped_without_crashing() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let discord = record("discord", "good alert", 1);

        let summary = replay_spool_lines(
            lines(&["not json", &discord]),
            vec![Box::new(RecordingSender::new("discord", Rc::clone(&calls)))],
            None,
        )
        .unwrap();

        assert_eq!(summary.malformed_lines, 1);
        assert_eq!(summary.replayed_records, 1);
        assert_eq!(*calls.borrow(), vec![("good alert".to_string(), 1)]);
    }

    #[test]
    fn replays_discord_records_only_when_discord_sender_is_configured() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let discord = record("discord", "discord alert", 2);

        let skipped = replay_spool_lines(lines(&[&discord]), Vec::new(), None).unwrap();
        assert_eq!(skipped.skipped_unconfigured_records, 1);

        let replayed = replay_spool_lines(
            lines(&[&discord]),
            vec![Box::new(RecordingSender::new("discord", Rc::clone(&calls)))],
            None,
        )
        .unwrap();

        assert_eq!(replayed.replayed_records, 1);
        assert_eq!(*calls.borrow(), vec![("discord alert".to_string(), 2)]);
    }

    #[test]
    fn replays_lxmf_records_only_when_lxmf_sender_is_configured() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let lxmf = record("lxmf", "lxmf alert", 3);

        let skipped = replay_spool_lines(lines(&[&lxmf]), Vec::new(), None).unwrap();
        assert_eq!(skipped.skipped_unconfigured_records, 1);

        let replayed = replay_spool_lines(
            lines(&[&lxmf]),
            vec![Box::new(RecordingSender::new("lxmf", Rc::clone(&calls)))],
            None,
        )
        .unwrap();

        assert_eq!(replayed.replayed_records, 1);
        assert_eq!(*calls.borrow(), vec![("lxmf alert".to_string(), 3)]);
    }

    #[test]
    fn replays_meshcore_records_only_when_meshcore_sender_is_configured() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let meshcore = record("meshcore", "meshcore alert", 4);

        let skipped = replay_spool_lines(lines(&[&meshcore]), Vec::new(), None).unwrap();
        assert_eq!(skipped.skipped_unconfigured_records, 1);

        let replayed = replay_spool_lines(
            lines(&[&meshcore]),
            vec![Box::new(RecordingSender::new(
                "meshcore",
                Rc::clone(&calls),
            ))],
            None,
        )
        .unwrap();

        assert_eq!(replayed.replayed_records, 1);
        assert_eq!(*calls.borrow(), vec![("meshcore alert".to_string(), 4)]);
    }

    #[test]
    fn meshtastic_records_are_never_replayed() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let meshtastic = record("meshtastic", "mesh alert", 0);

        let summary = replay_spool_lines(
            lines(&[&meshtastic]),
            vec![Box::new(RecordingSender::new(
                "meshtastic",
                Rc::clone(&calls),
            ))],
            None,
        )
        .unwrap();

        assert_eq!(summary.skipped_meshtastic_records, 1);
        assert!(calls.borrow().is_empty());
    }

    #[test]
    fn replay_failure_does_not_stop_later_records() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let first = record("discord", "first alert", 0);
        let second = record("lxmf", "second alert", 0);

        let summary = replay_spool_lines(
            lines(&[&first, &second]),
            vec![
                Box::new(
                    RecordingSender::new("discord", Rc::clone(&calls))
                        .with_send_error("discord down"),
                ),
                Box::new(RecordingSender::new("lxmf", Rc::clone(&calls))),
            ],
            None,
        )
        .unwrap();

        assert_eq!(summary.failed_records, 1);
        assert_eq!(summary.replayed_records, 1);
        assert_eq!(
            *calls.borrow(),
            vec![
                ("first alert".to_string(), 0),
                ("second alert".to_string(), 0)
            ]
        );
    }

    #[test]
    fn failed_replay_records_are_written_when_spooler_is_configured() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let spooled = Rc::new(RefCell::new(Vec::new()));
        let discord = record("discord", "discord alert", 0);
        let mut spooler = RecordingSpooler {
            records: Rc::clone(&spooled),
        };

        let summary = replay_spool_lines(
            lines(&[&discord]),
            vec![Box::new(
                RecordingSender::new("discord", Rc::clone(&calls)).with_send_error("discord down"),
            )],
            Some(&mut spooler),
        )
        .unwrap();

        assert_eq!(summary.failed_records, 1);
        assert_eq!(spooled.borrow().len(), 1);
        assert_eq!(spooled.borrow()[0].sender, "discord");
        assert_eq!(spooled.borrow()[0].error, "Replay failed: discord down");
    }

    #[test]
    fn replay_spool_file_writes_failed_records_to_failed_output_path() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let spool_path = std::env::temp_dir().join(format!(
            "project-sentinel-replay-source-{}-{}.jsonl",
            std::process::id(),
            unique
        ));
        let failed_path = std::env::temp_dir().join(format!(
            "project-sentinel-replay-failed-{}-{}.jsonl",
            std::process::id(),
            unique
        ));
        let discord = record("discord", "discord alert", 0);
        fs::write(&spool_path, format!("{}\n", discord)).unwrap();

        let summary = replay_spool_file(
            &spool_path,
            vec![Box::new(
                RecordingSender::new("discord", Rc::clone(&calls)).with_send_error("discord down"),
            )],
            Some(&failed_path),
        )
        .unwrap();

        assert_eq!(summary.failed_records, 1);
        let failed_contents = fs::read_to_string(&failed_path).unwrap();
        let failed_record =
            SpoolRecord::from_json_line(failed_contents.trim_end_matches('\n')).unwrap();
        assert_eq!(failed_record.sender, "discord");
        assert_eq!(failed_record.error, "Replay failed: discord down");

        let _ = fs::remove_file(spool_path);
        let _ = fs::remove_file(failed_path);
    }
}
