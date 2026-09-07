# Shared image mirror rollout

This reconciles Agentsight draft PR
[#4](https://github.com/gauransh/agentsight/pull/4), head
`344b24759b2c79f1962740dc96e7dcb3d06d3658`, onto main
`e4e516daa2808669da62286d19de50d3113adbec`. It preserves the manual
`mirror-ecr` trigger and concurrency group, the `session-capture` image,
account `335971291626`, and explicit credentials. The copy logic is the
reviewed reusable workflow from sandbox
[#337](https://github.com/gauransh/itsm-sandbox-policy-engine/pull/337), pinned
to `918e1ff54bfda7b8ae31058726e0d3bf9979643a` instead of floating `@main`.

The credential-free validation job permits only `sha-<7..40 lowercase hex>`
before invoking the shared workflow. The shared implementation does not
enforce that tag restriction itself. Matching this syntax does not prove an
image came from an approved build: the operator still selects the artifact.
The reviewed ARO reconciliation uses the same preflight and interface; its
`aro` image is deliberately not copied into this repository's mapping.

## Merge and conflict order

1. Land sandbox #337 and its review fixes before adopting this caller. This
   is the operational publication/approval order. An accessible full SHA
   can resolve before its merge to main; GitHub does not require a branch
   reference. See the
   [reusable workflow reference](https://docs.github.com/en/actions/how-tos/reuse-automations/reuse-workflows#calling-a-reusable-workflow).
   If the upstream merge uses squash or rebase, preserve access to the
   pinned commit or update the pin to the reviewed merged commit and rerun
   the contract check.
2. Merge this reconciliation into current Agentsight main after final-base
   CI and review. It replaces #4's one-file caller change; do not merge both
   independently. Update or close #4 after its replacement lands, linking
   the replacement. The existing branch does not need a history rewrite.
3. ARO's corresponding caller can land independently after sandbox #337.
   Sandbox #338's infrastructure rollout and changes to the running
   session-capture image pin are separate operations.

If main moves, preserve its image, trigger and account changes while retaining
the reviewed immutable `uses` pin, `needs: validate`, and explicit secrets.
Check the resolved image is still `session-capture` and rerun the contract
test against the final result. Do not restore duplicate copy steps or change
to `secrets: inherit` to resolve a workflow conflict.

## Local validation

With Python 3.9+, PyYAML 6, and a sandbox checkout containing the pinned SHA:

```bash
rtk proxy python3 .github/tests/test_mirror_workflow.py /path/to/sandbox-checkout
rtk git diff --check
```

The three tests read the actual callee at the caller's pinned SHA without
fetching. They check required and unknown inputs/secrets, scalar types,
the `session-capture` image and account, permissions and dependency gate,
and fourteen executions of the actual Bash preflight: two accepted tags
and twelve rejected cases covering length boundaries, uppercase/non-hex
text, whitespace, newlines, shell text, paths and extra arguments. Python
and PyYAML are only local test dependencies; the workflow requests no
credentials and checks out no code during tag validation.

## Canary and rollout gates

Verify GitHub Actions access/allowlist settings permit this repository to
call the pinned sandbox workflow. The caller's token needs access to its
GHCR package. For OIDC, `AWS_ECR_MIRROR_ROLE_ARN` must name the existing
least-privilege mirror role, with trust admitting this caller repository
and dispatch ref. The shared workflow keeps the caller's OIDC subject;
the job requires `id-token: write`, `contents: read`, and `packages: read`.
The explicit `ECR_MIRROR_TOKEN` fallback remains available before OIDC
provisioning and expires after 12 hours. If both are set, OIDC is preferred;
a failed role assumption does not silently retry with the password.

After merging, choose one approved GHCR `session-capture` build whose
commit-derived tag is absent from the expected immutable ECR repository.
Record its source digest and platform manifests. Set `MIRROR_TAG` to that
approved tag, then dispatch:

```bash
rtk gh workflow run mirror-ecr.yml --repo gauransh/agentsight --ref main -f tag="${MIRROR_TAG:?Set an approved, existing GHCR sha tag}"
```

Check both jobs pass, the expected authentication path was used, and the
destination is
`335971291626.dkr.ecr.us-east-2.amazonaws.com/agent-sandbox/session-capture`.
Verify its tag and platform manifests against the approved source. Compare
top-level manifest digests as well: manifest/index conversion can change
them, so copy success alone does not prove provenance preservation. Stop
promotion on any unexplained difference. Do not delete or overwrite an
immutable tag to make a retry pass; an identical existing target can still
make copying fail, so inspect it before retrying or selecting another build.

Local tests do not prove GitHub access policy, OIDC trust, package access,
actual ECR immutability, registry behavior, or workload compatibility.
The manual registry canary supplies the first five checks; workload rollout
must separately verify the session-capture artifact before changing its
deployment pin. This workflow only copies an image and does not deploy it.

## Rollback

On canary failure, stop dispatching and retain the failed run and target tag
for investigation. Keep any copied artifacts in place. Running workloads
and existing image references are unchanged by this PR.

If the shared implementation regresses after adoption, restore the last
successfully canaried full SHA through a reviewed PR, keep tag validation
and explicit secrets, then repeat local checks and the registry canary.
This first adoption has no previous shared pin: reverting this PR restores
the local mirror workflow from `e4e516daa2808669da62286d19de50d3113adbec`.
That previous workflow needs the short-lived password and lacks the new
tag preflight; use only approved commit-derived tags and complete a fresh
canary before dispatches resume. Rollback requires no database migration,
Terraform apply, image deletion, or change to a running image.

## Verification record

On 2026-09-06, all three contract tests and fourteen Bash cases passed
against sandbox commit `918e1ff54bfda7b8ae31058726e0d3bf9979643a`. The tests
failed against the previous local workflow before this change. No registry
copy, OIDC canary, cloud configuration change, or deployment was executed.
Workflow adoption alone is not evidence of realized cloud savings.
