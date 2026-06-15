use crate::alert::Alert;
use crate::spool::{SpoolRecord, Spooler};
use anyhow::Result;

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alert::{Alert, AlertSignificance};
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
}
