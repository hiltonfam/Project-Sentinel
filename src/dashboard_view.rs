use crate::event_contracts::{
    AlertRecord, DeliveryAttemptRecord, DeliveryAttemptStatus, SenderStatusRecord,
};
use crate::event_log_reader::DashboardData;

const REFRESH_SECONDS: u64 = 30;

#[derive(Debug, Default, PartialEq, Eq)]
struct DashboardSummary {
    alert_count: usize,
    sender_ready_count: usize,
    sender_not_ready_count: usize,
    delivery_success_count: usize,
    delivery_failure_count: usize,
    delivery_skipped_count: usize,
}

pub fn render_dashboard(event_log_path: &str, data: &DashboardData) -> String {
    let summary = DashboardSummary::from_data(data);
    let mut html = String::new();

    html.push_str("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
    html.push_str(&format!(
        "<meta http-equiv=\"refresh\" content=\"{}\">",
        REFRESH_SECONDS
    ));
    html.push_str("<title>Sentinel Dashboard</title>");
    html.push_str("<style>");
    html.push_str(
        ":root{color-scheme:light;--bg:#eef2f6;--panel:#ffffff;--ink:#18212f;--muted:#5d6b7c;\
         --line:#d8e0ea;--head:#12263a;--ok:#166534;--bad:#b42318;--warn:#a15c00;--chip:#e8eef5}\
         *{box-sizing:border-box}body{font-family:Arial,sans-serif;margin:0;background:var(--bg);color:var(--ink)}\
         header{background:var(--head);color:#fff;padding:18px 22px}header h1{margin:0;font-size:1.55rem}\
         header div{margin-top:4px;color:#d5dee8}main{padding:18px;max-width:1280px;margin:0 auto}\
         section{background:var(--panel);border:1px solid var(--line);border-radius:8px;margin:0 0 16px;padding:16px}\
         h2{font-size:1.15rem;margin:0 0 12px}h3{font-size:1rem;margin:0 0 8px}p{margin:0 0 10px;line-height:1.4}\
         table{width:100%;border-collapse:collapse}th,td{border-bottom:1px solid #e7edf4;padding:8px;text-align:left;vertical-align:top}\
         th{background:#f1f5f9;color:#26384c;font-size:.78rem;text-transform:uppercase;letter-spacing:.03em}\
         .summary{display:grid;grid-template-columns:repeat(5,minmax(0,1fr));gap:12px}\
         .metric{background:#f8fafc;border:1px solid var(--line);border-radius:8px;padding:12px}.metric strong{display:block;font-size:1.55rem}.metric span{color:var(--muted)}\
         .status-grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(190px,1fr));gap:10px}.status-card{border:1px solid var(--line);border-radius:8px;padding:12px;background:#fbfdff}\
         .status-card strong{display:block;margin-bottom:6px}.alert-card{border:1px solid var(--line);border-radius:8px;background:#fff;margin:0 0 14px;overflow:hidden}\
         .alert-head{display:flex;gap:10px;justify-content:space-between;align-items:flex-start;background:#f8fafc;border-bottom:1px solid var(--line);padding:12px}\
         .alert-title{font-size:1.05rem;font-weight:bold}.alert-meta{color:var(--muted);font-size:.9rem}.alert-body{padding:12px}.message{white-space:pre-wrap;background:#f8fafc;border:1px solid var(--line);border-radius:6px;padding:10px}\
         .pill{display:inline-block;border-radius:999px;padding:3px 9px;background:var(--chip);font-size:.82rem;font-weight:bold}.ok{color:var(--ok);font-weight:bold}.bad{color:var(--bad);font-weight:bold}.warn{color:var(--warn);font-weight:bold}.muted{color:var(--muted)}\
         .error{border-color:#ffb4ab;background:#fff7f5}.health-list{display:grid;grid-template-columns:repeat(auto-fit,minmax(190px,1fr));gap:8px}.health-item{background:#f8fafc;border:1px solid var(--line);border-radius:6px;padding:9px}.health-item span{display:block;color:var(--muted);font-size:.82rem}\
         .table-wrap{overflow-x:auto}@media(max-width:760px){main{padding:12px}.summary{grid-template-columns:repeat(2,minmax(0,1fr))}.alert-head{display:block}.metric strong{font-size:1.35rem}th,td{padding:7px}}\
         @media(max-width:480px){.summary{grid-template-columns:1fr}header{padding:14px}section{padding:12px}}",
    );
    html.push_str("</style></head><body>");
    html.push_str("<header><h1>Sentinel Dashboard</h1><div>Read-only local event log view</div></header><main>");

    render_summary_band(&mut html, &summary);
    render_health_section(&mut html, event_log_path, data);
    render_sender_status_section(&mut html, &data.sender_statuses);
    render_alerts_section(&mut html, data);

    html.push_str("</main></body></html>");
    html
}

impl DashboardSummary {
    fn from_data(data: &DashboardData) -> Self {
        let mut summary = DashboardSummary {
            alert_count: data.alerts.len(),
            sender_ready_count: data
                .sender_statuses
                .iter()
                .filter(|status| status.ready)
                .count(),
            sender_not_ready_count: data
                .sender_statuses
                .iter()
                .filter(|status| !status.ready)
                .count(),
            ..DashboardSummary::default()
        };

        for attempt in data.delivery_attempts_by_alert.values().flatten() {
            match attempt.status {
                DeliveryAttemptStatus::Success => summary.delivery_success_count += 1,
                DeliveryAttemptStatus::Failure => summary.delivery_failure_count += 1,
                DeliveryAttemptStatus::Skipped => summary.delivery_skipped_count += 1,
            }
        }

        summary
    }
}

fn render_summary_band(html: &mut String, summary: &DashboardSummary) {
    html.push_str("<section><h2>Operator Summary</h2><div class=\"summary\">");
    render_metric(html, "Recent Alerts", summary.alert_count);
    render_metric(html, "Senders Ready", summary.sender_ready_count);
    render_metric(html, "Senders Not Ready", summary.sender_not_ready_count);
    render_metric(html, "Delivery Success", summary.delivery_success_count);
    render_metric(html, "Delivery Failure", summary.delivery_failure_count);
    html.push_str("</div>");
    if summary.delivery_skipped_count > 0 {
        html.push_str(&format!(
            "<p class=\"muted\">Skipped delivery attempts: {}</p>",
            summary.delivery_skipped_count
        ));
    }
    html.push_str("</section>");
}

fn render_metric(html: &mut String, label: &str, value: usize) {
    html.push_str(&format!(
        "<div class=\"metric\"><strong>{}</strong><span>{}</span></div>",
        value,
        escape_html(label)
    ));
}

fn render_health_section(html: &mut String, event_log_path: &str, data: &DashboardData) {
    let class = if data.read_error.is_some() {
        " class=\"error\""
    } else {
        ""
    };
    html.push_str(&format!("<section{}><h2>Event Log Health</h2>", class));
    html.push_str("<div class=\"health-list\">");
    render_health_item(html, "Path", &escape_html(event_log_path));
    render_health_item(html, "Parsed records", &data.parsed_records.to_string());
    render_health_item(html, "Malformed lines", &data.malformed_lines.to_string());
    render_health_item(
        html,
        "Truncated records",
        &data.truncated_records.to_string(),
    );
    render_health_item(
        html,
        "Read status",
        &escape_html(data.read_error.as_deref().unwrap_or("OK")),
    );
    render_health_item(
        html,
        "Refresh",
        &format!("Local meta refresh every {} seconds", REFRESH_SECONDS),
    );
    html.push_str("</div>");
    if data.malformed_lines > 0 {
        html.push_str("<p class=\"warn\">Malformed event log lines were skipped.</p>");
    }
    if data.truncated_records > 0 {
        html.push_str("<p class=\"warn\">Display cap reached; newer dashboard pages may omit older records.</p>");
    }
    html.push_str("</section>");
}

fn render_health_item(html: &mut String, label: &str, value: &str) {
    html.push_str(&format!(
        "<div class=\"health-item\"><span>{}</span>{}</div>",
        escape_html(label),
        value
    ));
}

fn render_sender_status_section(html: &mut String, statuses: &[SenderStatusRecord]) {
    html.push_str("<section><h2>Sender Status</h2>");
    if statuses.is_empty() {
        html.push_str("<p class=\"muted\">No sender status records found.</p></section>");
        return;
    }

    html.push_str("<div class=\"status-grid\">");
    for status in statuses {
        let status_class = if status.ready { "ok" } else { "bad" };
        let status_text = if status.ready { "ready" } else { "not ready" };
        html.push_str("<div class=\"status-card\">");
        html.push_str(&format!(
            "<strong>{}</strong><span class=\"{}\">{}</span>",
            escape_html(&status.sender),
            status_class,
            status_text
        ));
        html.push_str(&format!(
            "<p class=\"muted\">{} sender</p>",
            if status.required {
                "required"
            } else {
                "best-effort"
            }
        ));
        html.push_str("<table><tbody>");
        html.push_str(&row("Last status", &status.timestamp_unix_secs.to_string()));
        html.push_str(&row(
            "Last success",
            &option_u64(status.last_success_unix_secs),
        ));
        html.push_str(&row(
            "Last failure",
            &option_u64(status.last_failure_unix_secs),
        ));
        html.push_str(&row(
            "Error",
            &escape_html(status.error.as_deref().unwrap_or("")),
        ));
        html.push_str("</tbody></table></div>");
    }
    html.push_str("</div></section>");
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
    html.push_str("<article class=\"alert-card\">");
    html.push_str("<div class=\"alert-head\"><div>");
    html.push_str(&format!(
        "<div class=\"alert-title\">{}</div><div class=\"alert-meta\">{} | {}</div>",
        escape_html(&alert.event),
        escape_html(&alert.alert_id),
        alert.timestamp_unix_secs
    ));
    html.push_str("</div><div>");
    html.push_str(&format!(
        "<span class=\"pill\">{}</span> ",
        escape_html(&format!("{:?}", alert.significance))
    ));
    if alert.is_national {
        html.push_str("<span class=\"pill\">national</span> ");
    }
    if alert.is_test {
        html.push_str("<span class=\"pill\">test</span>");
    }
    html.push_str("</div></div><div class=\"alert-body\">");
    html.push_str("<table><tbody>");
    html.push_str(&row("Originator", &escape_html(&alert.originator)));
    html.push_str(&row("Callsign", &escape_html(&alert.callsign)));
    html.push_str(&row(
        "Locations",
        &escape_html(&alert.location_names.join(", ")),
    ));
    html.push_str("</tbody></table>");
    html.push_str(&format!(
        "<h3>Message</h3><div class=\"message\">{}</div>",
        escape_html(&alert.message_text)
    ));

    html.push_str("<h3>Delivery Attempts</h3>");
    let attempts = data.delivery_attempts_by_alert.get(&alert.alert_id);
    if let Some(attempts) = attempts {
        render_attempts(html, attempts);
    } else {
        html.push_str("<p class=\"muted\">No delivery attempts recorded for this alert.</p>");
    }
    html.push_str("</div></article>");
}

fn render_attempts(html: &mut String, attempts: &[DeliveryAttemptRecord]) {
    html.push_str("<div class=\"table-wrap\"><table><thead><tr><th>Time</th><th>Sender</th><th>Role</th><th>Status</th><th>Channel</th><th>Error</th></tr></thead><tbody>");
    for attempt in attempts {
        html.push_str("<tr>");
        html.push_str(&cell(&attempt.timestamp_unix_secs.to_string()));
        html.push_str(&cell(&escape_html(&attempt.sender)));
        html.push_str(&cell(if attempt.required {
            "required"
        } else {
            "best-effort"
        }));
        html.push_str(&cell(&status_label(&attempt.status)));
        html.push_str(&cell(&option_u32(attempt.channel)));
        html.push_str(&cell(&escape_html(attempt.error.as_deref().unwrap_or(""))));
        html.push_str("</tr>");
    }
    html.push_str("</tbody></table></div>");
}

fn row(label: &str, value: &str) -> String {
    format!("<tr><th>{}</th><td>{}</td></tr>", escape_html(label), value)
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

    fn alert_record(alert_id: &str, event: &str) -> AlertRecord {
        AlertRecord {
            schema_version: EVENT_CONTRACT_SCHEMA_VERSION,
            record_type: "alert".to_string(),
            alert_id: alert_id.to_string(),
            timestamp_unix_secs: 10,
            source: "same".to_string(),
            event: event.to_string(),
            significance: AlertSignificance::Warning,
            originator: "National Weather Service".to_string(),
            callsign: "KXYZ".to_string(),
            is_national: false,
            is_test: false,
            location_codes: vec!["006085".to_string()],
            location_names: vec!["Central Santa Clara".to_string()],
            message_text: "alert <text>".to_string(),
        }
    }

    fn delivery_attempt(
        alert_id: &str,
        sender: &str,
        status: DeliveryAttemptStatus,
    ) -> DeliveryAttemptRecord {
        DeliveryAttemptRecord {
            schema_version: EVENT_CONTRACT_SCHEMA_VERSION,
            record_type: "delivery_attempt".to_string(),
            alert_id: alert_id.to_string(),
            timestamp_unix_secs: 11,
            sender: sender.to_string(),
            required: sender == "meshtastic",
            channel: Some(0),
            status,
            error: if sender == "discord" {
                Some("webhook unavailable".to_string())
            } else {
                None
            },
        }
    }

    fn sender_status(sender: &str, ready: bool) -> SenderStatusRecord {
        SenderStatusRecord {
            schema_version: EVENT_CONTRACT_SCHEMA_VERSION,
            record_type: "sender_status".to_string(),
            timestamp_unix_secs: 12,
            sender: sender.to_string(),
            configured: true,
            required: sender == "meshtastic",
            ready,
            last_success_unix_secs: if ready { Some(12) } else { None },
            last_failure_unix_secs: if ready { None } else { Some(12) },
            error: if ready {
                None
            } else {
                Some("unavailable".to_string())
            },
        }
    }

    fn dashboard_data() -> DashboardData {
        let mut attempts = HashMap::new();
        attempts.insert(
            "alert-1".to_string(),
            vec![
                delivery_attempt("alert-1", "meshtastic", DeliveryAttemptStatus::Success),
                delivery_attempt("alert-1", "discord", DeliveryAttemptStatus::Failure),
            ],
        );

        DashboardData {
            alerts: vec![alert_record("alert-1", "Tornado Warning")],
            delivery_attempts_by_alert: attempts,
            sender_statuses: vec![
                sender_status("meshtastic", true),
                sender_status("discord", false),
            ],
            malformed_lines: 0,
            parsed_records: 5,
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
        assert!(!html.to_lowercase().contains("cdn"));
    }

    #[test]
    fn summary_counts_render_correctly() {
        let html = render_dashboard("events.jsonl", &dashboard_data());

        assert!(html.contains("Operator Summary"));
        assert!(html.contains("<strong>1</strong><span>Recent Alerts</span>"));
        assert!(html.contains("<strong>1</strong><span>Senders Ready</span>"));
        assert!(html.contains("<strong>1</strong><span>Senders Not Ready</span>"));
        assert!(html.contains("<strong>1</strong><span>Delivery Success</span>"));
        assert!(html.contains("<strong>1</strong><span>Delivery Failure</span>"));
    }

    #[test]
    fn latest_sender_status_cards_render_correctly() {
        let html = render_dashboard("events.jsonl", &dashboard_data());

        assert!(html.contains("<strong>meshtastic</strong><span class=\"ok\">ready</span>"));
        assert!(html.contains("<strong>discord</strong><span class=\"bad\">not ready</span>"));
        assert!(html.contains("required sender"));
        assert!(html.contains("best-effort sender"));
    }

    #[test]
    fn alert_cards_include_grouped_delivery_attempts() {
        let html = render_dashboard("events.jsonl", &dashboard_data());

        assert!(html.contains("<article class=\"alert-card\">"));
        assert!(html.contains("<h3>Delivery Attempts</h3>"));
        assert!(html.contains("<td>meshtastic</td>"));
        assert!(html.contains("<td>discord</td>"));
        assert!(html.contains("webhook unavailable"));
    }

    #[test]
    fn health_warnings_render_clearly() {
        let data = DashboardData {
            read_error: Some("Unable to read event log: missing".to_string()),
            malformed_lines: 2,
            truncated_records: 4,
            ..DashboardData::default()
        };

        let html = render_dashboard("missing.jsonl", &data);

        assert!(html.contains("Unable to read event log: missing"));
        assert!(html.contains("Malformed event log lines were skipped."));
        assert!(html.contains("Display cap reached"));
        assert!(html.contains("No alert records found."));
    }

    #[test]
    fn auto_refresh_is_meta_refresh_only() {
        let html = render_dashboard("events.jsonl", &dashboard_data());

        assert!(html.contains("<meta http-equiv=\"refresh\" content=\"30\">"));
        assert!(!html.contains("<script"));
    }
}
