# Eternalist Foundry

Foundry turns a product-owned evidence contract into native CI jobs, verifies
that every planned proof returned a receipt, and emits the exact support
manifest from which Eternalist's public platform claims are derived.

It does not own application tests or package construction. Applications expose
independently runnable proof commands. [`dist`](https://github.com/axodotdev/cargo-dist)
builds CLI distributions; Cargo Packager builds native GUI containers. Foundry
schedules those engines, records their products, and refuses publication when
the declared support envelope and the surviving evidence differ.

The initial profiles are native GUI, portable CLI, Rust library, and explicitly
platform-bound Rust products. Baseline coordinates are closed values rather
than arbitrary runner labels.

```console
foundry check
foundry plan
foundry prove source
foundry judge evidence/
foundry support evidence/ support.json
foundry stage evidence/ release/
```

`foundry.toml` is the repository boundary. GitHub Actions is only one scheduler
over the same commands.

## Adoption

Each product keeps three small pieces of policy: `foundry.toml`, an exact
`rust-toolchain.toml`, and the product-owned commands named by its proofs. Its
workflow is the pinned caller in [`templates/ci.yml`](templates/ci.yml). The
caller grants publication as its maximum permission; the reusable workflow
contracts every nonpublication job back to read-only access.

The workflow pin advances only to a Foundry commit whose own native matrix and
verdict are green. Consumer repositories therefore inherit one immutable
scheduler, action set, tool versions, Linux display substrate, receipt schema,
and publication gate without copying their implementation. `actionlint` runs
inside every source cell, so drift in a consumer workflow fails before product
proof begins.

Before pushing a migration, validate the local boundary:

```console
foundry --contract foundry.toml check
foundry --contract foundry.toml plan
```

The complete native GUI contract is illustrated by
[`tests/fixtures/native-gui.toml`](tests/fixtures/native-gui.toml). A product
may combine several laws in one coordinate command when that avoids redundant
compilation; the receipt still records the exact law set and host.
Library and CLI integration proofs may declare `setup = "x11"` or
`setup = "wayland"` when their generic Linux coordinate needs a private display
substrate. Native GUI coordinates derive the same setup from their coordinate.

## Contract

Every baseline coordinate is exactly one of `release-tested`, `supported`, or
an exclusion with a concrete product reason. Release-tested means native
runtime evidence exists. Supported means the native host gate passes, but the
release carries no stronger claim.

Proofs discharge closed laws. Global proofs cover source, dependency security,
and the publishable Cargo graph. Coordinate proofs cover host behavior, first
present, lifecycle, X11 acceptance, and release artifacts. Delivery is a
platform fact rather than an architecture fact, so one witnessed universal DMG
may serve both native macOS coordinates. See
[`tests/fixtures/native-gui.toml`](tests/fixtures/native-gui.toml) for the full
native profile.

Each proof seals its exact command, source commit, native host, elapsed time,
run URL, and SHA-256 artifact inventory into a receipt. `judge` demands the
complete planned receipt graph and rejects stale commands, foreign hosts,
failed proofs, alien evidence cells, and altered artifacts. `stage` then emits
`support.json` and a flat, collision-free release directory containing only
judged artifacts.

Linux distribution is Cargo-first. Native GUI containers are unsigned DMGs and
current-user NSIS installers during incubation. Portable CLIs delegate archive
and installer construction to `dist`; Foundry schedules and judges that engine
instead of reimplementing it.
