# Restore the native CI baseline

This work starts from Agentsight main
`e4e516daa2808669da62286d19de50d3113adbec`. The mirror caller in
[#5](https://github.com/gauransh/agentsight/pull/5) exposed existing native CI
failures. Keep their fixes in this separate change so registry adoption and
native CLI compatibility can be reviewed independently.

## Evidence and boundaries

The Linux `items_after_test_module` failure in `ext/vis/src/export.rs` also
failed on main in [run 33710185140](https://github.com/gauransh/agentsight/actions/runs/33710185140).
Moving the unchanged helper before the test module preserves strict Clippy.

The Windows job compiled unconditional Unix bridge APIs. Its bridge source,
module declaration and workflow match main; the Windows workflow only runs
for pull requests or manual dispatch, so no executed main comparison is
claimed. The Unix socket bridge remains Unix-only. On unsupported platforms,
requesting `--bridge-socket` must fail before capture or runtime extraction;
ordinary commands must still work. Native Windows CI is the final platform
gate, including its existing CLI and authentication smoke tests.

The macOS token smoke also failed on main in
[run 33710185161](https://github.com/gauransh/agentsight/actions/runs/33710185161).
The failing mirror run used Codex 0.153.4, which also passed against the
localhost mock in local macOS runs. The old substring gate also accepted
an incomplete JSON record that the production parser correctly skips. The
smoke now waits for a complete cumulative-usage event with bounded reads
and a monotonic deadline, then takes the same single collector snapshot and
asserts exactly 15 tokens. A regression test proves the premature gate;
the original zero-token CI output was not reproduced locally, so its precise
cause remains unproven until the native CI run. Runtime parsing is unchanged.

The full collector suite additionally reproduced a probe test reading files
before its mock Gemini child consumed stdin. The fixture now signals completed
consumption; a bounded wait precedes the unchanged argument and 64 KiB message
checks. Two export fixtures were hidden by more than 20 unrelated live agent
processes on the development host. They now use the existing command filter
with their unique session directory, retaining the 20-row limit and all
discovery, token, model, time and tool assertions. These are test changes.

## Merge and rollout

1. Run native Linux, Windows and macOS checks on this final branch. Investigate
   failures; do not exempt or disable a required job to make this baseline green.
2. Merge this baseline into main first. Merge the resulting main into #5,
   preserve its immutable reusable-workflow pin, explicit secrets and tag
   preflight, and run #5's final combined checks before merging it.
3. Recheck the intended diff if main changes: bridge platform guards belong
   to the CLI boundary, while mirror credentials and image mappings belong
   to the separate caller change. Keep concurrent CLI behavior and docs.
4. For a native release, use an exact tested artifact. On Unix, smoke the
   bridge with a private temporary socket and the existing authentication
   tests. On Windows, verify ordinary `top --plain --once`, `report`, and
   authenticated bind behavior, then confirm each requested bridge mode exits
   nonzero with the explicit unsupported-platform error before doing work.

This preparation does not publish a binary, copy a registry image, change
cloud infrastructure, or deploy a service. Mirror rollout remains governed
by the runbook in [#5](https://github.com/gauransh/agentsight/pull/5) after
the caller lands.

## Rollback

For a native release regression, retain logs and choose the last verified
artifact for that platform. The old main source does not compile on Windows;
blindly reverting this change is not a usable Windows recovery strategy.
Keep the platform guards and apply a forward fix, or use a known working
previous Windows release. On Unix, no store migration accompanies this
change, so a verified previous binary can be restored after stopping its
process and retaining captured session data.

The helper placement and smoke-test readiness changes affect build/test
behavior only. Reverting them reintroduces their CI failures without
repairing runtime state. Do not remove the strict Clippy or 15-token checks.

## Verification record

Local macOS validation on September 6/7, 2026. Final tested code is
`0ce4c9158dd9746ac7137a61be5d67f89a4d7c63`; the following commit adds only
this verification/runbook record.

- `rtk cargo test --manifest-path ext/vis/Cargo.toml --offline`: 18 passed.
- `rtk cargo test --manifest-path collector/Cargo.toml --locked --offline`:
  **160 passed, 4 ignored across 4 suites** in 86.03 seconds, including all
  155 unit tests and all 5 export fixtures, with localhost socket access.
- Collector and `agentvis` all-target Clippy with `-D warnings`: passed.
- Scoped Rust formatting, shell syntax and `git diff --check`: passed. The
  unrelated existing formatting difference in `server/capability.rs` is not
  included in this change; no whole-repository formatting pass is claimed.
- Seven readiness regressions passed, including partial JSON, unrelated and
  noncumulative totals, latest cumulative usage, deadline and input bounds.
- `script/ci/macos_top_smoke.sh` passed with real Codex **0.153.4**, temporary
  home directories and the localhost mock. The live row and overview both
  report **15 tokens**; prompt, model and process/path assertions passed.
- Independent code, security and Python reviews approved the changed paths.

The local host only has the macOS Rust target. Native Windows execution is
not claimed; the PR's Windows workflow is required before release. The four
existing authenticated provider smoke tests remain ignored by default and
were not run. No paid provider calls were made for this validation.
