# Project Sentinel: Unified Disaster Communications and Alert Platform

## Mission

Build a resilient, portable, offline-capable emergency communications and situational awareness platform designed for deployment during natural disasters, industrial incidents, and prolonged infrastructure outages.

The system must function as a self-contained Incident Command communications hub capable of operating with or without internet connectivity.

The platform should prioritize life safety, interoperability, modularity, and reliability over feature complexity.

---

## Primary Objectives

1. Provide automated alert dissemination from NOAA, SAME, NWS CAP feeds, and other emergency sources.
2. Operate during extended power and communications outages.
3. Bridge multiple independent communications networks into a unified alert ecosystem.
4. Provide a common operational picture for field personnel and incident command.
5. Be deployable in a Pelican case by a single individual within 15 minutes.
6. Support both community emergency response and industrial emergency management scenarios.

---

## Supported Communications Networks

### Meshtastic

* Primary low-bandwidth LoRa alerting network.
* Support:

  * Direct messages
  * Channel broadcasts
  * Multi-channel operation
  * MQTT bridging when internet is available.
* Maintain compatibility with upstream Meshtastic releases.

---

### Reticulum / LXMF

* Encrypted, decentralized messaging backbone.
* Support:

  * LXMF message delivery
  * Propagation nodes
  * Store-and-forward capability
  * Hybrid transport over Wi-Fi, Ethernet, and LoRa.
* Allow independent operation if Meshtastic is unavailable.

---

### MeshCore

* Local off-grid community messaging layer.
* Support:

  * Room broadcasts
  * Contact messaging
  * Gateway operation
  * CLI and API integration.

---

### Skywarn Integration

* Monitor designated amateur radio frequencies.
* Detect and process severe weather reports.
* Long-term objective:

  * AI-assisted voice transcription.
  * Extraction of actionable weather intelligence.
  * Conversion into structured alert objects.

---

### NOAA / SAME / CAP

* Support multiple alert acquisition methods:

  * RTL-SDR SAME decoding.
  * NOAA Weather Radio monitoring.
  * National Weather Service CAP API ingestion.
* SAME decoding shall remain the preferred offline alert source.
* Internet-based CAP shall supplement but never replace local SAME capability.

---

## Incident Command Dashboard

Develop a unified dashboard displaying:

### Network Status

* Meshtastic health.
* Reticulum health.
* MeshCore health.
* SDR health.
* Internet availability.
* Power status.

### Active Alerts

* Tornado warnings.
* Flash flood warnings.
* Civil emergency messages.
* Hazardous materials alerts.
* Industrial emergency notifications.

### Resource Tracking

* Node inventory.
* Field team status.
* GPS-enabled assets.
* Communication path visualization.

---

## Alert Fan-Out Engine

Create a common alert object.

All inbound alerts shall be normalized into this format before distribution.

Potential destinations include:

* Meshtastic
* Reticulum LXMF
* MeshCore
* Discord
* Email
* SMS gateways
* ATAK
* Incident Command dashboard
* Future integrations.

Filtering rules shall support:

* SAME county codes.
* Event type.
* Severity.
* Originator.
* National-level overrides.
* User-defined distribution groups.

---

## Offline-First Philosophy

The system shall assume internet connectivity is unavailable.

Core functionality must continue operating without:

* Cloud services.
* External APIs.
* Cellular connectivity.
* Commercial power.

Internet connectivity, when available, enhances capability but is never required for mission-critical operations.

---

## Hardware Objectives

Primary deployment platform:

* Raspberry Pi 4B or newer.

Secondary deployment options:

* LattePanda.
* Standard Linux systems.
* Containerized environments.

Field deployment hardware:

* Pelican case enclosure.
* LiFePO4 battery system.
* Solar charging capability.
* Meshtastic gateway node.
* Reticulum gateway node.
* MeshCore gateway node.
* RTL-SDR receivers.
* External antenna connections.

---

## Software Design Principles

1. Modular architecture.
2. Plugin-based integrations.
3. Open-source licensing.
4. Extensive automated testing.
5. Configuration-driven behavior.
6. Graceful degradation under failure conditions.
7. Minimal hardware requirements.
8. Comprehensive logging and observability.

---

## Development Priorities

### Phase 1

* NOAA SAME decoding.
* Meshtastic alert forwarding.
* County filtering.
* Test alert handling.

### Phase 2

* Discord integration.
* Reticulum LXMF integration.
* MeshCore integration.
* Alert spooling and retry mechanisms.

### Phase 3

* Unified Incident Command dashboard.
* Alert history and analytics.
* GPS asset visualization.
* Role-based user interfaces.

### Phase 4

* AI-assisted Skywarn transcription.
* Automatic severe weather intelligence extraction.
* ATAK interoperability.
* Predictive incident support tools.

### Phase 5

* Multi-case deployment support.
* High-availability clustering.
* Community mesh federation.
* Industrial incident command enhancements.

---

## Success Criteria

The project is successful when:

* A Tornado Warning received via NOAA SAME can automatically propagate through Meshtastic, Reticulum, MeshCore, and Incident Command interfaces within seconds.
* The system remains functional during a multi-day power outage.
* Field personnel receive actionable alerts regardless of which supported communications network they use.
* The platform can be deployed rapidly by a single operator.
* The solution remains maintainable and extensible for future emergency communications technologies.
