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
