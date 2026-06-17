use crate::event_contracts::{
    AlertRecord, DeliveryAttemptRecord, DeliveryAttemptStatus, SenderStatusRecord,
};
use crate::event_log_reader::DashboardData;

pub fn render_dashboard(event_log_path: &str, data: &DashboardData) -> String {
    let mut html = String::new();

    html.push_str("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
    html.push_str("<title>Sentinel Dashboard</title>");
    html.push_str("<style>");
    html.push_str(
        "body{font-family:Arial,sans-serif;margin:0;background:#f5f7fa;color:#17202a}\
         header{background:#102030;color:#fff;padding:16px 24px}\
         main{padding:20px;max-width:1200px;margin:0 auto}\
         section{background:#fff;border:1px solid #d7dde5;border-radius:8px;margin:0 0 16px;padding:16px}\
         h1,h2,h3{margin:0 0 12px}\
         table{width:100%;border-collapse:collapse}\
         th,td{border-bottom:1px solid #e6ebf0;padding:8px;text-align:left;vertical-align:top}\
         th{background:#eef3f8}\
         .ok{color:#137333;font-weight:bold}.bad{color:#b3261e;font-weight:bold}\
         .muted{color:#566573}.error{background:#fff1f0;border-color:#ffccc7}\
         .pill{display:inline-block;border-radius:999px;padding:2px 8px;background:#eef3f8}",
    );
    html.push_str("</style></head><body>");
    html.push_str("<header><h1>Sentinel Dashboard</h1><div>Read-only local event log view</div></header><main>");

    render_health_section(&mut html, event_log_path, data);
    render_sender_status_section(&mut html, &data.sender_statuses);
    render_alerts_section(&mut html, data);

    html.push_str("</main></body></html>");
    html
}

fn render_health_section(html: &mut String, event_log_path: &str, data: &DashboardData) {
    let class = if data.read_error.is_some() {
        " class=\"error\""
    } else {
        ""
    };
    html.push_str(&format!("<section{}><h2>Event Log Health</h2>", class));
    html.push_str("<table><tbody>");
    html.push_str(&row("Path", &escape_html(event_log_path)));
    html.push_str(&row("Parsed records", &data.parsed_records.to_string()));
    html.push_str(&row("Malformed lines", &data.malformed_lines.to_string()));
    html.push_str(&row(
        "Truncated records",
        &data.truncated_records.to_string(),
    ));
    html.push_str(&row(
        "Read status",
        &escape_html(data.read_error.as_deref().unwrap_or("OK")),
    ));
    html.push_str("</tbody></table></section>");
}

fn render_sender_status_section(html: &mut String, statuses: &[SenderStatusRecord]) {
    html.push_str("<section><h2>Sender Status</h2>");
    if statuses.is_empty() {
        html.push_str("<p class=\"muted\">No sender status records found.</p></section>");
        return;
    }

    html.push_str("<table><thead><tr><th>Sender</th><th>Required</th><th>Ready</th><th>Last Success</th><th>Last Failure</th><th>Error</th></tr></thead><tbody>");
    for status in statuses {
        html.push_str("<tr>");
        html.push_str(&cell(&escape_html(&status.sender)));
        html.push_str(&cell(if status.required { "yes" } else { "no" }));
        html.push_str(&cell(if status.ready {
            "<span class=\"ok\">ready</span>"
        } else {
            "<span class=\"bad\">not ready</span>"
        }));
        html.push_str(&cell(&option_u64(status.last_success_unix_secs)));
        html.push_str(&cell(&option_u64(status.last_failure_unix_secs)));
        html.push_str(&cell(&escape_html(status.error.as_deref().unwrap_or(""))));
        html.push_str("</tr>");
    }
    html.push_str("</tbody></table></section>");
}

fn render_alerts_section(html: &mut String, data: &DashboardData) {
    html.push_str("<section><h2>Recent Alerts</h2>");
    if data.alerts.is_empty() {
        html.push_str("<p class=\"muted\">No alert records found.</p></section>");
        return;
    }

    for alert in &data.alerts {
        render_alert(html, alert, data);
    }
    html.push_str("</section>");
}

fn render_alert(html: &mut String, alert: &AlertRecord, data: &DashboardData) {
    html.push_str("<section>");
    html.push_str(&format!(
        "<h3>{}</h3><p><span class=\"pill\">{}</span> <span class=\"muted\">{}</span></p>",
        escape_html(&alert.event),
        escape_html(&format!("{:?}", alert.significance)),
        escape_html(&alert.alert_id)
    ));
    html.push_str("<table><tbody>");
    html.push_str(&row("Timestamp", &alert.timestamp_unix_secs.to_string()));
    html.push_str(&row("Originator", &escape_html(&alert.originator)));
    html.push_str(&row("Callsign", &escape_html(&alert.callsign)));
    html.push_str(&row(
        "Locations",
        &escape_html(&alert.location_names.join(", ")),
    ));
    html.push_str(&row("Message", &escape_html(&alert.message_text)));
    html.push_str("</tbody></table>");

    html.push_str("<h3>Delivery Attempts</h3>");
    let attempts = data.delivery_attempts_by_alert.get(&alert.alert_id);
    if let Some(attempts) = attempts {
        render_attempts(html, attempts);
    } else {
        html.push_str("<p class=\"muted\">No delivery attempts recorded for this alert.</p>");
    }
    html.push_str("</section>");
}

fn render_attempts(html: &mut String, attempts: &[DeliveryAttemptRecord]) {
    html.push_str("<table><thead><tr><th>Sender</th><th>Required</th><th>Status</th><th>Channel</th><th>Error</th></tr></thead><tbody>");
    for attempt in attempts {
        html.push_str("<tr>");
        html.push_str(&cell(&escape_html(&attempt.sender)));
        html.push_str(&cell(if attempt.required { "yes" } else { "no" }));
        html.push_str(&cell(&status_label(&attempt.status)));
        html.push_str(&cell(&option_u32(attempt.channel)));
        html.push_str(&cell(&escape_html(attempt.error.as_deref().unwrap_or(""))));
        html.push_str("</tr>");
    }
    html.push_str("</tbody></table>");
}

fn row(label: &str, value: &str) -> String {
    format!("<tr><th>{}</th><td>{}</td></tr>", label, value)
}

fn cell(value: &str) -> String {
    format!("<td>{}</td>", value)
}

fn option_u64(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "".to_string())
}

fn option_u32(value: Option<u32>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "".to_string())
}

fn status_label(status: &DeliveryAttemptStatus) -> String {
    match status {
        DeliveryAttemptStatus::Success => "<span class=\"ok\">success</span>".to_string(),
        DeliveryAttemptStatus::Failure => "<span class=\"bad\">failure</span>".to_string(),
        DeliveryAttemptStatus::Skipped => "<span class=\"muted\">skipped</span>".to_string(),
    }
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alert::AlertSignificance;
    use crate::event_contracts::{DeliveryAttemptStatus, EVENT_CONTRACT_SCHEMA_VERSION};
    use std::collections::HashMap;

    fn dashboard_data() -> DashboardData {
        let alert = AlertRecord {
            schema_version: EVENT_CONTRACT_SCHEMA_VERSION,
            record_type: "alert".to_string(),
            alert_id: "alert-1".to_string(),
            timestamp_unix_secs: 10,
            source: "same".to_string(),
            event: "Tornado Warning".to_string(),
            significance: AlertSignificance::Warning,
            originator: "National Weather Service".to_string(),
            callsign: "KXYZ".to_string(),
            is_national: false,
            is_test: false,
            location_codes: vec!["006085".to_string()],
            location_names: vec!["Central Santa Clara".to_string()],
            message_text: "alert <text>".to_string(),
        };
        let attempt = DeliveryAttemptRecord {
            schema_version: EVENT_CONTRACT_SCHEMA_VERSION,
            record_type: "delivery_attempt".to_string(),
            alert_id: "alert-1".to_string(),
            timestamp_unix_secs: 11,
            sender: "meshtastic".to_string(),
            required: true,
            channel: Some(0),
            status: DeliveryAttemptStatus::Success,
            error: None,
        };
        let status = SenderStatusRecord {
            schema_version: EVENT_CONTRACT_SCHEMA_VERSION,
            record_type: "sender_status".to_string(),
            timestamp_unix_secs: 12,
            sender: "meshtastic".to_string(),
            configured: true,
            required: true,
            ready: true,
            last_success_unix_secs: Some(12),
            last_failure_unix_secs: None,
            error: None,
        };
        let mut attempts = HashMap::new();
        attempts.insert("alert-1".to_string(), vec![attempt]);

        DashboardData {
            alerts: vec![alert],
            delivery_attempts_by_alert: attempts,
            sender_statuses: vec![status],
            malformed_lines: 0,
            parsed_records: 3,
            truncated_records: 0,
            read_error: None,
        }
    }

    #[test]
    fn rendered_html_contains_no_external_assets() {
        let html = render_dashboard("events.jsonl", &dashboard_data());

        assert!(html.contains("Sentinel Dashboard"));
        assert!(html.contains("Tornado Warning"));
        assert!(html.contains("meshtastic"));
        assert!(html.contains("alert &lt;text&gt;"));
        assert!(!html.contains("http://"));
        assert!(!html.contains("https://"));
        assert!(!html.contains("<script"));
        assert!(!html.contains("cdn"));
    }

    #[test]
    fn missing_file_error_is_rendered_readably() {
        let data = DashboardData {
            read_error: Some("Unable to read event log: missing".to_string()),
            ..DashboardData::default()
        };

        let html = render_dashboard("missing.jsonl", &data);

        assert!(html.contains("Unable to read event log: missing"));
        assert!(html.contains("No alert records found."));
    }
}
