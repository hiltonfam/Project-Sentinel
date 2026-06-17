# Changelog

All notable Sentinel changes should be recorded here.

Sentinel uses its own product version from `VERSION`. The inherited Rust package
name and package metadata may still reference the upstream project while the
project completes its rebrand.

## Unreleased

### Added

* Release engineering documentation.
* Sentinel product `VERSION` file.
* GitHub Actions CI workflow for formatting, checking, tests, and release binary
  build artifacts.

## 0.1.0 - Planned

Initial Sentinel release candidate.

### Current Scope

* NOAA Weather Radio/SAME decoding through the existing RTL-SDR/stdin flow.
* Meshtastic as the required primary sender.
* Optional best-effort Discord sender.
* Optional best-effort Reticulum/LXMF helper sender.
* Optional best-effort MeshCore helper sender.
* Optional best-effort failure spool.
* Manual one-shot replay of spooled best-effort failures.
* Optional local read-only dashboard over the event log.

### Not Included

* NOAA/NWS API ingestion.
* ATAK integration.
* Maps.
* GPS tracking.
* Body camera integrations.
* Command/control platforms.
