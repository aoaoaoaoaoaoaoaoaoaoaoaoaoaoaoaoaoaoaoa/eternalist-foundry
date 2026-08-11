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
