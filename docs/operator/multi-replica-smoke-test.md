# Multi-replica smoke test

Last verified: 2026-05-23

Run this procedure when scaling to `replicaCount > 1` for the first time, after a chart upgrade that touches the Deployment, PDB, or Service configuration, or when diagnosing a suspected cross-replica fan-out bug. For the runtime invariants that make symmetric multi-replica safe (per-replica `broadcast_watermark`, REPEATABLE READ snapshots, ring-buffer dedup), see [`docs/architecture/deployment.md`](../architecture/deployment.md) § Multi-replica.

## Prerequisites

A Kubernetes cluster (`kind create cluster`, `k3d cluster create`, OrbStack, or any homelab cluster), `kubectl`, `helm`, `node` (for the WebSocket tap), `curl`, and `jq`. `helm`, `kubeconform`, `node`, and `jq` are all provisioned by `mise install` — invocations below assume `mise activate` is wired into your shell, otherwise prefix them with `mise exec --`. A reachable PostgreSQL — provision in-cluster via the bitnami PG chart, or point `databaseUrl` at an existing instance.

> **WebSocket tap.** The runbook uses `scripts/ws-tap.js` (a ~30-line Node WebSocket client) instead of `wscat` for capturing event streams. `wscat` is readline/TTY-bound and silently produces no output when redirected to a file, so it can't be used in a scripted single-delivery assertion. `wscat` remains the right tool for interactive WebSocket debugging — install it ad-hoc with `mise use npm:wscat` if you need it for that.

## Procedure

```bash
set -euo pipefail

# 1. Provision PostgreSQL (skip if you already have one).
helm install pg oci://registry-1.docker.io/bitnamicharts/postgresql \
  --set auth.username=atc --set auth.password=atc --set auth.database=atc

# 2. Install ATC with two replicas pointing at the PG.
helm install atc deploy/helm/atc \
  --set replicaCount=2 \
  --set config.databaseUrl=postgres://atc:atc@pg-postgresql:5432/atc

# Wait for both pods Ready.
kubectl rollout status deploy/atc

# 3. Port-forward each pod to a distinct local port.
PODS=( $(kubectl get pod -l app.kubernetes.io/name=atc -o name) )
kubectl port-forward "${PODS[0]}" 8081:8080 >/tmp/pf-a.log 2>&1 &
PF_A=$!
kubectl port-forward "${PODS[1]}" 8082:8080 >/tmp/pf-b.log 2>&1 &
PF_B=$!

# Open a WebSocket tap to each pod's /v1/ws (capture each session to a logfile).
# scripts/ws-tap.js is a small pipe-friendly Node WebSocket client; see the note
# in the prerequisites section above for why this isn't wscat.
node scripts/ws-tap.js ws://localhost:8081/v1/ws > /tmp/ws-a.log 2> /tmp/ws-a.err &
WS_A=$!
node scripts/ws-tap.js ws://localhost:8082/v1/ws > /tmp/ws-b.log 2> /tmp/ws-b.err &
WS_B=$!

# 4. Start a /readyz watchdog covering steps 4–6 (fails the run if either replica
# returns non-200 at any sample point during the test window).
( while kill -0 $$ 2>/dev/null; do
    for port in 8081 8082; do
      code=$(curl -s -o /dev/null -w '%{http_code}' "http://localhost:${port}/readyz")
      if [[ "$code" != "200" ]]; then
        echo "FAIL: /readyz on port ${port} returned ${code}" >&2
        kill $$
        exit 1
      fi
    done
    sleep 0.5
  done ) &
READYZ_WATCH=$!

# 5. POST a webhook (replay a captured GitHub Actions webhook with valid HMAC).
# The route is /v1/webhooks/github (see backend/crates/atc-server/src/routes.rs).
curl -fsS -X POST http://localhost:8081/v1/webhooks/github \
  -H "X-GitHub-Event: workflow_run" \
  -H "X-Hub-Signature-256: sha256=<HMAC>" \
  --data @/path/to/captured-webhook.json

# 6. Within 5 seconds, both pod-local /v1/state endpoints should converge on the
# same lastSeq. Fail the run if convergence does not happen in time.
deadline=$(( $(date +%s) + 5 ))
converged=0
while (( $(date +%s) < deadline )); do
  A=$(curl -fsS http://localhost:8081/v1/state | jq -r .lastSeq)
  B=$(curl -fsS http://localhost:8082/v1/state | jq -r .lastSeq)
  if [[ "$A" == "$B" && "$A" != "null" && "$A" != "0" ]]; then
    echo "converged at lastSeq=$A"
    converged=1
    break
  fi
  sleep 0.1
done
if (( converged != 1 )); then
  echo "FAIL: /v1/state lastSeq did not converge within 5s (A=$A, B=$B)" >&2
  exit 1
fi

# 7. Each WS-tap logfile must show exactly one CommittedEvent for the webhook
# (single-delivery via ring-buffer dedup). Allow up to 2s for live delivery.
sleep 2
COUNT_A=$(grep -c '"seq":' /tmp/ws-a.log 2>/dev/null || true)
COUNT_B=$(grep -c '"seq":' /tmp/ws-b.log 2>/dev/null || true)
COUNT_A=${COUNT_A:-0}
COUNT_B=${COUNT_B:-0}
if (( COUNT_A != 1 )) || (( COUNT_B != 1 )); then
  echo "FAIL: expected exactly one CommittedEvent per replica (A=$COUNT_A, B=$COUNT_B)" >&2
  exit 1
fi
echo "single-delivery verified: A=1 B=1"

# 8. Tear down the watchdog and port-forwards. Reaching here means /readyz stayed
# 200 throughout (the watchdog kills $$ on any sample failure above).
kill "$READYZ_WATCH" "$WS_A" "$WS_B" "$PF_A" "$PF_B" 2>/dev/null || true
wait 2>/dev/null || true
echo "PASS: multi-replica smoke test"
```

## Pass criteria

- Both `/v1/state` endpoints converge on the same `lastSeq` within 5 seconds of the webhook POST.
- Each WS-tap logfile shows exactly one `CommittedEvent` for the webhook.
- Both `/readyz` endpoints return 200 throughout the test.

`kubectl logs -l app.kubernetes.io/name=atc -f --prefix` tags each line with the pod name — sufficient for "which replica did what" attribution during inspection. Per-process replica identification at the metrics layer is added at the collector by the standard target attributes (`pod`, `instance`) — the `atc_pg_*` metrics ship unlabeled per-process and dashboards aggregate `by (pod)`. See `docs/architecture/metrics.md` § Operational metrics for the per-metric scoping rules.

## Re-running against the same cluster

Replays of the same fixture against a non-empty PG return `{"status":"rejected"}` because the predicated `UPDATE … WHERE status IN (predecessors)` clause yields 0 rows on idempotent or invalid transitions (per [ADR 0002 D2](../architecture-decisions/0002-state-externalization-postgres-outbox.md)). To re-run cleanly, both PG state AND the per-replica `broadcast_watermark` must be reset:

```bash
# 1. Truncate the durable state.
kubectl -n atc-smoke exec deploy/postgres -- psql -U atc -d atc -c \
  "TRUNCATE outbox, jobs, runs RESTART IDENTITY CASCADE;"

# 2. Restart ATC pods so each replica re-reads COALESCE(MAX(seq),0)=0 into its
# in-process broadcast_watermark cursor. Without this step the drain task on
# each replica still has a non-zero watermark from the prior run, so the
# replayed seq=1 row is below-watermark and never broadcast.
kubectl -n atc-smoke rollout restart deploy/atc
kubectl -n atc-smoke rollout status deploy/atc
```
