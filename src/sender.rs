use crate::alert::Alert;
use crate::event_contracts::{
    DeliveryAttemptRecord, DeliveryAttemptStatus, SenderStatusRecord, EVENT_CONTRACT_SCHEMA_VERSION,
};
use crate::event_emitter::{warn_event_write_failure, EventEmitter};
use crate::spool::{SpoolRecord, Spooler};
use anyhow::Result;
use std::time::{SystemTime, UNIX_EPOCH};

pub trait Sender {
    fn name(&self) -> &'static str;
    fn check_ready(&self) -> Result<()>;
    fn send_alert(&mut self, alert: &Alert, channel: u32) -> Result<()>;
}

pub struct FanOut {
    required_senders: Vec<Box<dyn Sender>>,
    best_effort_senders: Vec<Box<dyn Sender>>,
    spooler: Option<Box<dyn Spooler>>,
}

impl FanOut {
    pub fn new(required_senders: Vec<Box<dyn Sender>>) -> Self {
        Self {
            required_senders,
            best_effort_senders: Vec::new(),
            spooler: None,
        }
    }

    pub fn with_best_effort(
        required_senders: Vec<Box<dyn Sender>>,
        best_effort_senders: Vec<Box<dyn Sender>>,
    ) -> Self {
        Self {
            required_senders,
            best_effort_senders,
            spooler: None,
        }
    }

    pub fn with_spooler(mut self, spooler: Box<dyn Spooler>) -> Self {
        self.spooler = Some(spooler);
        self
    }

    #[cfg(test)]
    pub fn required_sender_count(&self) -> usize {
        self.required_senders.len()
    }

    #[cfg(test)]
    pub fn best_effort_sender_count(&self) -> usize {
        self.best_effort_senders.len()
    }

    #[cfg(test)]
    pub fn has_spooler(&self) -> bool {
        self.spooler.is_some()
    }

    pub fn check_ready(&self) -> Result<()> {
        for sender in &self.required_senders {
            sender.check_ready()?;
        }

        for sender in &self.best_effort_senders {
            if let Err(e) = sender.check_ready() {
                log::warn!("Best-effort sender readiness check failed: {}", e);
            }
        }

        Ok(())
    }

    pub fn check_ready_with_events(&self, event_emitter: &mut dyn EventEmitter) -> Result<()> {
        for sender in &self.required_senders {
            if let Err(e) = sender.check_ready() {
                emit_sender_status(
                    event_emitter,
                    sender.name(),
                    true,
                    false,
                    Some(e.to_string()),
                );
                return Err(e);
            }
            emit_sender_status(event_emitter, sender.name(), true, true, None);
        }

        for sender in &self.best_effort_senders {
            if let Err(e) = sender.check_ready() {
                log::warn!("Best-effort sender readiness check failed: {}", e);
                emit_sender_status(
                    event_emitter,
                    sender.name(),
                    false,
                    false,
                    Some(e.to_string()),
                );
            } else {
                emit_sender_status(event_emitter, sender.name(), false, true, None);
            }
        }

        Ok(())
    }

    pub fn send_alert(&mut self, alert: &Alert, channel: u32) -> Result<()> {
        for sender in &mut self.required_senders {
            sender.send_alert(alert, channel)?;
        }

        for sender in &mut self.best_effort_senders {
            if let Err(e) = sender.send_alert(alert, channel) {
                log::warn!("Best-effort sender failed: {}", e);
                if let Some(spooler) = &mut self.spooler {
                    let record =
                        SpoolRecord::from_failure(sender.name(), alert, channel, &e.to_string());
                    if let Err(spool_error) = spooler.spool(&record) {
                        log::warn!(
                            "Failed to spool best-effort sender failure: {}",
                            spool_error
                        );
                    }
                }
            }
        }

        Ok(())
    }

    pub fn send_alert_with_events(
        &mut self,
        alert: &Alert,
        channel: u32,
        alert_id: &str,
        event_emitter: &mut dyn EventEmitter,
    ) -> Result<()> {
        for sender in &mut self.required_senders {
            if let Err(e) = sender.send_alert(alert, channel) {
                emit_delivery_attempt(
                    event_emitter,
                    alert_id,
                    sender.name(),
                    true,
                    channel,
                    DeliveryAttemptStatus::Failure,
                    Some(e.to_string()),
                );
                return Err(e);
            }
            emit_delivery_attempt(
                event_emitter,
                alert_id,
                sender.name(),
                true,
                channel,
                DeliveryAttemptStatus::Success,
                None,
            );
        }

        for sender in &mut self.best_effort_senders {
            if let Err(e) = sender.send_alert(alert, channel) {
                log::warn!("Best-effort sender failed: {}", e);
                emit_delivery_attempt(
                    event_emitter,
                    alert_id,
                    sender.name(),
                    false,
                    channel,
                    DeliveryAttemptStatus::Failure,
                    Some(e.to_string()),
                );
                if let Some(spooler) = &mut self.spooler {
                    let record =
                        SpoolRecord::from_failure(sender.name(), alert, channel, &e.to_string());
                    if let Err(spool_error) = spooler.spool(&record) {
                        log::warn!(
                            "Failed to spool best-effort sender failure: {}",
                            spool_error
                        );
                    }
                }
            } else {
                emit_delivery_attempt(
                    event_emitter,
                    alert_id,
                    sender.name(),
                    false,
                    channel,
                    DeliveryAttemptStatus::Success,
                    None,
                );
            }
        }

        Ok(())
    }
}

fn unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn emit_delivery_attempt(
    event_emitter: &mut dyn EventEmitter,
    alert_id: &str,
    sender: &str,
    required: bool,
    channel: u32,
    status: DeliveryAttemptStatus,
    error: Option<String>,
) {
    let record = DeliveryAttemptRecord {
        schema_version: EVENT_CONTRACT_SCHEMA_VERSION,
        record_type: "delivery_attempt".to_string(),
        alert_id: alert_id.to_string(),
        timestamp_unix_secs: unix_timestamp_secs(),
        sender: sender.to_string(),
        required,
        channel: Some(channel),
        status,
        error,
    };

    if let Err(e) = event_emitter.emit_delivery_attempt(&record) {
        warn_event_write_failure(e);
    }
}

fn emit_sender_status(
    event_emitter: &mut dyn EventEmitter,
    sender: &str,
    required: bool,
    ready: bool,
    error: Option<String>,
) {
    let timestamp = unix_timestamp_secs();
    let record = SenderStatusRecord {
        schema_version: EVENT_CONTRACT_SCHEMA_VERSION,
        record_type: "sender_status".to_string(),
        timestamp_unix_secs: timestamp,
        sender: sender.to_string(),
        configured: true,
        required,
        ready,
        last_success_unix_secs: if ready { Some(timestamp) } else { None },
        last_failure_unix_secs: if ready { None } else { Some(timestamp) },
        error,
    };

    if let Err(e) = event_emitter.emit_sender_status(&record) {
        warn_event_write_failure(e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alert::{Alert, AlertSignificance};
    use crate::event_contracts::{DeliveryAttemptRecord, SenderStatusRecord};
    use anyhow::anyhow;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    struct RecordingSender {
        label: &'static str,
        calls: Rc<RefCell<Vec<&'static str>>>,
        ready_error: Option<&'static str>,
        send_error: Option<&'static str>,
        send_count: Rc<Cell<u32>>,
    }

    impl RecordingSender {
        fn new(label: &'static str, calls: Rc<RefCell<Vec<&'static str>>>) -> Self {
            Self {
                label,
                calls,
                ready_error: None,
                send_error: None,
                send_count: Rc::new(Cell::new(0)),
            }
        }

        fn with_send_error(mut self, error: &'static str) -> Self {
            self.send_error = Some(error);
            self
        }

        fn with_ready_error(mut self, error: &'static str) -> Self {
            self.ready_error = Some(error);
            self
        }
    }

    impl Sender for RecordingSender {
        fn name(&self) -> &'static str {
            self.label
        }

        fn check_ready(&self) -> Result<()> {
            if let Some(error) = self.ready_error {
                Err(anyhow!(error))
            } else {
                Ok(())
            }
        }

        fn send_alert(&mut self, _alert: &Alert, _channel: u32) -> Result<()> {
            self.calls.borrow_mut().push(self.label);
            self.send_count.set(self.send_count.get() + 1);

            if let Some(error) = self.send_error {
                Err(anyhow!(error))
            } else {
                Ok(())
            }
        }
    }

    struct RecordingSpooler {
        records: Rc<RefCell<Vec<SpoolRecord>>>,
        error: Option<&'static str>,
    }

    impl Spooler for RecordingSpooler {
        fn spool(&mut self, record: &SpoolRecord) -> Result<()> {
            self.records.borrow_mut().push(record.clone());

            if let Some(error) = self.error {
                Err(anyhow!(error))
            } else {
                Ok(())
            }
        }
    }

    struct RecordingEventEmitter {
        delivery_attempts: Rc<RefCell<Vec<DeliveryAttemptRecord>>>,
        sender_statuses: Rc<RefCell<Vec<SenderStatusRecord>>>,
        error: Option<&'static str>,
    }

    impl RecordingEventEmitter {
        fn new(
            delivery_attempts: Rc<RefCell<Vec<DeliveryAttemptRecord>>>,
            sender_statuses: Rc<RefCell<Vec<SenderStatusRecord>>>,
        ) -> Self {
            Self {
                delivery_attempts,
                sender_statuses,
                error: None,
            }
        }

        fn with_error(mut self, error: &'static str) -> Self {
            self.error = Some(error);
            self
        }
    }

    impl EventEmitter for RecordingEventEmitter {
        fn emit_alert(&mut self, _record: &crate::event_contracts::AlertRecord) -> Result<()> {
            if let Some(error) = self.error {
                Err(anyhow!(error))
            } else {
                Ok(())
            }
        }

        fn emit_delivery_attempt(&mut self, record: &DeliveryAttemptRecord) -> Result<()> {
            self.delivery_attempts.borrow_mut().push(record.clone());

            if let Some(error) = self.error {
                Err(anyhow!(error))
            } else {
                Ok(())
            }
        }

        fn emit_sender_status(&mut self, record: &SenderStatusRecord) -> Result<()> {
            self.sender_statuses.borrow_mut().push(record.clone());

            if let Some(error) = self.error {
                Err(anyhow!(error))
            } else {
                Ok(())
            }
        }
    }

    fn test_alert() -> Alert {
        Alert::new(
            "Tornado Warning".to_string(),
            AlertSignificance::Warning,
            "National Weather Service".to_string(),
            "KXYZ".to_string(),
            false,
            Vec::new(),
            Vec::new(),
            "test alert".to_string(),
        )
    }

    #[test]
    fn required_sender_failure_returns_error() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut fanout = FanOut::new(vec![Box::new(
            RecordingSender::new("required", Rc::clone(&calls)).with_send_error("required failed"),
        )]);

        assert!(fanout.send_alert(&test_alert(), 0).is_err());
        assert_eq!(*calls.borrow(), vec!["required"]);
    }

    #[test]
    fn best_effort_sender_failure_does_not_block_required_sender() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut fanout = FanOut::with_best_effort(
            vec![Box::new(RecordingSender::new(
                "required",
                Rc::clone(&calls),
            ))],
            vec![Box::new(
                RecordingSender::new("best-effort", Rc::clone(&calls))
                    .with_send_error("optional failed"),
            )],
        );

        assert!(fanout.send_alert(&test_alert(), 0).is_ok());
        assert_eq!(*calls.borrow(), vec!["required", "best-effort"]);
    }

    #[test]
    fn best_effort_readiness_failure_does_not_fail_startup() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let fanout = FanOut::with_best_effort(
            vec![Box::new(RecordingSender::new(
                "required",
                Rc::clone(&calls),
            ))],
            vec![Box::new(
                RecordingSender::new("best-effort", Rc::clone(&calls))
                    .with_ready_error("optional unavailable"),
            )],
        );

        assert!(fanout.check_ready().is_ok());
    }

    #[test]
    fn best_effort_sender_failure_is_spooled_when_configured() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let spooled = Rc::new(RefCell::new(Vec::new()));
        let mut fanout = FanOut::with_best_effort(
            vec![Box::new(RecordingSender::new(
                "required",
                Rc::clone(&calls),
            ))],
            vec![Box::new(
                RecordingSender::new("discord", Rc::clone(&calls))
                    .with_send_error("optional failed"),
            )],
        )
        .with_spooler(Box::new(RecordingSpooler {
            records: Rc::clone(&spooled),
            error: None,
        }));

        assert!(fanout.send_alert(&test_alert(), 4).is_ok());

        let records = spooled.borrow();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].sender, "discord");
        assert_eq!(records[0].channel, 4);
        assert_eq!(records[0].error, "optional failed");
        assert_eq!(records[0].message_text, "test alert");
    }

    #[test]
    fn spool_write_failure_does_not_block_fanout_completion() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let spooled = Rc::new(RefCell::new(Vec::new()));
        let mut fanout = FanOut::with_best_effort(
            vec![Box::new(RecordingSender::new(
                "required",
                Rc::clone(&calls),
            ))],
            vec![Box::new(
                RecordingSender::new("discord", Rc::clone(&calls))
                    .with_send_error("optional failed"),
            )],
        )
        .with_spooler(Box::new(RecordingSpooler {
            records: Rc::clone(&spooled),
            error: Some("disk unavailable"),
        }));

        assert!(fanout.send_alert(&test_alert(), 0).is_ok());
        assert_eq!(spooled.borrow().len(), 1);
    }

    #[test]
    fn required_sender_failure_is_not_spooled() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let spooled = Rc::new(RefCell::new(Vec::new()));
        let mut fanout = FanOut::new(vec![Box::new(
            RecordingSender::new("required", Rc::clone(&calls)).with_send_error("required failed"),
        )])
        .with_spooler(Box::new(RecordingSpooler {
            records: Rc::clone(&spooled),
            error: None,
        }));

        assert!(fanout.send_alert(&test_alert(), 0).is_err());
        assert!(spooled.borrow().is_empty());
    }

    #[test]
    fn delivery_attempt_records_are_emitted_for_sender_success_and_failure() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let attempts = Rc::new(RefCell::new(Vec::new()));
        let statuses = Rc::new(RefCell::new(Vec::new()));
        let mut emitter = RecordingEventEmitter::new(Rc::clone(&attempts), Rc::clone(&statuses));
        let mut fanout = FanOut::with_best_effort(
            vec![Box::new(RecordingSender::new(
                "meshtastic",
                Rc::clone(&calls),
            ))],
            vec![Box::new(
                RecordingSender::new("discord", Rc::clone(&calls))
                    .with_send_error("optional failed"),
            )],
        );

        assert!(fanout
            .send_alert_with_events(&test_alert(), 4, "alert-123", &mut emitter)
            .is_ok());

        let attempts = attempts.borrow();
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].sender, "meshtastic");
        assert!(attempts[0].required);
        assert_eq!(attempts[0].status, DeliveryAttemptStatus::Success);
        assert_eq!(attempts[0].alert_id, "alert-123");
        assert_eq!(attempts[0].channel, Some(4));
        assert_eq!(attempts[1].sender, "discord");
        assert!(!attempts[1].required);
        assert_eq!(attempts[1].status, DeliveryAttemptStatus::Failure);
        assert_eq!(attempts[1].error.as_deref(), Some("optional failed"));
    }

    #[test]
    fn event_write_failure_does_not_fail_fanout() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let attempts = Rc::new(RefCell::new(Vec::new()));
        let statuses = Rc::new(RefCell::new(Vec::new()));
        let mut emitter = RecordingEventEmitter::new(Rc::clone(&attempts), Rc::clone(&statuses))
            .with_error("disk unavailable");
        let mut fanout = FanOut::with_best_effort(
            vec![Box::new(RecordingSender::new(
                "meshtastic",
                Rc::clone(&calls),
            ))],
            vec![Box::new(RecordingSender::new("discord", Rc::clone(&calls)))],
        );

        assert!(fanout
            .send_alert_with_events(&test_alert(), 0, "alert-123", &mut emitter)
            .is_ok());
        assert_eq!(*calls.borrow(), vec!["meshtastic", "discord"]);
        assert_eq!(attempts.borrow().len(), 2);
    }

    #[test]
    fn no_event_records_are_emitted_when_emitter_is_absent() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut fanout = FanOut::new(vec![Box::new(RecordingSender::new(
            "meshtastic",
            Rc::clone(&calls),
        ))]);

        assert!(fanout.send_alert(&test_alert(), 0).is_ok());
        assert_eq!(*calls.borrow(), vec!["meshtastic"]);
    }

    #[test]
    fn sender_status_records_are_emitted_after_readiness_checks() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let attempts = Rc::new(RefCell::new(Vec::new()));
        let statuses = Rc::new(RefCell::new(Vec::new()));
        let mut emitter = RecordingEventEmitter::new(Rc::clone(&attempts), Rc::clone(&statuses));
        let fanout = FanOut::with_best_effort(
            vec![Box::new(RecordingSender::new(
                "meshtastic",
                Rc::clone(&calls),
            ))],
            vec![Box::new(
                RecordingSender::new("discord", Rc::clone(&calls))
                    .with_ready_error("optional unavailable"),
            )],
        );

        assert!(fanout.check_ready_with_events(&mut emitter).is_ok());

        let statuses = statuses.borrow();
        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses[0].sender, "meshtastic");
        assert!(statuses[0].required);
        assert!(statuses[0].ready);
        assert_eq!(statuses[1].sender, "discord");
        assert!(!statuses[1].required);
        assert!(!statuses[1].ready);
        assert_eq!(statuses[1].error.as_deref(), Some("optional unavailable"));
    }
}
