# Deploying `corelink-fabricd` on Cloudflare Containers (gap-#1 option b)

The Rust control plane (RunnerLease API · §13 envelope · attestation key) as ONE
singleton CF Container, fronted by a thin proxy Worker, kept warm 24/7 by a cron
ping. The runner BOXES still spawn on `../cloudflare` (the spawn-Worker); this is
only the control-plane host. Env matrix + checkpoints: `docs/deploy/fabric-server.md`.

## Prerequisites (the only owner/machine actions)
1. **Docker daemon running** — the deploy builds the image locally (`cargo build
   --release` of the workspace). Start Docker Desktop: `open -a Docker`, wait for
   `docker info` to succeed.
2. **wrangler authed** — already logged in (`gmhelmold@gmail.com`); verify with
   `npx wrangler whoami`.

## Deploy
```sh
cd deploy/cloudflare-fabricd
npm install

# 1. Secrets (NEVER in wrangler.jsonc). Values come from the OOB secrets dir;
#    piped from file so the value is never echoed.
npx wrangler secret put FABRIC_SIGNING_KEY        < ~/.hugit/secrets/corelink/fabric-signing-key-prod
npx wrangler secret put FABRIC_INTROSPECT_AUTH_KEY < ~/.hugit/secrets/corelink/fabric-introspect-key
npx wrangler secret put BILLING_INGEST_AUTH_KEY    < ~/.hugit/secrets/corelink/billing-ingest-key

# 2. Deploy (builds + pushes the image, creates the Worker + container + DO + cron).
npm run deploy
```

The prod signing key was generated 2026-06-25 (32-byte ed25519, fingerprint
`9f54d5ee`); its PUBLIC half — `key_id faa5b7726ccd2c52`,
`pubkey_b64 Mo4wTL2QDnjL0inY7vasKHt1Jw7YIbAX3w2trY8824o=` — is what
`GET /v1/attestation/key` will serve and what hugit's v2 verifier pins. Setting a
DIFFERENT key changes that pubkey, so use that exact file.

## Smoke (checkpoint A/B/C)
```sh
HOST="https://corelink-fabricd.<account-subdomain>.workers.dev"   # printed by deploy
curl -s $HOST/v1/health                       # → ok
curl -s $HOST/v1/attestation/key              # → key_id faa5b7726ccd2c52 (the prod pubkey)
# acquire with a real tenant PAT → 200 Held; GET .../envelope/meta → 200 (not 404)
```
Then hand `$HOST` to the hugit TL as `HUGIT_RUNNER_HOST` + the spawn/lease PAT
(`HUGIT_RUNNER_PAT`), per the frozen Seam 1.

## Boxes (checkpoint B+ — when wiring real per-job metrics)
Add to `wrangler.jsonc` `vars`: `CLOUDFLARE_SPAWN_WORKER_URL` (the spawn-Worker URL),
and `npx wrangler secret put CLOUDFLARE_SPAWN_AUTH_TOKEN`. Until then the lease/§13/
attestation surface is live but `exec` returns 503 (no box backend) — fail-closed,
exactly as the dress-rehearsal showed.

## Status
⚠️ **NOT yet deploy-verified** — authored while the Docker daemon was down, so the
container build + the @cloudflare/containers env-injection lifecycle have not been
run end-to-end. The config mirrors the proven `../cloudflare` spawn-Worker; verify
on first deploy and adjust the container glue if the 0.3.x API differs.
