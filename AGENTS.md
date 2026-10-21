eternalist_foundry: fleet CI law, compiler and evidence engine; scheduling, evidence accounting, artifact identity, release publication

## gate
- `scripts/check` before landing; `./check.py check` mutates while editing

## invariants
- Applications own product behavior, fixtures, domain proof commands, storage, user stories; Cargo Packager owns native GUI containers
- Shared laws (audit, publishable graph, standard host tests) carry recipes here; never copy them into products
- `fleet` advances only through this repo's green `main` verdict; consumers call `pipeline.yml@fleet`; only a frozen product names a commit
- `rust-toolchain.toml` names the fleet compiler for every product; changing it is a fleet change
- Every baseline coordinate: release-tested, supported with explicit evidence tier, or excluded with concrete product reason
- Compilation ≠ runtime support; green matrix with missing receipt = failed run
