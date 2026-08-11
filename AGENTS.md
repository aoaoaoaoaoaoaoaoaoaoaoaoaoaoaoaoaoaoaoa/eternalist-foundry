# Eternalist Foundry

Before meaningful Rust edits, read the canonical
[Rust Style Doctrine](/home/main/programming/projects/rust_starter/docs/rust-style-doctrine.md).

Foundry owns portfolio CI scheduling, evidence accounting, artifact identity,
and release publication. Applications own product behavior, fixtures, proof
commands, storage, and user stories. Packaging engines retain their own law:
`dist` owns CLI distribution and Cargo Packager owns native GUI containers.

Every baseline coordinate must be release-tested, supported with an explicit
evidence tier, or excluded with a concrete product reason. Compilation is not
runtime support. A green matrix with a missing receipt is a failed run.

Run `./check.py check` after meaningful edits and `./check.py verify` for the
non-mutating gate.

The owner's global Cargo configuration already routes targets to the separate
Cargo disk. Do not create a source-tree `target/` or override that policy.
