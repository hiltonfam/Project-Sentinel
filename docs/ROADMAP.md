# Sentinel Roadmap

## Completed

* [x] Phase 1 - Fan-out architecture
* [x] Phase 1.1 - Hardening and regression testing
* [x] Phase 2 - Optional Discord sender
* [x] Phase 2.1 - Optional best-effort failure spool
* [x] Phase 2.2 - Documentation rebrand
* [x] Phase 3 - Reticulum LXMF sender
* [x] Phase 4 - MeshCore sender
* [x] Phase 4.1 - PR automation safety rails
* [x] Phase 5 - Manual replay for spooled failures
* [x] Phase 5.1 - Documentation update
* [x] Phase 6.1 - Local event record contracts
* [x] Phase 6.2 - Opt-in event emission
* [x] Phase 6.3 - Read-only local dashboard service
* [x] Phase 6.4 - Dashboard operator polish
* [x] Phase 6.5 - Release engineering foundation

---

## Planned

* [ ] Phase 7 - NOAA/NWS API ingestion
* [ ] Phase 8 - Unified alert model for SAME and API alerts
* [ ] Phase 9 - Portable Sentinel deployment kits
* [ ] Phase 10 - Release packaging and operator install flow

---

## Explicit Non-Scope

Sentinel is not a general command/control platform. The roadmap does not include:

* ATAK integration
* GPS tracking
* Body camera integrations
* Map or mapping features
* Asset tracking platforms
* Team tracking platforms
* Unrelated incident command systems

---

## Guiding Principles

Sentinel will remain:

* Offline-first
* Modular
* Open-source
* Extensible
* Operator-focused
* Resilient during infrastructure failure
* Usable in both community and industrial emergency response environments

## Development Philosophy

Sentinel favors:

* Small milestones
* Extensive testing
* Offline-first operation
* Minimal dependencies
* Graceful degradation
* Clear separation between implemented capabilities and future vision
