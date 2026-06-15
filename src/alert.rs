#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertSignificance {
    Test,
    Statement,
    Emergency,
    Watch,
    Warning,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alert {
    pub event: String,
    pub significance: AlertSignificance,
    pub originator: String,
    pub callsign: String,
    pub is_national: bool,
    pub location_codes: Vec<String>,
    pub location_names: Vec<String>,
    pub message_text: String,
    pub is_test: bool,
}

impl Alert {
    pub fn new(
        event: String,
        significance: AlertSignificance,
        originator: String,
        callsign: String,
        is_national: bool,
        location_codes: Vec<String>,
        location_names: Vec<String>,
        message_text: String,
    ) -> Self {
        Self {
            event,
            significance,
            originator,
            callsign,
            is_national,
            location_codes,
            location_names,
            message_text,
            is_test: significance == AlertSignificance::Test,
        }
    }
}

pub struct AlertMessageParts<'a> {
    pub event: &'a str,
    pub significance: AlertSignificance,
    pub originator: &'a str,
    pub callsign: &'a str,
    pub is_national: bool,
    pub location_names: &'a [String],
}

pub fn alert_send_channel(
    significance: AlertSignificance,
    alert_channel: u32,
    test_channel: u32,
) -> Option<u32> {
    if significance == AlertSignificance::Test {
        if test_channel == 10 {
            None
        } else {
            Some(test_channel)
        }
    } else {
        Some(alert_channel)
    }
}

pub fn location_filter_allows(
    is_national: bool,
    configured_locations: &[String],
    alert_codes: &[String],
) -> bool {
    if is_national {
        return true;
    }

    if !configured_locations.is_empty() && !alert_codes.is_empty() {
        alert_codes
            .iter()
            .any(|code| configured_locations.contains(code))
    } else {
        true
    }
}

pub fn format_alert_message(parts: AlertMessageParts<'_>) -> String {
    let mut message = ", Issued By: ".to_string() + parts.originator;

    match parts.significance {
        AlertSignificance::Test => {
            message =
                "📖Received ".to_string() + parts.event + " from " + parts.callsign + &message;
        }
        AlertSignificance::Statement => {
            message = "📟".to_string() + parts.event + &message;
        }
        AlertSignificance::Emergency => {
            message = "🚨".to_string() + parts.event + &message;
        }
        AlertSignificance::Watch => {
            message = "⚠️".to_string() + parts.event + &message;
        }
        AlertSignificance::Warning => {
            message = "🚨".to_string() + parts.event + &message;
        }
        AlertSignificance::Unknown => {
            message = "🚨".to_string() + parts.event + &message;
        }
    }

    if parts.is_national {
        message += " Nationwide Alert";
    } else if !parts.location_names.is_empty() {
        if parts.location_names.len() == 1 {
            message.push_str(", Location: ");
        } else {
            message.push_str(", Locations: ");
        }
        message.push_str(&parts.location_names.join(", "));
    }

    message
}

pub fn chunk_message(message: &str, max_len: usize) -> Vec<String> {
    if message.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let mut start = 0;
    let mut current = 0;

    for space in message.match_indices(' ').map(|(idx, _)| idx) {
        if space.saturating_sub(start) > max_len {
            if current > start {
                chunks.push(message[start..current].to_string());
                start = current;
            }
        }
        current = space;
    }

    if start < message.len() {
        chunks.push(message[start..].to_string());
    }

    chunks
        .into_iter()
        .filter(|chunk| !chunk.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alert_without_test_channel_is_not_routed() {
        assert_eq!(alert_send_channel(AlertSignificance::Test, 0, 10), None);
    }

    #[test]
    fn test_alert_with_test_channel_uses_test_channel() {
        assert_eq!(alert_send_channel(AlertSignificance::Test, 0, 4), Some(4));
    }

    #[test]
    fn non_test_alert_uses_alert_channel() {
        assert_eq!(
            alert_send_channel(AlertSignificance::Warning, 2, 4),
            Some(2)
        );
    }

    #[test]
    fn national_alert_bypasses_location_filter() {
        assert!(location_filter_allows(
            true,
            &[String::from("006085")],
            &[String::from("048201")]
        ));
    }

    #[test]
    fn non_national_alert_requires_matching_location_when_filter_is_set() {
        assert!(!location_filter_allows(
            false,
            &[String::from("006085")],
            &[String::from("048201")]
        ));
        assert!(location_filter_allows(
            false,
            &[String::from("006085")],
            &[String::from("006085")]
        ));
    }

    #[test]
    fn empty_location_filter_or_empty_alert_codes_allow_alert() {
        assert!(location_filter_allows(
            false,
            &[],
            &[String::from("006085")]
        ));
        assert!(location_filter_allows(
            false,
            &[String::from("006085")],
            &[]
        ));
    }

    #[test]
    fn format_warning_with_single_location_matches_existing_output() {
        let locations = vec![String::from("Central Santa Clara")];

        assert_eq!(
            format_alert_message(AlertMessageParts {
                event: "Tornado Warning",
                significance: AlertSignificance::Warning,
                originator: "National Weather Service",
                callsign: "KXYZ",
                is_national: false,
                location_names: &locations,
            }),
            "🚨Tornado Warning, Issued By: National Weather Service, Location: Central Santa Clara"
        );
    }

    #[test]
    fn format_test_alert_matches_existing_output() {
        assert_eq!(
            format_alert_message(AlertMessageParts {
                event: "Required Weekly Test",
                significance: AlertSignificance::Test,
                originator: "National Weather Service",
                callsign: "KXYZ",
                is_national: false,
                location_names: &[],
            }),
            "📖Received Required Weekly Test from KXYZ, Issued By: National Weather Service"
        );
    }

    #[test]
    fn format_national_alert_matches_existing_output() {
        assert_eq!(
            format_alert_message(AlertMessageParts {
                event: "Emergency Action Notification",
                significance: AlertSignificance::Emergency,
                originator: "Primary Entry Point System",
                callsign: "WXYZ",
                is_national: true,
                location_names: &[],
            }),
            "🚨Emergency Action Notification, Issued By: Primary Entry Point System Nationwide Alert"
        );
    }

    #[test]
    fn chunk_message_keeps_short_messages_whole() {
        assert_eq!(chunk_message("short alert", 75), vec!["short alert"]);
    }

    #[test]
    fn chunk_message_splits_long_messages_without_empty_chunks() {
        let chunks = chunk_message("one two three four five six seven", 13);

        assert_eq!(chunks, vec!["one two three", " four five", " six seven"]);
        assert!(chunks.iter().all(|chunk| !chunk.is_empty()));
    }
}
