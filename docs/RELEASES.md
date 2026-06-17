# Sentinel Releases

This document defines Sentinel's release process. Release work should be
repeatable, reviewable, and separate from new alerting features.

## Version Strategy

Sentinel product releases use semantic versioning:

```text
MAJOR.MINOR.PATCH
```

The current recommended initial Sentinel version is:

```text
v0.1.0
```

The authoritative Sentinel product version is stored in the repository `VERSION`
file. Release tags should use the `v` prefix, for example `v0.1.0`.

The Rust package name and Cargo package version are inherited from the upstream
Meshtastic SAME/EAS alerter. Until the package/binary rebrand is handled in a
separate milestone, release notes should identify both:

* Sentinel product version from `VERSION`.
* Built binary name, currently `Meshtastic-SAME-EAS-Alerter`.

## Supported Targets

Primary supported targets:

* Raspberry Pi / Linux ARM64: `aarch64-unknown-linux-gnu`
* Linux x86_64: `x86_64-unknown-linux-gnu`

Practical supported target:

* Windows x86_64: `x86_64-pc-windows-msvc`

The Windows build is useful for development and operator workstations. Field
deployment is expected to focus first on Linux and Raspberry Pi class systems.

## Release Checklist

Before tagging:

* Confirm the milestone contains no unrelated feature expansion.
* Confirm `VERSION` contains the intended product version.
* Update `CHANGELOG.md`.
* Run:

```sh
cargo fmt --check
cargo check
cargo test
```

* Confirm these Sentinel boundaries still hold:
  * Meshtastic remains the required primary sender.
  * Optional senders remain best-effort.
  * Failure spool remains opt-in.
  * Manual replay remains one-shot/manual.
  * Dashboard remains optional and read-only.
  * No command/control actions are introduced.

Tag and push:

```sh
git tag v0.1.0
git push origin v0.1.0
```

After CI completes:

* Download and smoke-test the generated artifacts where practical.
* Verify each artifact can print help:

```sh
Meshtastic-SAME-EAS-Alerter --help
```

* Create a GitHub release from the tag.
* Attach release artifacts if the workflow did not publish them automatically.
* Paste the relevant `CHANGELOG.md` entry into the GitHub release notes.

## CI Expectations

Pull requests and pushes should run:

* `cargo fmt --check`
* `cargo check`
* `cargo test`

Release tags and manual workflow runs should additionally attempt release builds
for the supported targets above. If a cross-target build fails because of a
toolchain or platform constraint, the release notes should call that out rather
than silently implying support.

## Scope Guardrails

Release engineering must not add new runtime alerting behavior.

In scope:

* Version files.
* Changelog.
* Release documentation.
* CI validation.
* Release artifact builds.

Out of scope:

* NOAA/NWS API ingestion.
* New senders.
* Dashboard features.
* ATAK.
* Maps.
* GPS tracking.
* Body camera integrations.
* Incident command or command/control platforms.
