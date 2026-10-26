# Pinned Git distribution

`stable/ui-runtime-v1` maintains the current runtime integration line.
`ui-runtime-v1.0.0` names its initial fixed snapshot; existing tags must not move.
The integration tag does not change crate versions or imply that every platform
or the complete component test suite has been validated.

The source workspace retains the imported crate names. `packages/` is a separate,
checked-in Cargo workspace containing the `gpui-fast-*` distribution packages.
Cargo Git dependencies discover these packages directly; consumers need neither
source patches, a sibling checkout, Bun, nor a preparation command.

Maintain code in `crates/`, then run `bun script/prepare-git.ts` and commit the
source and generated packages together. The generator uses the official release
transformation, preserves Apache notices, and pins wgpu through a direct Git
dependency declared in `git-release.toml`. Internal Fast packages use relative
paths within the same Git checkout. Do not edit generated source files directly.

Depend on a complete commit SHA and retain Cargo.lock. Maintenance branches may
advance after validation; move consumers explicitly and issue a new integration
tag. The immutable source attribution is recorded in `packages/SOURCE.json`.
