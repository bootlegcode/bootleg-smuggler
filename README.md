# Smuggler: container registration agent

Smuggler is an outbound-only entrypoint supervisor. This milestone provides registration and connectivity presence, **not file sync or application readiness**. Linux is the deployment target; Unix/macOS supports local process tests.

```sh
cargo build --locked
# No credentials: replaces itself with the application, with no network runtime.
./target/debug/smuggler -- /bin/sh -c 'echo hello'
# In your configured deployment, inject BOOTLEG_PROJECT_TOKEN from your secret manager.
# Never bake the token into an image.
BOOTLEG_PROJECT_ID=project_6m8x91b0349vbrq30fxn997q5s \
  ./target/debug/smuggler --name-env POD_NAME -- your-application
```

## Configuration

- `BOOTLEG_DISABLED=true`: unconditional no-network exec-through, ignoring other environment configuration.
- Both project ID and token absent: exec-through. Partial/invalid credentials: configuration error, exit 78, before application launch.
- `BOOTLEG_PROJECT_ID`: canonical public `project_…` TypeID (for example, `project_6m8x91b0349vbrq30fxn997q5s`), not a numeric ID or secret token. `BOOTLEG_PROJECT_TOKEN`: secret raw `project_…` bearer, supplied at runtime.
- Names: `--name` > `BOOTLEG_INSTANCE_NAME` > selected `--name-env` / `BOOTLEG_NAME_ENV` > `POD_NAME` > `KUBERNETES_POD_NAME` > `HOSTNAME` > OS hostname > short ContainerId. Flag name-env overrides env name-env. Missing/empty explicitly selected values are errors. Derived pod labels append `/BOOTLEG_CONTAINER_NAME` when supplied. Explicit name-env values are used verbatim.
- Optional immutable baseline: `BOOTLEG_BUILD_COMMIT` (full lowercase SHA-1/SHA-256; absent/empty is unknown), `BOOTLEG_BUILD_DIRTY=true|false` (absent is unknown).
- Unverified metadata: `POD_UID`, `POD_NAME`, `POD_NAMESPACE`, `BOOTLEG_CONTAINER_NAME`, `BOOTLEG_CLUSTER_NAME`. No Docker socket, orchestrator API, service-account token, or metadata endpoint access.

Enabled mode launches the child before starting network participation. The child has its own process group. TERM/INT/QUIT are forwarded, with SIGKILL escalation after 10 seconds; HUP is forwarded without scheduling termination. Linux uses subreaper adoption plus nonblocking waitpid to reap orphaned descendants. Normal child exits preserve the code; signal exits return 128 + signal. Remaining processes in the child's group are terminated when the main child exits. Applications that deliberately escape the process group are outside group-signalling guarantees. As with any supervisor, uninterruptible kernel tasks cannot be made to exit on a userspace deadline.

Connection failures never stop the child. Permanent authentication/configuration/protocol rejection disables participation until process restart. One ContainerId survives all reconnects; each accepted connection uses the server's new SmugglerSessionId. The client has bounded frames/write buffers/timeouts, one outstanding heartbeat, capped full-jitter retries, and no remote command handler. Announcements are plain stderr diagnostics with a bounded 128-ID FIFO deduplication cache.

## Advanced: connection overrides and local testing

Day to day, you should not need to set or change `BOOTLEG_API_URL` yourself: connection settings belong in your deployment configuration. Focus on your project ID, runtime-injected token, and application command.

For local testing or a custom installation, override `BOOTLEG_API_URL` with a credential-free HTTPS **origin**, without a path prefix, query, or fragment. Local plaintext HTTP additionally requires explicit `BOOTLEG_ALLOW_INSECURE=true`:

```sh
# Use an isolated local test project; inject its token separately.
BOOTLEG_API_URL=http://localhost:3000 BOOTLEG_ALLOW_INSECURE=true \
  BOOTLEG_PROJECT_ID=project_6m8x91b0349vbrq30fxn997q5s cargo run -- -- your-application
```

Implementation note: this checkout does not yet have a built-in hosted origin. Enabled deployments must currently supply `BOOTLEG_API_URL`; standalone developers testing registration must supply it too. Disabled/no-credentials mode needs no URL. A hosted default must be agreed before removing this deployment requirement.

## Boundaries

- `src/config.rs`: startup/no-op policy, name resolution, baseline and runtime claims, explicit inputs.
- `src/lifecycle.rs`: session transitions, deadlines, fencing of acknowledgments, retry policy and notice memory. No sockets/process access.
- `src/network.rs`: maps configuration to frozen core DTOs, bearer/header attribution, bounded WS adapter, TLS, registration/heartbeats/reconnect.
- `src/process.rs`: Unix process-group and Linux subreaper adapter.
- `src/main.rs`: argument/environment adapter and startup coordination.

Shared contract: sibling `bootleg-core` revision **`5d37918fb8aef80f9b5a8cc443e7241e0af1d419`**. No shared-contract changes required. The path dependency intentionally follows the coordinator's shared checkout; Cargo.lock does not pin a path dependency's source revision.

## Tests

```sh
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
cargo fmt --check
# Build context is parent directory; only explicitly listed source paths are copied.
docker build --target test -f Dockerfile -t smuggler-tests ..
docker build -f Dockerfile -t smuggler-demo ..
bash tests/docker-smoke.sh
```

Policies are tested directly, with explicit IDs/times/env maps and no API or WebSocket mocks. Process tests run real executable children/signals; Linux additionally checks orphan reaping. Shared fixtures test serialization, not pretend servers. Docker smoke checks run smuggler as PID 1 with `--network none`, including disabled mode, TERM and KILL.

Two opt-in tests require the **actual bootleg-web server** and an eligible project token in the environment (plus the explicit HTTP development setting if necessary):

```sh
cargo test --locked real_web_ -- --ignored --test-threads=1
```

They exercise registration/heartbeat, reconnect identity, duplicate names/new lifetime identity, and invalid credentials. They create real retained Container records; use an isolated test project. They do not provision a server or replace it with canned responses.

## Docker and kind

The Dockerfile's final image demonstrates `ENTRYPOINT ["/usr/local/bin/smuggler", "--"]` plus an application CMD. Persist a build-context-accurate full Git baseline via `--build-arg BOOTLEG_BUILD_COMMIT=…`; mark dirty builds with `--build-arg BOOTLEG_BUILD_DIRTY=true`. Default baseline is unknown/dirty, never falsely clean. Only the baseline, never a token, belongs in build arguments.

`examples/kind.yaml` supplies two replicas, each with two instrumented containers, explicit Downward API fields and runtime Secret references. The same pod label deliberately appears on both containers to exercise nonunique labels. Adapt the nonsecret API origin/project ID and load `smuggler-demo:latest` into kind. Provision `bootleg-project` Secret with key `token` through your normal secure workflow; no secret-bearing manifest is generated. Existing application probes should remain unchanged; this fixture adds no probe proxy. Service-account automount is disabled.

## Integration handoff / remaining acceptance

Verified locally: direct domain/serialization tests, real Unix children/signals, Linux orphan reaping and bounded shutdown, and real Docker PID 1 TERM/KILL behavior. See `HANDOFF.md` for the latest run counts.

Blocked pending coordinator-provided eligible test credentials and an actual reachable control plane: the two real-web tests, token rotation/deactivation while connected, observed lease expiry/fencing across replicas, CLI/UI discovery, and Docker/kind end-to-end registration. The kind manifest is provided but has not been deployed against a live control plane. No claims of completed end-to-end registration, permission isolation, exact file drift, or sync are made.
