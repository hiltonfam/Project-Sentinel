# Sentinel Read-Only Dashboard

## Scope

Phase 6.3 adds a local read-only dashboard service for viewing Sentinel event logs created with `--event-log-path`.

The dashboard is optional. Sentinel alert delivery does not require it, and the dashboard is not in the alert delivery path.

## Run

Start Sentinel monitoring with event emission:

```sh
Meshtastic-SAME-EAS-Alerter --event-log-path sentinel-events.jsonl
```

Start the dashboard against that event log:

```sh
Meshtastic-SAME-EAS-Alerter --dashboard-event-log sentinel-events.jsonl
```

The default bind address is:

```text
127.0.0.1:8080
```

Use `--dashboard-bind <ADDR:PORT>` to override it:

```sh
Meshtastic-SAME-EAS-Alerter --dashboard-event-log sentinel-events.jsonl --dashboard-bind 127.0.0.1:9090
```

## Current Capabilities

The dashboard reads the JSONL event log on request and renders local HTML with inline CSS only.

It shows:

* Operator summary counts.
* Event log health.
* Source status for NOAA API and SAME Radio when `source_status` records exist.
* Recent alerts.
* Sender status summary cards.
* Delivery attempts grouped by alert with sender, role, status, channel, and error details.
* Malformed line count.
* Truncated record count when the display cap is reached.

The page uses a local HTML meta refresh so command-center displays can update without external assets or background workers.

Missing event logs render a readable dashboard error instead of panicking.

If no source status records are present, NOAA API and SAME Radio render as `unknown`.

## Boundaries

The dashboard does not include:

* Alert delivery.
* Command or control actions.
* Authentication.
* Maps.
* SQLite or another database.
* Background workers.
* Async runtime.
* Cloud services.
* CDN assets.

## Security Notes

The default dashboard bind is localhost only. Binding to a LAN address is an operator decision and should be limited to trusted local networks.
