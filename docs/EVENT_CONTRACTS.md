# Sentinel Local Event Contracts

## Purpose

These contracts define append-friendly local records that future Sentinel dashboard components can read. They are designed for local files, offline operation, and Pelican-case deployments where cloud services may be unavailable.

Phase 6.1 defined the data shapes. Phase 6.2 adds opt-in local event emission with `--event-log-path <PATH>`.

Sentinel does not emit these records unless `--event-log-path` is provided. No dashboard service, HTTP server, UI, database, scheduler, or background worker is introduced by event emission.

## Design Principles

* Local-first: records should be useful from files on the same host.
* Append-friendly: each record can be written as one JSON object per line.
* Dashboard-optional: Sentinel alert delivery must not depend on a dashboard reader.
* Stable enough to test: every record carries `schema_version`.
* Operationally plain: fields use simple strings, numbers, booleans, arrays, and nullable values.
* Source-preserving: alert text, SAME metadata, sender names, and status fields should remain inspectable without a remote service.

## File Format

The preferred on-disk format is JSON Lines:

```text
{"schema_version":1,"record_type":"alert",...}
{"schema_version":1,"record_type":"delivery_attempt",...}
```

Writers should append complete lines. Readers should tolerate malformed lines by reporting or skipping them rather than failing the whole dashboard view.

## Opt-In Emission

Event emission is disabled by default.

```sh
Meshtastic-SAME-EAS-Alerter --event-log-path sentinel-events.jsonl
```

When enabled, Sentinel appends event records to the configured file. Event write failures are logged as warnings only and do not block alert delivery.

Current emission behavior:

* `AlertRecord` is written after an alert passes SAME filtering and before fan-out delivery.
* `DeliveryAttemptRecord` is written after observable sender attempts.
* `SenderStatusRecord` is written after sender readiness checks.
* `SystemStatusRecord` remains a defined contract but is not emitted yet.

If `--event-log-path` is absent, Sentinel does not write event records.

## Common Fields

All records include:

* `schema_version`: integer contract version. Current value: `1`.
* `record_type`: string discriminator for the record type.
* `timestamp_unix_secs`: integer Unix timestamp in seconds.

Record ordering should be based on file order first and timestamp second. Offline deployments may have imperfect clocks.

## AlertRecord

Represents a normalized alert visible to the dashboard.

Fields:

* `schema_version`: `1`.
* `record_type`: `"alert"`.
* `alert_id`: local identifier used to connect delivery attempts to the alert.
* `timestamp_unix_secs`: time the alert record was created.
* `source`: alert source, initially expected to be `"same"`.
* `event`: alert event name.
* `significance`: SAME significance value, such as `"Warning"` or `"Test"`.
* `originator`: alert originator.
* `callsign`: source callsign when available.
* `is_national`: whether national alert override behavior applied.
* `is_test`: whether the alert is a test.
* `location_codes`: SAME location codes from the alert.
* `location_names`: resolved local names for location codes.
* `message_text`: final alert message text.

Example:

```json
{"schema_version":1,"record_type":"alert","alert_id":"alert-123","timestamp_unix_secs":1000,"source":"same","event":"Tornado Warning","significance":"Warning","originator":"National Weather Service","callsign":"KXYZ","is_national":false,"is_test":false,"location_codes":["006085"],"location_names":["Central Santa Clara"],"message_text":"Tornado warning message"}
```

## DeliveryAttemptRecord

Represents a send attempt for one sender.

Fields:

* `schema_version`: `1`.
* `record_type`: `"delivery_attempt"`.
* `alert_id`: alert identifier associated with this attempt.
* `timestamp_unix_secs`: time the attempt was recorded.
* `sender`: sender name, such as `"meshtastic"`, `"discord"`, `"lxmf"`, or `"meshcore"`.
* `required`: whether this sender is required for delivery success.
* `channel`: sender channel when applicable, otherwise `null`.
* `status`: `"Success"`, `"Failure"`, or `"Skipped"`.
* `error`: error summary when applicable, otherwise `null`.

Example:

```json
{"schema_version":1,"record_type":"delivery_attempt","alert_id":"alert-123","timestamp_unix_secs":1001,"sender":"meshtastic","required":true,"channel":0,"status":"Success","error":null}
```

## SenderStatusRecord

Represents a point-in-time view of a sender.

Fields:

* `schema_version`: `1`.
* `record_type`: `"sender_status"`.
* `timestamp_unix_secs`: time the status was recorded.
* `sender`: sender name.
* `configured`: whether Sentinel was configured with this sender.
* `required`: whether this sender is required.
* `ready`: whether the latest readiness check succeeded.
* `last_success_unix_secs`: latest known success time, or `null`.
* `last_failure_unix_secs`: latest known failure time, or `null`.
* `error`: latest error summary, or `null`.

Example:

```json
{"schema_version":1,"record_type":"sender_status","timestamp_unix_secs":1002,"sender":"discord","configured":true,"required":false,"ready":false,"last_success_unix_secs":900,"last_failure_unix_secs":1001,"error":"webhook unavailable"}
```

## SystemStatusRecord

Represents local host and Sentinel process context for dashboard display.

Fields:

* `schema_version`: `1`.
* `record_type`: `"system_status"`.
* `timestamp_unix_secs`: time the status was recorded.
* `hostname`: local hostname or deployment label.
* `sentinel_version`: Sentinel package version.
* `uptime_secs`: local process or host uptime when available, otherwise `null`.
* `disk_free_bytes`: local free disk space when available, otherwise `null`.
* `network_available`: local network availability when known, otherwise `null`.
* `notes`: operator- or probe-friendly notes.

Example:

```json
{"schema_version":1,"record_type":"system_status","timestamp_unix_secs":1003,"hostname":"sentinel-pi","sentinel_version":"0.8.0","uptime_secs":3600,"disk_free_bytes":1048576,"network_available":false,"notes":["offline mode"]}
```

## Dashboard Integration Boundary

Future dashboard components should read these records as local artifacts. Sentinel should remain able to decode SAME alerts, filter alerts, and deliver alerts through configured senders when no dashboard exists.

This contract does not require:

* HTTP server.
* Web UI.
* SQLite or another database.
* Cloud connectivity.
* Background workers.
* Dashboard-driven alert delivery.

## Future Compatibility Notes

When fields are added, prefer additive changes and increment `schema_version` only when readers need different parsing behavior. Avoid removing or renaming existing fields without a migration path.
