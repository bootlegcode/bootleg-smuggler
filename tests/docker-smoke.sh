#!/usr/bin/env bash
# Real Linux PID 1 checks. Build image first; no web service or network mocks.
set -euo pipefail
image=${SMUGGLER_IMAGE:-smuggler-demo:latest}
name="smuggler-test-$$"
trap 'docker rm -f "$name" >/dev/null 2>&1 || true' EXIT
# --network none guarantees disabled mode cannot contact any service.
set +e
docker run --rm --network none -e BOOTLEG_DISABLED=true -e BOOTLEG_PROJECT_ID=invalid "$image" /bin/sh -c 'exit 17'
status=$?
set -e
[[ $status == 17 ]]
enabled=(-e BOOTLEG_API_URL=http://127.0.0.1:1 -e BOOTLEG_ALLOW_INSECURE=true -e BOOTLEG_PROJECT_ID=project_6m8x91b0349vbrq30fxn997q5s -e BOOTLEG_PROJECT_TOKEN=project_test-only)
docker run -d --name "$name" --network none "${enabled[@]}" "$image" /bin/sh -c 'trap "exit 42" TERM; echo ready; while :; do sleep 1; done' >/dev/null
for _ in {1..50}; do
  if docker logs "$name" 2>/dev/null | grep -q '^ready$'; then break; fi
  sleep 0.1
done
docker kill --signal TERM "$name" >/dev/null
[[ $(docker wait "$name") == 42 ]]
docker rm "$name" >/dev/null
# SIGKILL of PID 1 must tear down the container; no user-space handler can intercept it.
docker run -d --name "$name" --network none "${enabled[@]}" "$image" >/dev/null
docker kill --signal KILL "$name" >/dev/null
[[ $(docker wait "$name") == 137 ]]
echo 'Docker PID 1 smoke checks passed'
