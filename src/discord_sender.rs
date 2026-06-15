use crate::alert::Alert;
use crate::sender::Sender;
use anyhow::{anyhow, Result};

const DISCORD_CONTENT_LIMIT: usize = 2000;
const TRUNCATION_SUFFIX: &str = "\n...[truncated]";

trait DiscordHttpClient {
    fn post_json(&self, url: &str, body: &str) -> Result<()>;
}

struct UreqDiscordHttpClient;

impl DiscordHttpClient for UreqDiscordHttpClient {
    fn post_json(&self, url: &str, body: &str) -> Result<()> {
        match ureq::post(url)
            .set("Content-Type", "application/json")
            .send_string(body)
        {
            Ok(_) => Ok(()),
            Err(ureq::Error::Status(status, _)) => Err(anyhow!(
                "Discord webhook POST failed with status {}",
                status
            )),
            Err(ureq::Error::Transport(_)) => Err(anyhow!("Discord webhook POST failed")),
        }
    }
}

pub struct DiscordSender {
    webhook_url: String,
    client: Box<dyn DiscordHttpClient>,
}

impl DiscordSender {
    pub fn new(webhook_url: String) -> Self {
        Self::with_client(webhook_url, Box::new(UreqDiscordHttpClient))
    }

    fn with_client(webhook_url: String, client: Box<dyn DiscordHttpClient>) -> Self {
        Self {
            webhook_url,
            client,
        }
    }
}

impl Sender for DiscordSender {
    fn name(&self) -> &'static str {
        "discord"
    }

    fn check_ready(&self) -> Result<()> {
        Ok(())
    }

    fn send_alert(&mut self, alert: &Alert, _channel: u32) -> Result<()> {
        let content = truncate_discord_content(&alert.message_text);
        let payload = discord_payload(&content);
        self.client.post_json(&self.webhook_url, &payload)
    }
}

fn discord_payload(content: &str) -> String {
    format!(r#"{{"content":"{}"}}"#, json_escape(content))
}

fn truncate_discord_content(content: &str) -> String {
    if content.chars().count() <= DISCORD_CONTENT_LIMIT {
        return content.to_string();
    }

    let keep = DISCORD_CONTENT_LIMIT - TRUNCATION_SUFFIX.chars().count();
    let mut truncated: String = content.chars().take(keep).collect();
    truncated.push_str(TRUNCATION_SUFFIX);
    truncated
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
    use std::cell::RefCell;
    use std::rc::Rc;

    struct RecordingDiscordHttpClient {
        requests: Rc<RefCell<Vec<(String, String)>>>,
        error: Option<&'static str>,
    }

    impl DiscordHttpClient for RecordingDiscordHttpClient {
        fn post_json(&self, url: &str, body: &str) -> Result<()> {
            self.requests
                .borrow_mut()
                .push((url.to_string(), body.to_string()));

            if let Some(error) = self.error {
                Err(anyhow!(error))
            } else {
                Ok(())
            }
        }
    }

    fn test_alert(message_text: String) -> Alert {
        Alert::new(
            "Tornado Warning".to_string(),
            AlertSignificance::Warning,
            "National Weather Service".to_string(),
            "KXYZ".to_string(),
            false,
            Vec::new(),
            Vec::new(),
            message_text,
        )
    }

    #[test]
    fn discord_payload_uses_alert_message_text_as_content() {
        let requests = Rc::new(RefCell::new(Vec::new()));
        let mut sender = DiscordSender::with_client(
            "https://discord.example/webhook".to_string(),
            Box::new(RecordingDiscordHttpClient {
                requests: Rc::clone(&requests),
                error: None,
            }),
        );

        sender
            .send_alert(&test_alert("alert text".to_string()), 0)
            .unwrap();

        assert_eq!(
            *requests.borrow(),
            vec![(
                "https://discord.example/webhook".to_string(),
                r#"{"content":"alert text"}"#.to_string()
            )]
        );
    }

    #[test]
    fn discord_payload_escapes_json_content() {
        assert_eq!(
            discord_payload("line 1\n\"quoted\" \\ path"),
            r#"{"content":"line 1\n\"quoted\" \\ path"}"#
        );
    }

    #[test]
    fn discord_content_is_truncated_to_limit_with_suffix() {
        let content = truncate_discord_content(&"x".repeat(DISCORD_CONTENT_LIMIT + 1));

        assert_eq!(content.chars().count(), DISCORD_CONTENT_LIMIT);
        assert!(content.ends_with(TRUNCATION_SUFFIX));
    }

    #[test]
    fn discord_send_errors_are_returned_to_best_effort_fanout() {
        let requests = Rc::new(RefCell::new(Vec::new()));
        let mut sender = DiscordSender::with_client(
            "https://discord.example/webhook".to_string(),
            Box::new(RecordingDiscordHttpClient {
                requests: Rc::clone(&requests),
                error: Some("send failed"),
            }),
        );

        assert!(sender
            .send_alert(&test_alert("alert text".to_string()), 0)
            .is_err());
        assert_eq!(requests.borrow().len(), 1);
    }
}
