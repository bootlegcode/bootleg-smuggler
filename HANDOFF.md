# Smuggler workstream handoff

## Original implementation checkpoint (historical)

The scope, file list and test results below record the original implementation,
not tests rerun during documentation reconciliation. Current integration evidence
and compatible checkout guidance follow at the end.

### Scope / contract

Implemented only `bootleg-smuggler`. Read repository AGENTS.md, the assigned plan, frozen core contract and normative client-communication extension. No sibling repositories/shared contracts modified. Core revision used: `5d37918fb8aef80f9b5a8cc443e7241e0af1d419`. No contract change requested.

## Changed files

- `Cargo.toml`, `Cargo.lock`: executable and dependency graph (lockfile was initially untracked).
- `src/main.rs`, `src/lib.rs`: entrypoint/composition and module boundaries.
- `src/config.rs`: validated startup configuration, no-network policy, names, baseline/runtime claims.
- `src/lifecycle.rs`: explicit session/deadline/retry transitions and bounded notice memory.
- `src/network.rs`: bounded TLS WS adapter, attribution/bearer headers, registration/heartbeat, graceful close, reconnect and terminal failures.
- `src/process.rs`: real child launch, group signals, Linux subreaper, reaping, bounded escalation, meaningful exit codes.
- `src/network_tests.rs`: registration projection and two opt-in real-server tests.
- `tests/policies.rs`, `tests/contracts.rs`, `tests/processes.rs`: direct policies, frozen serialization fixtures, real children/signals.
- `tests/docker-smoke.sh`: actual Linux PID 1/container checks.
- `Dockerfile`, `examples/kind.yaml`: minimal Docker sample and two-replica/multi-container kind fixture.
- `README.md`, `HANDOFF.md`: operation, security, boundaries and integration status.

## Tests actually run at the original checkpoint

All final checks at that checkpoint passed:

- `cargo test --locked`: **18 passed**, 2 real-web tests ignored (macOS).
- `cargo clippy --all-targets --locked -- -D warnings`.
- `cargo fmt --check`.
- `docker build --target test -f Dockerfile -t smuggler-tests ..`: **19 passed**, 2 real-web tests ignored (Linux ARM64), plus Clippy. Includes adopted orphan reaping and 10-second shutdown escalation.
- `docker build -f Dockerfile -t smuggler-demo ..`.
- `bash tests/docker-smoke.sh`: disabled exec-through under `--network none`, enabled PID 1 SIGTERM with exit 42, PID 1 SIGKILL with exit 137.

Domain tests use real value objects/state transitions and explicit IDs/times, not mocked transport. Serialization fixtures are examples only. Process tests execute the real smuggler binary and real children. An initial Linux image lint step failed because Clippy was not installed; the Docker test target now installs it and the full final build passed.

## Proven invariants

- Disabled mode overrides invalid environment configuration; absent credentials exec through; partial credentials fail before launch.
- Deterministic label precedence, missing/empty selected names rejected, duplicate labels accepted.
- Unknown baseline remains unknown; only full SHA-1/SHA-256 accepted; secrets excluded from registration metadata/debug output.
- Container identity survives reconnect; sessions replace; wrong/stale/late acknowledgments fail; one heartbeat outstanding.
- Retry cap/jitter/minimum and reset only after heartbeat acknowledgment; permanent server codes disable participation.
- Notice deduplication is bounded and independent of child lifecycle.
- Child startup/exit does not depend on control-plane availability; real signals, forced termination and Linux orphan reaping work.

## Original integration blockers (since resolved by coordinated acceptance)

At the original checkpoint, no live control-plane URL plus eligible project credentials were supplied. Therefore the two standalone real-web tests had **not** run; the later coordinated suite below does not imply these opt-in tests were executed. Provide an isolated test project and runtime-injected credentials, then run:

`cargo test --locked real_web_ -- --ignored --test-threads=1`

The original remaining acceptance request was: live heartbeat/registration compatibility, rotation/deactivation, abrupt-loss expiry and stale-session fencing across replicas, discovery in CLI/UI, and kind deployment. `examples/kind.yaml` exists but was not applied to the existing cluster. Docker and kind were available; the original blocker was coordinated control-plane/credential setup, not a substituted mock service. This is historical context, not a current registration blocker.

## Coordinated acceptance update

The CLI-owned disposable harness subsequently completed actual-executable
registration acceptance, including rotation/deactivation, abrupt loss/reconnect,
CLI/Chromium discovery, plain Docker and an isolated kind cluster with two pods
and two instrumented containers per pod. Initial successful revisions were CLI
`f29e2cc`, web `515974d`, core `05144d6`, and smuggler `7869c5d`.
[Exact revisions and evidence](../bootleg-core/docs/registration-evidence.md),
[CLI reproduction/scenario guide](../bootleg-cli/docs/acceptance.md), and the
[sanitized successful report](../bootleg-cli/.pi/acceptance/bootleg-acceptance-d045c3a7f1ac/report.json)
record the scope and completed cleanup. Stale-session callbacks have supplemental
real-store/WebSocket protocol-client evidence, **not smuggler executable evidence**;
lifecycle, browser and kind checks use actual executables.

Use sibling `bootleg-{core,cli,smuggler,web}` checkouts under one parent. The CLI's
[`scripts/acceptance-revisions.json`](../bootleg-cli/scripts/acceptance-revisions.json)
is the enforced current dependency pin set; Cargo.lock cannot pin sibling path
sources. Later core quota clarification and web capacity changes are covered by
subsequent reports linked in the CLI guide. Documentation-only reconciliation
commits update these pins intentionally, without claiming a new acceptance run.
The original core revision above is historical, not the current required checkout.

No tests were rerun for this documentation update. The two standalone opt-in tests
remain separately runnable with isolated credentials; they are not an integration
blocker for the already completed coordinated registration milestone. Broader web
CI failures remain documented separately. Read-only CLI diff preview has since
completed, but no transfer, remote filesystem mutation or sync is implemented.

Application state reports running while the supervised child is active; presence is not readiness. No file sync, incoming arbitrary-command handling, probe proxy, Kubernetes API or Docker socket access was added. Base URLs currently accept origin-only deployment, not reverse-proxy subpath prefixes. Group signalling cannot guarantee cleanup of descendants that intentionally create a separate process group/session; PID 1 container teardown still applies.
