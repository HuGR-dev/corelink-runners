#!/usr/bin/env bash
# Focused static contract checks for the hosted, build-only RunnerDevEnv route.
set -euo pipefail

repo="$(git rev-parse --show-toplevel)"
workflow="${repo}/.github/workflows/build-cf-container-images.yml"

python3 - "${workflow}" <<'PY'
import re
import sys
from pathlib import Path


def reject(message):
    raise ValueError(message)


def job_block(workflow, name):
    matches = list(re.finditer(r"(?m)^  ([a-z0-9-]+):[ \t]*$", workflow))
    start = next((match for match in matches if match.group(1) == name), None)
    if start is None:
        reject(f"missing job: {name}")
    end = next((match.start() for match in matches if match.start() > start.start()), len(workflow))
    return workflow[start.start():end]


def validate(workflow):
    trigger_start = workflow.find("on:\n")
    permissions_start = workflow.find("\npermissions:", trigger_start)
    if trigger_start < 0 or permissions_start < 0:
        reject("workflow trigger boundary is missing")
    trigger = workflow[trigger_start:permissions_start]
    if "workflow_dispatch:" not in trigger or "pull_request:" in trigger or "push:" in trigger:
        reject("image workflow must remain manual-only")
    if any(value not in trigger for value in (
        "- production-publish", "- devenv-build-only", "- devenv-publish"
    )):
        reject("workflow_dispatch must expose the explicit production, proof, and DevEnv publication operations")
    if "expected_source_sha:" not in trigger:
        reject("manual DevEnv build mode must accept an expected full source SHA")

    guard = job_block(workflow, "validate-dispatch")
    if "runs-on: ubuntu-latest" not in guard:
        reject("dispatch validation must use GitHub-hosted Linux")
    if '"refs/heads/main"' not in guard or "EXPECTED_SOURCE_SHA" not in guard or "GITHUB_SHA" not in guard:
        reject("dispatch guard must bind production to main and DevEnv proof to exact SHA")

    publish = job_block(workflow, "build-and-push")
    if "needs: validate-dispatch" not in publish:
        reject("production publisher must wait for dispatch validation")
    if "inputs.operation == 'production-publish'" not in publish or "github.ref == 'refs/heads/main'" not in publish:
        reject("self-hosted publisher must require explicit production intent on main")
    if "runs-on: corelink" not in publish or "container-build-export-load.sh" not in publish:
        reject("existing production runner build and #575 bounded path must remain intact")

    hosted = job_block(workflow, "devenv-build-only")
    if "needs: validate-dispatch" not in hosted or "inputs.operation == 'devenv-build-only'" not in hosted:
        reject("hosted build must be reachable only after validated build-only dispatch")
    if "runs-on: ubuntu-latest" not in hosted:
        reject("DevEnv build proof must use GitHub-hosted Linux")
    if "ref: ${{ github.sha }}" not in hosted or "persist-credentials: false" not in hosted:
        reject("hosted job must checkout the exact dispatch SHA without persisted credentials")
    if "runner-devenv-build-contract.selftest.sh" not in hosted:
        reject("hosted build must run the focused contract test")
    for required in (
        "EXPECTED_SOURCE_SHA",
        'test "${EXPECTED_SOURCE_SHA}" = "${GITHUB_SHA}"',
        'test "$(git rev-parse HEAD)" = "${GITHUB_SHA}"',
        "deploy/cloudflare/Dockerfile.runner-devenv",
        "cp Cargo.toml Cargo.lock",
        "cp -R crates",
        "deploy/cloudflare/entrypoint.sh deploy/cloudflare/supervisord.conf",
        "docker buildx build",
        "--provenance=mode=max",
        'type=oci,name=${IMAGE_REF},dest=${ARCHIVE}',
        "--metadata-file",
        "scripts/ci/verify_runner_devenv_oci.py",
    ):
        if required not in hosted:
            reject(f"hosted DevEnv digest/provenance proof is missing: {required}")
    forbidden = (
        "secrets.",
        "CLOUDFLARE_API_TOKEN",
        "CLOUDFLARE_ACCOUNT_ID",
        "wrangler containers push",
        "wrangler deploy",
        "docker push",
        "runs-on: corelink",
    )
    for value in forbidden:
        if value in hosted:
            reject(f"hosted DevEnv proof contains a forbidden publish/provider path: {value}")

    hosted_publisher = job_block(workflow, "devenv-publish")
    if "runs-on: ubuntu-24.04" not in hosted_publisher:
        reject("DevEnv publication must use GitHub-hosted Ubuntu 24.04")
    if "needs: validate-dispatch" not in hosted_publisher or any(value not in hosted_publisher for value in (
        "inputs.operation == 'devenv-publish'",
        "github.ref == 'refs/heads/main'",
        "inputs.expected_source_sha == github.sha",
        'test "${EXPECTED_SOURCE_SHA}" = "${GITHUB_SHA}"',
        'test "$(git rev-parse HEAD)" = "${GITHUB_SHA}"',
        "wrangler containers push",
        '> "${OUTPUT}" 2>&1',
        "scripts/ci/resolve-pushed-ref.sh",
        "actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02",
        "retention-days: 90",
        "<redacted>",
    )):
        reject("GitHub-hosted DevEnv publication is missing an exact-main digest receipt boundary")
    for value in ("wrangler deploy", "wrangler.jsonc", "pull_request:", "packages: write"):
        if value in hosted_publisher:
            reject(f"hosted DevEnv publisher contains a forbidden deploy/pin/trigger path: {value}")
    for value in ('echo "${CLOUDFLARE_API_TOKEN}', 'echo "${CLOUDFLARE_ACCOUNT_ID}'):
        if value in hosted_publisher:
            reject("hosted DevEnv publisher logs a Cloudflare credential/account value")
    publisher_secrets = {
        line.strip()
        for line in hosted_publisher.splitlines()
        if "${{ secrets." in line
    }
    if publisher_secrets != {
        "CLOUDFLARE_ACCOUNT_ID: ${{ secrets.CLOUDFLARE_ACCOUNT_ID }}",
        "CLOUDFLARE_API_TOKEN: ${{ secrets.CLOUDFLARE_API_TOKEN }}",
    }:
        reject("hosted DevEnv publisher must use only the Cloudflare account and token secrets")


source = Path(sys.argv[1]).read_text(encoding="utf-8")
validate(source)
verifier_path = Path(sys.argv[1]).parents[2] / "scripts/ci/verify_runner_devenv_oci.py"
verifier = verifier_path.read_text(encoding="utf-8")
for required in (
    "def verify_member(",
    "descriptor_parts(",
    "containerimage.digest",
    "containerimage.config.digest",
    "containerimage.descriptor",
    "org.opencontainers.image.revision",
    "org.opencontainers.image.source",
    "vnd.docker.reference.type",
    "https://slsa.dev/provenance/",
    "index_digest",
    "image_manifest_digest",
    "GITHUB_STEP_SUMMARY",
):
    if required not in verifier:
        reject(f"OCI verifier is missing a required graph/provenance check: {required}")

# Negative cases ensure the assertions fail closed on the routing and evidence
# regressions that would otherwise make this proof unsafe or non-reproducible.
mutations = (
    ("missing main publication boundary", "build-and-push", "github.ref == 'refs/heads/main'", "github.ref == 'refs/heads/other'"),
    ("self-hosted DevEnv build", "devenv-build-only", "runs-on: ubuntu-latest", "runs-on: corelink"),
    ("missing provenance", "devenv-build-only", "--provenance=mode=max", "--provenance=disabled"),
    ("missing exact-SHA bind", "devenv-build-only", 'test \"${EXPECTED_SOURCE_SHA}\" = \"${GITHUB_SHA}\"', "true"),
    ("hosted registry publication", "devenv-build-only", 'type=oci,name=${IMAGE_REF},dest=${ARCHIVE}', "docker push"),
    ("missing workflow selftest", "devenv-build-only", "runner-devenv-build-contract.selftest.sh", "runner-devenv-contract-missing.sh"),
    ("publisher moved to corelink", "devenv-publish", "runs-on: ubuntu-24.04", "runs-on: corelink"),
    ("publisher missing exact SHA guard", "devenv-publish", "inputs.expected_source_sha == github.sha", "true"),
    ("publisher invokes deploy", "devenv-publish", "wrangler containers push", "wrangler deploy"),
    ("publisher reads an extra secret", "devenv-publish", "CLOUDFLARE_API_TOKEN: ${{ secrets.CLOUDFLARE_API_TOKEN }}", "CLOUDFLARE_API_TOKEN: ${{ secrets.OTHER_TOKEN }}"),
    ("publisher logs secret", "devenv-publish", "Build, publish, and capture immutable receipt", 'echo "${CLOUDFLARE_API_TOKEN}"'),
)
for label, job, before, after in mutations:
    block = job_block(source, job)
    if before not in block:
        reject(f"selftest mutation anchor missing: {label}")
    mutated_block = block.replace(before, after, 1)
    mutated = source.replace(block, mutated_block, 1)
    try:
        validate(mutated)
    except ValueError:
        continue
    reject(f"negative contract case passed unexpectedly: {label}")

print(f"runner-devenv-build-contract.selftest: PASS ({len(mutations) + 1} bounded contract cases)")
PY

python3 - "${repo}/scripts/ci/verify_runner_devenv_oci.py" <<'PY'
import hashlib
import importlib.util
import io
import json
import sys
import tarfile
import tempfile
from pathlib import Path

verifier_path = Path(sys.argv[1])
spec = importlib.util.spec_from_file_location("runner_devenv_oci_verifier", verifier_path)
verifier = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = verifier
spec.loader.exec_module(verifier)

source_sha = "a" * 40
image_ref = f"corelink-runner-devenv:{source_sha}"


def digest(data):
    return "sha256:" + hashlib.sha256(data).hexdigest()


def json_bytes(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode()


def fixture(directory, *, metadata_mode="complete", revision=source_sha,
            source="https://github.com/HuGR-dev/corelink-runners", bound=True,
            root_size_delta=0, ref_name=None):
    blobs = {}

    def put(data, media_type):
        value = digest(data)
        blobs[value] = data
        return {"mediaType": media_type, "digest": value, "size": len(data)}

    config = put(json_bytes({"config": {"Labels": {
        "org.opencontainers.image.revision": revision,
        "org.opencontainers.image.source": source,
    }}}), "application/vnd.oci.image.config.v1+json")
    layer = put(b"runner image layer", "application/vnd.oci.image.layer.v1.tar")
    image_manifest = json_bytes({
        "schemaVersion": 2,
        "config": config,
        "layers": [layer],
    })
    image = put(image_manifest, "application/vnd.oci.image.manifest.v1+json")
    subject = image["digest"].removeprefix("sha256:") if bound else "0" * 64
    statement = json_bytes({
        "_type": "https://in-toto.io/Statement/v1",
        "predicateType": "https://slsa.dev/provenance/v1",
        "subject": [{"name": image_ref, "digest": {"sha256": subject}}],
    })
    attestation_layer = put(statement, "application/vnd.in-toto+json")
    empty_config = put(b"{}", "application/vnd.oci.empty.v1+json")
    attestation_manifest = json_bytes({
        "schemaVersion": 2,
        "config": empty_config,
        "layers": [attestation_layer],
    })
    attestation = put(attestation_manifest, "application/vnd.oci.image.manifest.v1+json")
    index = {
        "schemaVersion": 2,
        "manifests": [
            {**image, "annotations": {
                "org.opencontainers.image.ref.name": ref_name or image_ref.rsplit(":", 1)[-1],
            }},
            {**attestation, "annotations": {
                "vnd.docker.reference.type": "attestation-manifest",
                "vnd.docker.reference.digest": image["digest"],
            }},
        ],
    }
    if root_size_delta:
        index["manifests"][0]["size"] += root_size_delta
    index_raw = json_bytes(index)
    index_digest = digest(index_raw)
    metadata = {}
    if metadata_mode == "complete":
        metadata = {
            "containerimage.digest": index_digest,
            "containerimage.config.digest": config["digest"],
            "containerimage.descriptor": {
                "mediaType": "application/vnd.oci.image.index.v1+json",
                "digest": index_digest,
                "size": len(index_raw),
            },
        }
    elif metadata_mode == "mismatch":
        metadata = {"containerimage.digest": "sha256:" + "f" * 64}
    elif metadata_mode == "config-mismatch":
        metadata = {"containerimage.config.digest": "sha256:" + "f" * 64}

    archive_path = directory / "image.oci.tar"
    with tarfile.open(archive_path, "w") as archive:
        members = {
            "oci-layout": json_bytes({"imageLayoutVersion": "1.0.0"}),
            "index.json": index_raw,
        }
        members.update({verifier.blob_path(key): value for key, value in blobs.items()})
        for name, data in members.items():
            member = tarfile.TarInfo(name)
            member.size = len(data)
            archive.addfile(member, io.BytesIO(data))
    metadata_path = directory / "metadata.json"
    metadata_path.write_text(json.dumps(metadata), encoding="utf-8")
    return archive_path, metadata_path


def expect_pass(label, **options):
    with tempfile.TemporaryDirectory() as temp:
        archive, metadata = fixture(Path(temp), **options)
        verifier.verify_archive(archive, metadata, image_ref, source_sha)


def expect_fail(label, **options):
    with tempfile.TemporaryDirectory() as temp:
        archive, metadata = fixture(Path(temp), **options)
        try:
            verifier.verify_archive(archive, metadata, image_ref, source_sha)
        except verifier.VerificationError:
            return
        raise SystemExit(f"OCI verifier negative case passed unexpectedly: {label}")


expect_pass("metadata fields present")
expect_pass("optional metadata fields omitted", metadata_mode="absent")
expect_fail("output digest mismatch", metadata_mode="mismatch")
expect_fail("config digest mismatch", metadata_mode="config-mismatch")
expect_fail("descriptor size mismatch", root_size_delta=1)
expect_fail("revision label mismatch", revision="b" * 40)
expect_fail("source label mismatch", source="https://example.invalid/repo")
expect_fail("SLSA subject not bound to runnable image", bound=False)
expect_fail("unexpected OCI ref name", ref_name="latest")
print("runner-devenv-oci-verifier.selftest: PASS (9 positive/negative graph cases)")
PY
