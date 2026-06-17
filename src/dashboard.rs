use crate::dashboard_view::render_dashboard;
use crate::event_log_reader::{read_dashboard_data, MAX_DISPLAY_RECORDS};
use anyhow::{Context, Result};
use std::path::PathBuf;
use tiny_http::{Header, Response, Server, StatusCode};

pub const DEFAULT_DASHBOARD_BIND: &str = "127.0.0.1:8080";

pub fn run_dashboard(event_log_path: PathBuf, bind_address: &str) -> Result<()> {
    let server = Server::http(bind_address)
        .map_err(|e| anyhow::anyhow!("failed to bind dashboard service: {}", e))?;
    log::info!("Dashboard listening on http://{}", bind_address);

    for request in server.incoming_requests() {
        let html = render_dashboard_response(&event_log_path);
        let response = html_response(html)?;
        if let Err(e) = request.respond(response) {
            log::warn!("Failed to serve dashboard response: {}", e);
        }
    }

    Ok(())
}

pub fn render_dashboard_response(event_log_path: &PathBuf) -> String {
    let data = read_dashboard_data(event_log_path, MAX_DISPLAY_RECORDS);
    render_dashboard(&event_log_path.display().to_string(), &data)
}

fn html_response(html: String) -> Result<Response<std::io::Cursor<Vec<u8>>>> {
    let content_type = Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..])
        .map_err(|_| anyhow::anyhow!("failed to build content-type header"))
        .context("invalid dashboard content-type header")?;

    Ok(Response::from_string(html)
        .with_status_code(StatusCode(200))
        .with_header(content_type))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn missing_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "project-sentinel-missing-dashboard-log-{}-{}.jsonl",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn missing_file_produces_readable_dashboard_error() {
        let html = render_dashboard_response(&missing_path());

        assert!(html.contains("Unable to read event log"));
        assert!(html.contains("No alert records found."));
    }
}
