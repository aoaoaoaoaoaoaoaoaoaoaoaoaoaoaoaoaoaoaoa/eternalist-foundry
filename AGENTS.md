eternalist_foundry: portfolio CI scheduling, evidence accounting, artifact identity, release publication

## gate
- `./check.py check` after edits; `./check.py verify` for non-mutating gate

## invariants
- Applications own product behavior, fixtures, proof commands, storage, user stories; `dist` owns CLI distribution; Cargo Packager owns native GUI containers
- Every baseline coordinate: release-tested, supported with explicit evidence tier, or excluded with concrete product reason
- Compilation ≠ runtime support; green matrix with missing receipt = failed run
