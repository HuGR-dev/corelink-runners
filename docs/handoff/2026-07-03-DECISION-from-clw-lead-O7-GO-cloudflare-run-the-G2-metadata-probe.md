# DECISION → corelink-runners TL — O7 = GO on Cloudflare Containers (owner-ratified). One confirming probe owed: G2.

> **From:** clw coordinator (go-live lead) · **cc** owner · **Date:** 2026-07-03

## Decision (owner-ratified): GO on CF Containers. Firecracker-self-managed is OFF the table.
The per-lease Firecracker microVM IS the isolation boundary. The two "app-layer gaps" the reviews found are
NOT security gaps: the missing sub-VM memory cap only affects intra-tenant cost/fairness (a job OOMs its OWN
VM, never a neighbor's — handled by per-tenant admission in the cf-multitenant packet), and the microVM is the
same Firecracker we'd otherwise self-build. Do not re-open the Firecracker option — it is permanently deferred.

## The ONE thing that still needs a fact, not a decision: G2 (metadata egress)
The only genuine residual risk is whether untrusted code in a lease can reach the cloud-metadata / link-local
surface (credential-theft vector). Our `deniedHosts` is best-effort only (no CIDR, raw-socket bypass — ADR-0009
now states this honestly). **Please run the live probe from INSIDE a running lease** and send me the raw output:
```
curl -s -m3 http://169.254.169.254/         # AWS/GCP-style IMDS
curl -s -m3 http://169.254.169.254/latest/meta-data/    # if the above responds
# + any CF-specific metadata/link-local address you know of
```
- **Nothing reachable / connection refused/timeout** → G2 closed at the platform network layer → **O7 fully GO**, I close it.
- **Anything sensitive reachable** → we mitigate at the NETWORK layer (egress proxy / allowlist / platform
  firewall) — NOT Firecracker. Send me the finding and I'll drive the mitigation.

This is the last open O7 item. Everything else (pids cap, exec-auth, egress kill-switch, honest ADR) is merged
(#272/#273). Reply with the probe output.

— clw coordinator
