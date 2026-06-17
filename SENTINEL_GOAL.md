# Project Sentinel: NOAA Alert Relay and Mesh Forwarding Platform

## Mission

Build a resilient, portable, offline-capable alert relay that receives NOAA
weather alerts and forwards them across local emergency communications paths.

Sentinel is focused on two alert acquisition paths:

1. NOAA/NWS API ingestion when internet access is available.
2. NOAA Weather Radio/SAME decoding through RTL-SDR as the offline backup.

Sentinel forwards accepted alerts through:

* Meshtastic.
* Reticulum/LXMF.
* MeshCore.

Optional supporting features include:

* Discord notifications.
* Best-effort failure spool.
* Manual replay of spooled best-effort failures.
* Local read-only dashboard.

The platform should prioritize life safety, offline operation, modularity, and
reliability over feature breadth.

---

## Primary Objectives

1. Receive NOAA alerts from internet and offline radio sources.
2. Preserve NOAA Weather Radio/SAME as the offline-capable fallback path.
3. Apply deterministic alert filtering before forwarding.
4. Forward alerts through local mesh-capable communications systems.
5. Keep optional integrations best-effort so they cannot weaken primary delivery.
6. Remain deployable on small field hardware such as Raspberry Pi systems.

---

## Alert Sources

### NOAA/NWS API

Future Sentinel work should add internet-available NOAA/NWS API ingestion.

Requirements:

* Treat API ingestion as optional and network-dependent.
* Normalize API alerts into the same internal alert model used by SAME alerts.
* Preserve local filtering semantics where applicable.
* Never make internet API availability required for offline operation.

### NOAA Weather Radio / SAME via RTL-SDR

The existing RTL-SDR/SAME path remains the offline backbone.

Requirements:

* Decode SAME/EAS alerts from NOAA Weather Radio audio.
* Preserve county/location filtering.
* Preserve national alert override behavior.
* Preserve test alert behavior.
* Continue operating without internet access.

---

## Forwarding Networks

### Meshtastic

Meshtastic is Sentinel's required primary sender.

Requirements:

* Preserve existing Meshtastic CLI behavior.
* Preserve host, port, channel, chunking, and retry behavior.
* Treat Meshtastic failure as required delivery failure.

### Reticulum / LXMF

Reticulum/LXMF is an optional best-effort sender.

Requirements:

* Use helper-based delivery unless a later milestone explicitly approves native
  protocol support.
* Register only when required helper configuration is present.
* Never block Meshtastic delivery.

### MeshCore

MeshCore is an optional best-effort sender.

Requirements:

* Use helper-based delivery unless a later milestone explicitly approves native
  protocol support.
* Register only when required helper configuration is present.
* Never block Meshtastic delivery.

---

## Supporting Features

### Discord

Discord remains optional and best-effort.

Requirements:

* Register only when a webhook is configured.
* Avoid logging sensitive webhook values.
* Never block Meshtastic delivery.

### Failure Spool

The failure spool is an opt-in durability feature for best-effort sender
failures.

Requirements:

* Disabled by default.
* Enabled only when an operator provides a spool path.
* Records failed best-effort sender attempts.
* Does not spool required Meshtastic failures unless explicitly approved in a
  later milestone.

### Manual Replay

Manual replay retries spooled best-effort failures as a one-shot operator action.

Requirements:

* No background worker by default.
* No scheduler by default.
* Source spool remains read-only.
* Replay targets only configured best-effort senders.

### Local Read-Only Dashboard

The dashboard is an optional local visibility aid.

Requirements:

* Read-only.
* Localhost/LAN deployment only.
* Reads existing event data.
* Not in the alert delivery path.
* Sentinel must continue running headless without it.

---

## Offline-First Philosophy

Sentinel assumes internet connectivity may be unavailable.

Core functionality must continue operating without:

* Cloud services.
* External APIs.
* Cellular connectivity.
* Commercial power.

Internet connectivity, when available, may enhance alert acquisition through the
NOAA/NWS API, but it must never replace the local SAME decoding path.

---

## Hardware Objectives

Primary deployment platform:

* Raspberry Pi 4B or newer.

Secondary deployment options:

* Standard Linux systems.
* Windows operator workstations where practical.

Field deployment hardware:

* Portable enclosure.
* Battery power.
* RTL-SDR receiver.
* NOAA Weather Radio antenna.
* Meshtastic gateway node.
* Optional Reticulum/LXMF helper environment.
* Optional MeshCore helper environment.

---

## Development Priorities

### Near-Term

* Preserve and harden NOAA Weather Radio/SAME decoding.
* Preserve and harden Meshtastic forwarding.
* Keep Reticulum/LXMF and MeshCore helper senders best-effort.
* Improve event logging and local read-only visibility.
* Prepare repeatable releases.

### Future

* Add NOAA/NWS API ingestion as an internet-available supplement.
* Normalize API and SAME alerts into a shared internal model.
* Improve operational packaging for Raspberry Pi and Linux deployments.
* Continue strengthening tests around filtering, routing, forwarding, replay,
  and dashboard read models.

---

## Explicit Non-Scope

Sentinel is not a general command/control platform.

The following are not in scope:

* ATAK integration.
* GPS tracking.
* Body camera integrations.
* Map or mapping features.
* Asset tracking platforms.
* Team tracking platforms.
* Unrelated incident command systems.

---

## Success Criteria

The project is successful when:

* A NOAA Weather Radio/SAME alert can automatically propagate through
  Meshtastic and configured best-effort mesh senders within seconds.
* A NOAA/NWS API alert, when internet is available, can follow the same filtering
  and forwarding path.
* The system remains functional during a multi-day power or internet outage
  using the RTL-SDR/SAME path.
* Operators can inspect local delivery status without putting the dashboard in
  the alert delivery path.
* The solution remains maintainable, testable, and focused on NOAA alert relay
  and mesh forwarding.
