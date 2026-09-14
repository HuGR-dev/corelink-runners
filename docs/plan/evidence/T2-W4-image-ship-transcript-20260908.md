# T2-W4 image ship transcript (sanitized)

- Observed: 2026-09-08T00:24:56Z
- Operation: push only; no Worker/config change and no deploy.
- Local source image: `corelink-runner-devenv:sha-bca2c39`
- OCI revision label: `bca2c39`
- OCI source label: `https://github.com/HuGR-Labs/corelink-runners`
- Local platform: `linux/amd64`
- Local image ID: `sha256:d105e11f92718d610b390a71c37acb9bfb668da278b2f80c1f617f7cd068768c`
- Registry tag: `registry.cloudflare.com/6a1fc1c626fc2628823e60b9db01f5cd/corelink-runner-devenv:sha-bca2c39`
- Immutable remote reference: `registry.cloudflare.com/6a1fc1c626fc2628823e60b9db01f5cd/corelink-runner-devenv@sha256:d105e11f92718d610b390a71c37acb9bfb668da278b2f80c1f617f7cd068768c`
- Remote media type: `application/vnd.oci.image.manifest.v1+json`
- Remote config digest: `sha256:974a3ee3e964809fa588f13a4e06a6cf4c7f06db711db27da28822724f60455b`
- Remote layer count: `14`

The first `wrangler containers push` authenticated and uploaded layers but stalled before publishing a digest. It was stopped after more than four minutes. The same immutable tag was retried with the authenticated Docker client; retry exited `0`, and the registry returned digest `sha256:d105e11f92718d610b390a71c37acb9bfb668da278b2f80c1f617f7cd068768c`. No second tag was generated.

This evidence proves registry push and digest resolution only. It does not claim pinning, Worker configuration, deployment, rollout, or live runner qualification.
