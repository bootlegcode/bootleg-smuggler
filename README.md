# Smuggler: container registration agent

Smuggler is an outbound-only entrypoint supervisor that registers containers with Bootleg and reports connectivity. It does not sync files or report application readiness. Linux is the deployment target; macOS also supports local builds and process tests.

## Build from source

Clone Smuggler alongside the public [`bootleg-core`](https://github.com/bootlegcode/bootleg-core) repository. Use the core revision pinned in [`.github/bootleg-core.rev`](.github/bootleg-core.rev); `Cargo.lock` does not pin the revision of a path dependency. Run these commands from the directory where you want both checkouts, with [rustup](https://rustup.rs/) installed:

```sh
git clone https://github.com/bootlegcode/bootleg-smuggler.git
git clone https://github.com/bootlegcode/bootleg-core.git
read -r core_rev < bootleg-smuggler/.github/bootleg-core.rev
git -C bootleg-core checkout --detach "$core_rev"
cd bootleg-smuggler
cargo build --locked
```

## Quick start

Run an application locally without credentials or networking:

```sh
BOOTLEG_DISABLED=true ./target/debug/smuggler -- /bin/echo hello
```

To register a container, inject `BOOTLEG_PROJECT_TOKEN` at runtime from your secret manager. Replace the example origin and project ID with your own values:

```sh
BOOTLEG_API_URL=https://bootleg.example.com \
  BOOTLEG_PROJECT_ID=project_6m8x91b0349vbrq30fxn997q5s \
  ./target/debug/smuggler --name demo -- your-application
```

## Configuration

- `BOOTLEG_DISABLED=true`: unconditional no-network exec-through, ignoring other environment configuration.
- Both project ID and token absent: exec-through. Missing or malformed required settings: exit 78 before application launch.
- `BOOTLEG_PROJECT_ID`: canonical public `project_…` TypeID, not a numeric ID or secret token.
- `BOOTLEG_PROJECT_TOKEN`: secret raw `project_…` bearer. Supply it at runtime, never in source control, an image, or a build argument.
- `BOOTLEG_API_URL`: required for registration. Use a credential-free HTTPS origin with no path prefix, query, or fragment. There is no default origin.
- Names: `--name` > `BOOTLEG_INSTANCE_NAME` > selected `--name-env` / `BOOTLEG_NAME_ENV` > `POD_NAME` > `KUBERNETES_POD_NAME` > `HOSTNAME` > OS hostname > short ContainerId. Flag name-env overrides env name-env. Missing/empty explicitly selected values are errors. Derived pod labels append `/BOOTLEG_CONTAINER_NAME` when supplied. Explicit name-env values are used verbatim.
- Optional immutable baseline: `BOOTLEG_BUILD_COMMIT` (full lowercase SHA-1/SHA-256; absent/empty is unknown), `BOOTLEG_BUILD_DIRTY=true|false` (absent is unknown).
- Unverified metadata: `POD_UID`, `POD_NAME`, `POD_NAMESPACE`, `BOOTLEG_CONTAINER_NAME`, `BOOTLEG_CLUSTER_NAME`. No Docker socket, orchestrator API, service-account token, or metadata endpoint access.

Enabled mode launches the child before starting network participation. The child has its own process group. TERM/INT/QUIT are forwarded, with SIGKILL escalation after 10 seconds; HUP is forwarded without scheduling termination. Linux uses subreaper adoption plus nonblocking waitpid to reap orphaned descendants. Normal child exits preserve the code; signal exits return 128 + signal. Remaining processes in the child's group are terminated when the main child exits. Applications that deliberately escape the process group are outside group-signalling guarantees. As with any supervisor, uninterruptible kernel tasks cannot be made to exit on a userspace deadline.

Connection failures never stop the child. Permanent authentication/configuration/protocol rejection disables participation until process restart. One ContainerId survives all reconnects; each accepted connection uses the server's new SmugglerSessionId. The client has bounded frames/write buffers/timeouts, one outstanding heartbeat, capped full-jitter retries, and no remote command handler. Announcements are plain stderr diagnostics with a bounded 128-ID FIFO deduplication cache.

## Local registration testing

For a local server using plaintext HTTP, set `BOOTLEG_ALLOW_INSECURE=true` explicitly. Use HTTPS in deployments.

```sh
# Use an isolated local test project; inject its token separately.
BOOTLEG_API_URL=http://localhost:3000 BOOTLEG_ALLOW_INSECURE=true \
  BOOTLEG_PROJECT_ID=project_6m8x91b0349vbrq30fxn997q5s cargo run -- -- your-application
```

## Code layout

- `src/config.rs`: startup/no-op policy, name resolution, baseline and runtime claims, explicit inputs.
- `src/lifecycle.rs`: session transitions, deadlines, fencing of acknowledgments, retry policy and notice memory. No sockets/process access.
- `src/network.rs`: maps configuration to the shared protocol types and handles authentication, WebSocket transport, registration, heartbeats, and reconnects.
- `src/process.rs`: Unix process-group and Linux subreaper adapter.
- `src/main.rs`: argument/environment adapter and startup coordination.

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

## Container releases

Run `just` to list commands. From a clean `main` matching `origin/main`, use `just release` (patch), `just release minor`, or `just release major`. The recipe requires `just`, Python 3.11+, `vinnie`, Cargo, and the sibling `bootleg-core` checkout. It bumps `Cargo.toml` and `Cargo.lock`, commits the change, and pushes `main` and the `v`-prefixed tag together. If the push fails, fix the cause and retry the printed Git command rather than bumping again.

A newly pushed `v*` tag that matches the Cargo version runs the Linux checks and then builds the Dockerfile for `linux/amd64` and `linux/arm64`. Both platforms are published under a single versioned tag, `ghcr.io/bootlegcode/bootleg-smuggler:<git-tag>`; the workflow does not publish `latest`. Set the GHCR package visibility to **Public** after the first publish if you want anonymous pulls.

To run the published image after pushing a release (replace `v0.1.1` with the desired tag):

```sh
docker run --rm --network none -e BOOTLEG_DISABLED=true \
  ghcr.io/bootlegcode/bootleg-smuggler:v0.1.1 /bin/echo hello
```

To wrap your own application, copy `/usr/local/bin/smuggler` from the published image into a compatible glibc-based application image (such as Debian Bookworm) and configure it as the entrypoint.

## License

Smuggler is source-available under the [Business Source License 1.1](LICENSE).
