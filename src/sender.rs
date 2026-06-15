use crate::alert::Alert;
use anyhow::Result;

pub trait Sender {
    fn check_ready(&self) -> Result<()>;
    fn send_alert(&mut self, alert: &Alert, channel: u32) -> Result<()>;
}

pub struct FanOut {
    required_senders: Vec<Box<dyn Sender>>,
    best_effort_senders: Vec<Box<dyn Sender>>,
}

impl FanOut {
    pub fn new(required_senders: Vec<Box<dyn Sender>>) -> Self {
        Self {
            required_senders,
            best_effort_senders: Vec::new(),
        }
    }

    pub fn with_best_effort(
        required_senders: Vec<Box<dyn Sender>>,
        best_effort_senders: Vec<Box<dyn Sender>>,
    ) -> Self {
        Self {
            required_senders,
            best_effort_senders,
        }
    }

    #[cfg(test)]
    pub fn required_sender_count(&self) -> usize {
        self.required_senders.len()
    }

    #[cfg(test)]
    pub fn best_effort_sender_count(&self) -> usize {
        self.best_effort_senders.len()
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
}
