# Sentinel

**Offline-first emergency alerting and disaster communications platform**

Sentinel is an open-source emergency communications platform designed to operate during natural disasters, industrial incidents, and prolonged infrastructure outages.

The project began as a fork of Meshtastic-SAME-EAS-Alerter and has evolved into a broader effort to provide resilient, multi-network alert dissemination and situational awareness capabilities.

Sentinel prioritizes:

* Offline operation
* Graceful degradation under failure
* Modular architecture
* Open-source development
* Interoperability between communication systems
* Support for both community and industrial emergency response

---

## Current Capabilities

### NOAA SAME Alert Monitoring

Sentinel receives NOAA Weather Radio broadcasts using an RTL-SDR and decodes SAME (Specific Area Message Encoding) alerts locally.

No internet connection is required.

Supported alert types include:

* Warnings
* Watches
* Statements
* Civil emergency messages
* National activations
* Weekly/monthly test alerts

---

### Meshtastic Integration (Required Sender)

Sentinel forwards qualifying alerts to Meshtastic networks using the Meshtastic Python CLI.

Features:

* Serial node support
* TCP node support
* Configurable alert channel
* Optional test alert channel
* County/location filtering
* National alert overrides

Meshtastic remains Sentinel's primary delivery mechanism.

---

### Discord Integration (Optional Sender)

Sentinel can optionally forward alerts to Discord using webhook URLs.

Features:

* Optional configuration
* Best-effort delivery
* Does not interfere with Meshtastic operation
* Discord failures do not prevent radio delivery

---

### Failure Spooling (Optional)

Sentinel can optionally record failed best-effort sender deliveries to a local spool file.

Features:

* Disabled by default
* File-backed durability
* No background worker required
* Does not impact Meshtastic delivery

---

## Architecture

Sentinel follows an offline-first fan-out architecture.

```text
RTL-SDR
    ↓
NOAA SAME Decoder
    ↓
Filtering Engine
    ↓
Alert Model
    ↓
Fan-Out Engine
     ├─ Meshtastic (required)
     ├─ Discord (optional)
     └─ Failure Spool (optional)
```

Future integrations will extend this architecture without replacing it.

---

## Installation

Installation instructions are currently focused on Raspberry Pi deployments using RTL-SDR and Meshtastic.

Detailed setup documentation will continue to evolve as Sentinel expands.

---

## Usage

### NOAA Monitoring Example

```bash
rtl_fm -f <FREQUENCY_HZ> -s 48000 -r 48000 | sentinel
```

### Discord Integration

```bash
rtl_fm -f <FREQUENCY_HZ> -s 48000 -r 48000 | sentinel \
  --discord-webhook-url <WEBHOOK_URL>
```

### Failure Spool

```bash
rtl_fm -f <FREQUENCY_HZ> -s 48000 -r 48000 | sentinel \
  --spool-path ./sentinel-spool.log
```

---

## Current CLI Options

* `--alert-channel`
* `--test-channel`
* `--host`
* `--port`
* `--rate`
* `--locations`
* `--discord-webhook-url`
* `--spool-path`

Run:

```bash
sentinel --help
```

for the latest options.

---

## Roadmap

The Sentinel roadmap is maintained in:

```text
docs/ROADMAP.md
```

Planned capabilities include:

* Reticulum / LXMF integration
* MeshCore integration
* Retry and replay workers
* Incident Command dashboard
* Skywarn ingestion
* ATAK interoperability

---

## Legal

Sentinel is an independent open-source project.

This project is not endorsed by or affiliated with:

* Meshtastic LLC
* The National Weather Service
* FEMA
* NOAA

Meshtastic® is a registered trademark of Meshtastic LLC.

Use at your own risk.

---

## Acknowledgements

Sentinel originated as a fork of:

RCGV1/Meshtastic-SAME-EAS-Alerter

We are grateful to the original authors and contributors whose work made Sentinel possible.
