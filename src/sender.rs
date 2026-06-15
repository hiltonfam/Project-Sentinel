use crate::alert::Alert;
use anyhow::Result;

pub trait Sender {
    fn check_ready(&self) -> Result<()>;
    fn send_alert(&mut self, alert: &Alert, channel: u32) -> Result<()>;
}

pub struct FanOut {
    senders: Vec<Box<dyn Sender>>,
}

impl FanOut {
    pub fn new(senders: Vec<Box<dyn Sender>>) -> Self {
        Self { senders }
    }

    pub fn check_ready(&self) -> Result<()> {
        for sender in &self.senders {
            sender.check_ready()?;
        }

        Ok(())
    }

    pub fn send_alert(&mut self, alert: &Alert, channel: u32) -> Result<()> {
        for sender in &mut self.senders {
            sender.send_alert(alert, channel)?;
        }

        Ok(())
    }
}
