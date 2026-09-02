#!/usr/bin/env bash
# T7-W4b / A7.6: fail-closed freshness gate for probe evidence.
set -Eeuo pipefail
IFS=$'\n\t'

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
root="${PROBE_FRESHNESS_ROOT:-$(cd -- "${script_dir}/../.." && pwd)}"
registry="${PROBE_FRESHNESS_REGISTRY:-${root}/docs/plan/evidence/freshness-v1.json}"
manifest="${PROBE_FRESHNESS_MANIFEST:-${root}/docs/plan/evidence/manifest-v1.json}"

usage() {
  cat >&2 <<'EOF'
usage: scripts/ci/probe-freshness-check.sh [--self-test]

Checks the committed freshness registry and every indexed probe artifact.  A
point-in-time observation must be no more than 24 hours before freeze; a
continuous observation uses its declared window end.  Future, missing,
malformed, stale, or version-unbound evidence fails closed.
EOF
}

case "${1:-}" in
  --help) usage; exit 0 ;;
  --self-test)
    exec python3 - "${script_dir}/probe-freshness-check.sh" <<'PY'
import hashlib
import json
import os
import pathlib
import subprocess
import tempfile

checker = pathlib.Path(__import__("sys").argv[1]).resolve()
FREEZE = "2026-09-02T00:00:00Z"
VERSION = "sha256:" + "a" * 64


def write(path, value, raw=False):
    path.parent.mkdir(parents=True, exist_ok=True)
    if raw:
        path.write_bytes(value)
    else:
        path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def git(root, *args):
    return subprocess.run(["git", "-C", str(root), *args], check=True,
                          text=True, stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE).stdout.strip()


def fixture(parent, label, *, artifact=True, raw_artifact=None,
            observed_at="2026-09-01T12:00:00Z", version=VERSION,
            temporal_kind="point", window_end_at=None, status="PASS",
            coverage="READY", manifest_anchor=True, manifest_version=None,
            registry_version=None):
    root = parent / label
    root.mkdir()
    artifact_path = "docs/plan/evidence/probe.json"
    artifact_value = {
        "schema_version": "evidence/v1",
        "artifact_id": "probe-current",
        "kind": "probe",
        "status": status,
        "observed_at": observed_at,
        "source": {"repository": "corelink-runners", "commit_sha": "0" * 40,
                    "path": artifact_path},
        "claims": ["A7.6"],
        "version": {"digest": version},
    }
    manifest_version = manifest_version or version
    registry_version = registry_version or version
    authority_path = "docs/plan/deployed-authority/fabricd.json"
    authority_record = {
        "schema_version": "deployed-version-authority/v1",
        "kind": "deployment-authority",
        "digest": manifest_version,
        "observed_at": "2026-09-01T12:00:00Z",
    }
    write(root / artifact_path, raw_artifact if raw_artifact is not None else artifact_value,
          raw=raw_artifact is not None)
    write(root / authority_path, authority_record)
    write(root / "docs/plan/evidence/manifest-v1.json", {
        "schema_version": "evidence-manifest/v1", "manifest_id": label,
        "coverage_status": coverage, "generated_at": FREEZE,
        "repository": "corelink-runners", "commit_sha": "0" * 40,
        "artifacts": [], "claims": [], "claim_sources": ["@tracked-markdown"],
        "deployed_versions": {},
    })
    write(root / "docs/plan/evidence/freshness-v1.json", {
        "schema_version": "evidence-freshness/v1", "registry_id": label,
        "repository": "corelink-runners", "freeze_at": FREEZE,
        "max_age_seconds": 86400, "max_future_skew_seconds": 0,
        "coverage_status": coverage,
        "deployed_versions": {"fabricd": {"digest": registry_version}},
        "artifacts": [],
    })
    git(root, "init", "-q")
    git(root, "config", "user.email", "selftest@example.invalid")
    git(root, "config", "user.name", "freshness-selftest")
    git(root, "add", ".")
    git(root, "commit", "-qm", "fixture")
    commit = git(root, "rev-parse", "HEAD")
    if artifact:
        if raw_artifact is None:
            artifact_value["source"]["commit_sha"] = commit
            write(root / artifact_path, artifact_value)
        else:
            # A malformed artifact is intentionally left uncommitted as bytes;
            # it is still indexed below to exercise parser failure.
            pass
        artifact_bytes = (root / artifact_path).read_bytes()
        entry = {"path": artifact_path, "artifact_id": "probe-current",
                 "temporal_kind": temporal_kind}
        if window_end_at is not None:
            entry["window_end_at"] = window_end_at
        registry = json.loads((root / "docs/plan/evidence/freshness-v1.json").read_text())
        registry["artifacts"] = [entry]
        write(root / "docs/plan/evidence/freshness-v1.json", registry)
        manifest = json.loads((root / "docs/plan/evidence/manifest-v1.json").read_text())
        manifest["commit_sha"] = commit
        manifest["artifacts"] = [{"path": artifact_path, "artifact_id": "probe-current",
                                  "sha256": hashlib.sha256(artifact_bytes).hexdigest()}]
        if manifest_anchor:
            manifest["deployed_versions"] = {"fabricd": {
                "digest": manifest_version,
                "authority": {
                    "commit_sha": commit,
                    "path": authority_path,
                    "sha256": hashlib.sha256((root / authority_path).read_bytes()).hexdigest(),
                },
            }}
        write(root / "docs/plan/evidence/manifest-v1.json", manifest)
        git(root, "add", ".")
        git(root, "commit", "-qm", "index fixture")
    return root


def run(root):
    env = os.environ.copy()
    env.update({"PROBE_FRESHNESS_ROOT": str(root),
                "PROBE_FRESHNESS_REGISTRY": str(root / "docs/plan/evidence/freshness-v1.json"),
                "PROBE_FRESHNESS_MANIFEST": str(root / "docs/plan/evidence/manifest-v1.json")})
    return subprocess.run([str(checker)], env=env, text=True,
                          stdout=subprocess.PIPE, stderr=subprocess.STDOUT)


def expect(root, good, label, needle=None):
    result = run(root)
    if (result.returncode == 0) != good:
        raise SystemExit(f"{label}: expected {'PASS' if good else 'FAIL'}, "
                         f"got {result.returncode}\n{result.stdout}")
    if needle is not None and needle not in result.stdout:
        raise SystemExit(f"{label}: expected diagnostic {needle!r}\n{result.stdout}")


with tempfile.TemporaryDirectory(prefix="corelink-freshness-selftest-") as tmp:
    parent = pathlib.Path(tmp)
    expect(fixture(parent, "source-bytes-diverge"), False,
           "artifact bytes must match source.commit_sha",
           "bytes differ from artifact at source.commit_sha")
    expect(fixture(parent, "registry-only-anchor", manifest_anchor=False), False,
           "registry-only deployment anchor", "manifest.deployed_versions.fabricd: must be an object")
    expect(fixture(parent, "divergent-deployed-version",
                   manifest_version="sha256:" + "b" * 64), False,
           "registry and manifest deployed versions diverge",
           "disagrees with manifest deployed version")

    expect(fixture(parent, "stale", observed_at="2026-08-31T23:59:59Z"), False,
           "24h+1s stale evidence")
    missing = fixture(parent, "missing")
    (missing / "docs/plan/evidence/probe.json").unlink()
    expect(missing, False, "missing artifact")
    expect(fixture(parent, "malformed", raw_artifact=b"{not-json\n"), False,
           "malformed artifact")
    expect(fixture(parent, "future", observed_at="2026-09-02T00:00:01Z"), False,
           "future artifact")
    expect(fixture(parent, "unbound", version="sha256:" + "b" * 64), False,
           "version-unbound artifact")
    expect(fixture(parent, "window-stale", observed_at="2026-08-31T00:00:00Z",
                   temporal_kind="continuous", window_end_at="2026-08-31T23:59:59Z"), False,
           "stale continuous window")
    empty = fixture(parent, "red-baseline", artifact=False, coverage="RED")
    expect(empty, False, "honest RED baseline")
print("probe-freshness-check selftest: PASS (11 negative cases; offline)")
PY
    ;;
  "") ;;
  *) usage; exit 2 ;;
esac

exec python3 - "${root}" "${registry}" "${manifest}" <<'PY'
import hashlib
import json
import pathlib
import re
import subprocess
import sys
from datetime import datetime, timezone

root, registry_path, manifest_path = (pathlib.Path(value).resolve() for value in sys.argv[1:])
errors = []
MAX_AGE = 86400
REGISTRY_FIELDS = {"schema_version", "registry_id", "repository", "freeze_at",
                   "max_age_seconds", "max_future_skew_seconds", "coverage_status",
                   "deployed_versions", "artifacts"}
REGISTRY_ARTIFACT_FIELDS = {"path", "artifact_id", "temporal_kind", "window_end_at"}
MANIFEST_FIELDS = {"schema_version", "manifest_id", "coverage_status", "generated_at",
                   "repository", "commit_sha", "artifacts", "claims", "claim_sources",
                   "deployed_versions"}
MANIFEST_ARTIFACT_FIELDS = {"path", "artifact_id", "sha256"}
DEPLOYED_VERSION_FIELDS = {"id", "digest", "authority"}
AUTHORITY_FIELDS = {"commit_sha", "path", "sha256"}
AUTHORITY_RECORD_FIELDS = {"schema_version", "kind", "id", "digest", "observed_at"}
ARTIFACT_FIELDS = {"schema_version", "artifact_id", "kind", "status", "observed_at",
                   "source", "claims", "version", "evidence", "notes"}
SOURCE_FIELDS = {"repository", "commit_sha", "path"}
VERSION_FIELDS = {"id", "digest"}
HEX40 = re.compile(r"^[0-9a-f]{40}$")
HEX64 = re.compile(r"^[0-9a-f]{64}$")
DIGEST = re.compile(r"^sha256:[0-9a-f]{64}$")


def fail(message):
    errors.append(message)


def reject_duplicates(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            raise ValueError(f"duplicate JSON key: {key}")
        value[key] = item
    return value


def load(path, label):
    try:
        with path.open(encoding="utf-8") as handle:
            return json.load(handle, object_pairs_hook=reject_duplicates)
    except (OSError, ValueError, UnicodeError) as exc:
        fail(f"{label}: unreadable or malformed JSON ({exc})")
        return None


def object_only(value, fields, label):
    if not isinstance(value, dict):
        fail(f"{label}: must be an object")
        return False
    unknown = sorted(set(value) - fields)
    if unknown:
        fail(f"{label}: unknown field(s): {', '.join(unknown)}")
    return not unknown


def required(value, fields, label):
    if not isinstance(value, dict):
        return False
    missing = sorted(set(fields) - set(value))
    if missing:
        fail(f"{label}: missing required field(s): {', '.join(missing)}")
        return False
    return True


def timestamp(value, label):
    if not isinstance(value, str) or not re.fullmatch(
            r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})", value):
        fail(f"{label}: must be RFC3339 with timezone")
        return None
    try:
        return datetime.fromisoformat(value.replace("Z", "+00:00")).astimezone(timezone.utc)
    except ValueError as exc:
        fail(f"{label}: invalid timestamp ({exc})")
        return None


def safe_path(value, label):
    if not isinstance(value, str) or not value or value.startswith("/") or "\\" in value:
        fail(f"{label}: path must be relative and POSIX")
        return None
    parts = pathlib.PurePosixPath(value).parts
    if any(part in ("", ".", "..") for part in parts):
        fail(f"{label}: unsafe path")
        return None
    candidate = (root / value).resolve()
    try:
        candidate.relative_to(root)
    except ValueError:
        fail(f"{label}: path escapes repository")
        return None
    return candidate


def tracked(path, label):
    relative = path.relative_to(root).as_posix()
    result = subprocess.run(["git", "-C", str(root), "ls-files", "--error-unmatch", "--", relative],
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    if result.returncode:
        fail(f"{label}: artifact is not tracked")
        return False
    return True


def git_bytes(commit, path):
    return subprocess.run(["git", "-C", str(root), "show", f"{commit}:{path}"],
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE)


def commit_exists(commit, label):
    result = subprocess.run(["git", "-C", str(root), "cat-file", "-e", f"{commit}^{{commit}}"],
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if result.returncode:
        fail(f"{label}: declared commit does not exist")
        return False
    return True


def committed_bytes(commit, relative_path, expected_sha, label):
    """Return immutable bytes only when their commit, path and digest all bind.

    A registry entry is merely an index.  The byte-bearing authority must be a
    tracked object in a reachable commit, whose content hash is explicit.
    """
    if not isinstance(commit, str) or not HEX40.fullmatch(commit):
        fail(f"{label}.commit_sha: must be 40 lowercase hex")
        return None
    path = safe_path(relative_path, f"{label}.path")
    if path is None:
        return None
    if not isinstance(expected_sha, str) or not HEX64.fullmatch(expected_sha):
        fail(f"{label}.sha256: must be 64 lowercase hex")
        return None
    if not commit_exists(commit, f"{label}.commit_sha"):
        return None
    result = git_bytes(commit, relative_path)
    if result.returncode:
        fail(f"{label}: declared path is absent at declared commit")
        return None
    if hashlib.sha256(result.stdout).hexdigest() != expected_sha:
        fail(f"{label}: committed bytes do not match sha256")
        return None
    return result.stdout


def version_valid(value, label):
    if not object_only(value, VERSION_FIELDS, label):
        return False
    if not isinstance(value, dict) or not (value.get("id") or value.get("digest")):
        fail(f"{label}: id or digest is required")
        return False
    if isinstance(value, dict) and "id" in value and (not isinstance(value["id"], str) or not value["id"]):
        fail(f"{label}.id: must be a non-empty string")
    if isinstance(value, dict) and "digest" in value and (not isinstance(value["digest"], str) or not DIGEST.fullmatch(value["digest"])):
        fail(f"{label}.digest: must be sha256:<64 lowercase hex>")
    return isinstance(value, dict)


def same_version(left, right):
    """Require both version records to name exactly the same immutable version."""
    if not isinstance(left, dict) or not isinstance(right, dict):
        return False
    return (set(left) == set(right) and
            all(left.get(field) == right.get(field) for field in left))


registry = load(registry_path, "freshness registry")
manifest = load(manifest_path, "evidence manifest")
if registry is None or manifest is None:
    print("probe freshness: RED (registry or manifest unavailable)", file=sys.stderr)
    sys.exit(1)

if object_only(registry, REGISTRY_FIELDS, "registry"):
    required(registry, {"schema_version", "registry_id", "repository", "freeze_at",
                        "max_age_seconds", "max_future_skew_seconds", "coverage_status",
                        "deployed_versions", "artifacts"}, "registry")
if object_only(manifest, MANIFEST_FIELDS, "manifest"):
    required(manifest, {"schema_version", "manifest_id", "coverage_status", "generated_at",
                        "repository", "commit_sha", "artifacts", "claims", "claim_sources",
                        "deployed_versions"}, "manifest")

if registry.get("schema_version") != "evidence-freshness/v1":
    fail("registry.schema_version: expected evidence-freshness/v1")
if manifest.get("schema_version") != "evidence-manifest/v1":
    fail("manifest.schema_version: expected evidence-manifest/v1")
for label, value in (("registry.registry_id", registry.get("registry_id")),
                     ("registry.repository", registry.get("repository")),
                     ("manifest.manifest_id", manifest.get("manifest_id")),
                     ("manifest.repository", manifest.get("repository"))):
    if not isinstance(value, str) or not value:
        fail(f"{label}: must be a non-empty string")
if registry.get("repository") != "corelink-runners" or manifest.get("repository") != "corelink-runners":
    fail("repository: expected corelink-runners")
if registry.get("coverage_status") not in {"RED", "READY"}:
    fail("registry.coverage_status: expected RED or READY")
if manifest.get("coverage_status") not in {"RED", "READY"}:
    fail("manifest.coverage_status: expected RED or READY")
if registry.get("coverage_status") != manifest.get("coverage_status"):
    fail("coverage_status: registry and manifest disagree")

manifest_commit = manifest.get("commit_sha")
if not isinstance(manifest_commit, str) or not HEX40.fullmatch(manifest_commit):
    fail("manifest.commit_sha: must be 40 lowercase hex")
elif not commit_exists(manifest_commit, "manifest.commit_sha"):
    pass

freeze = timestamp(registry.get("freeze_at"), "registry.freeze_at")
if registry.get("max_age_seconds") != MAX_AGE:
    fail("registry.max_age_seconds: must be exactly 86400")
if registry.get("max_future_skew_seconds") != 0:
    fail("registry.max_future_skew_seconds: must be exactly 0")

anchors = registry.get("deployed_versions")
if not isinstance(anchors, dict):
    fail("registry.deployed_versions: must be an object")
    anchors = {}
manifest_anchors = manifest.get("deployed_versions")
if not isinstance(manifest_anchors, dict):
    fail("manifest.deployed_versions: must be an object")
    manifest_anchors = {}

# An anchor is not evidence merely because the freshness registry says so.  It
# must be mirrored by the evidence manifest and bind an immutable, hashed,
# committed deployment-authority record, following the T7-W3 WP contract.
verified_anchors = {}
for name, anchor in anchors.items():
    label = f"registry.deployed_versions.{name}"
    if not isinstance(name, str) or not name or not version_valid(anchor, label):
        continue
    manifest_anchor = manifest_anchors.get(name)
    manifest_label = f"manifest.deployed_versions.{name}"
    if not object_only(manifest_anchor, DEPLOYED_VERSION_FIELDS, manifest_label):
        continue
    if not required(manifest_anchor, {"authority"}, manifest_label):
        continue
    manifest_version = {field: manifest_anchor[field] for field in ("id", "digest")
                        if field in manifest_anchor}
    if not version_valid(manifest_version, manifest_label):
        continue
    if not same_version(anchor, manifest_version):
        fail(f"{label}: disagrees with manifest deployed version")
        continue
    authority = manifest_anchor.get("authority")
    authority_label = f"{manifest_label}.authority"
    if not object_only(authority, AUTHORITY_FIELDS, authority_label) or not required(
            authority, AUTHORITY_FIELDS, authority_label):
        continue
    authority_path = authority.get("path") if isinstance(authority, dict) else None
    if isinstance(authority_path, str) and not authority_path.startswith("docs/plan/deployed-authority/"):
        fail(f"{authority_label}.path: must be under docs/plan/deployed-authority/")
        continue
    authority_bytes = committed_bytes(authority.get("commit_sha"), authority_path,
                                      authority.get("sha256"), authority_label)
    if authority_bytes is None:
        continue
    try:
        authority_record = json.loads(authority_bytes.decode("utf-8"),
                                      object_pairs_hook=reject_duplicates)
    except (UnicodeDecodeError, ValueError) as exc:
        fail(f"{authority_label}: must be UTF-8 JSON ({exc})")
        continue
    if not object_only(authority_record, AUTHORITY_RECORD_FIELDS, authority_label):
        continue
    if not required(authority_record, {"schema_version", "kind", "observed_at"}, authority_label):
        continue
    if (authority_record.get("schema_version") != "deployed-version-authority/v1" or
            authority_record.get("kind") != "deployment-authority"):
        fail(f"{authority_label}: invalid authority record kind/schema")
        continue
    timestamp(authority_record.get("observed_at"), f"{authority_label}.observed_at")
    authority_version = {field: authority_record[field] for field in ("id", "digest")
                         if field in authority_record}
    if not version_valid(authority_version, authority_label):
        continue
    if not same_version(anchor, authority_version):
        fail(f"{authority_label}: does not declare the registry/manifest version")
        continue
    verified_anchors[name] = anchor
for name in sorted(set(manifest_anchors) - set(anchors)):
    fail(f"manifest.deployed_versions.{name}: absent from freshness registry")

manifest_entries = manifest.get("artifacts")
if not isinstance(manifest_entries, list):
    fail("manifest.artifacts: must be an array")
    manifest_entries = []
indexed = {}
for index, entry in enumerate(manifest_entries):
    label = f"manifest.artifacts[{index}]"
    if not object_only(entry, MANIFEST_ARTIFACT_FIELDS, label) or not required(entry, MANIFEST_ARTIFACT_FIELDS, label):
        continue
    path = entry.get("path")
    if not isinstance(path, str) or not path or path in indexed:
        fail(f"{label}.path: must be unique and non-empty")
        continue
    if not isinstance(entry.get("artifact_id"), str) or not entry["artifact_id"]:
        fail(f"{label}.artifact_id: must be a non-empty string")
    if not isinstance(entry.get("sha256"), str) or not HEX64.fullmatch(entry["sha256"]):
        fail(f"{label}.sha256: must be 64 lowercase hex")
    indexed[path] = entry

registry_entries = registry.get("artifacts")
if not isinstance(registry_entries, list):
    fail("registry.artifacts: must be an array")
    registry_entries = []
seen = set()
valid_artifact_count = 0
for index, entry in enumerate(registry_entries):
    label = f"registry.artifacts[{index}]"
    if not object_only(entry, REGISTRY_ARTIFACT_FIELDS, label) or not required(entry, {"path", "artifact_id", "temporal_kind"}, label):
        continue
    path_value = entry.get("path")
    path = safe_path(path_value, f"{label}.path")
    if isinstance(path_value, str) and (
            not path_value.startswith("docs/plan/evidence/") or not path_value.endswith(".json")):
        fail(f"{label}.path: must be a JSON artifact under docs/plan/evidence/")
    artifact_id = entry.get("artifact_id")
    if path_value in seen:
        fail(f"{label}.path: duplicate registry entry")
    seen.add(path_value)
    if not isinstance(artifact_id, str) or not artifact_id:
        fail(f"{label}.artifact_id: must be a non-empty string")
    temporal_kind = entry.get("temporal_kind")
    if temporal_kind not in {"point", "continuous"}:
        fail(f"{label}.temporal_kind: expected point or continuous")
    if temporal_kind == "point" and "window_end_at" in entry:
        fail(f"{label}: point evidence cannot carry window_end_at")
    window_end = timestamp(entry.get("window_end_at"), f"{label}.window_end_at") if temporal_kind == "continuous" else None
    if temporal_kind == "continuous" and window_end is None:
        fail(f"{label}.window_end_at: required for continuous evidence")
    manifest_entry = indexed.get(path_value)
    if manifest_entry is None:
        fail(f"{label}: path is absent from evidence manifest")
        continue
    if manifest_entry.get("artifact_id") != artifact_id:
        fail(f"{label}: artifact_id disagrees with manifest")
    if path is None or not path.is_file():
        fail(f"{label}: artifact is missing")
        continue
    if not tracked(path, label):
        continue
    if hashlib.sha256(path.read_bytes()).hexdigest() != manifest_entry.get("sha256"):
        fail(f"{label}: bytes do not match manifest sha256")
    artifact = load(path, path_value)
    if artifact is None:
        continue
    if not object_only(artifact, ARTIFACT_FIELDS, path_value) or not required(
            artifact, {"schema_version", "artifact_id", "kind", "status", "observed_at", "source", "claims", "version"}, path_value):
        continue
    if artifact.get("schema_version") != "evidence/v1":
        fail(f"{path_value}.schema_version: expected evidence/v1")
    if artifact.get("artifact_id") != artifact_id:
        fail(f"{path_value}.artifact_id: disagrees with registry")
    if artifact.get("kind") not in {"probe", "test+probe"}:
        fail(f"{path_value}.kind: freshness applies only to probe/test+probe")
    if artifact.get("status") != "PASS":
        fail(f"{path_value}.status: only PASS evidence can satisfy freshness")
    observed = timestamp(artifact.get("observed_at"), f"{path_value}.observed_at")
    if freeze is not None and observed is not None:
        sample_end = window_end if temporal_kind == "continuous" else observed
        if sample_end is None:
            continue
        if observed > freeze:
            fail(f"{path_value}: observed_at is after freeze")
        if sample_end > freeze:
            fail(f"{path_value}: evidence end is after freeze")
        if temporal_kind == "continuous" and window_end < observed:
            fail(f"{path_value}: window_end_at precedes observed_at")
        age = (freeze - sample_end).total_seconds()
        if age > MAX_AGE:
            fail(f"{path_value}: evidence is older than 24 hours at freeze")
        if age < 0:
            fail(f"{path_value}: evidence is future-dated")
    source = artifact.get("source")
    if not object_only(source, SOURCE_FIELDS, f"{path_value}.source") or not required(source, SOURCE_FIELDS, f"{path_value}.source"):
        continue
    if source.get("repository") != "corelink-runners":
        fail(f"{path_value}.source.repository: expected corelink-runners")
    commit = source.get("commit_sha")
    source_path = source.get("path")
    if not isinstance(commit, str) or not HEX40.fullmatch(commit):
        fail(f"{path_value}.source.commit_sha: must be 40 lowercase hex")
    if source_path != path_value:
        fail(f"{path_value}.source.path: must equal registry path")
    if isinstance(commit, str) and HEX40.fullmatch(commit) and source_path == path_value:
        if commit != manifest_commit:
            fail(f"{path_value}.source.commit_sha: disagrees with manifest.commit_sha")
        elif commit_exists(commit, f"{path_value}.source.commit_sha"):
            result = git_bytes(commit, source_path)
            if result.returncode:
                fail(f"{path_value}: source commit does not contain the declared path")
            elif result.stdout != path.read_bytes():
                fail(f"{path_value}: bytes differ from artifact at source.commit_sha")
            elif hashlib.sha256(result.stdout).hexdigest() != manifest_entry.get("sha256"):
                fail(f"{path_value}: source.commit_sha bytes differ from manifest sha256")
    version = artifact.get("version")
    if not version_valid(version, f"{path_value}.version"):
        continue
    matches = []
    for anchor in verified_anchors.values():
        if not isinstance(anchor, dict):
            continue
        fields = set(version) | set(anchor)
        if all(version.get(field) == anchor.get(field) for field in fields if field in version and field in anchor):
            # A version carrying both id and digest must bind both; an anchor
            # missing either half is intentionally insufficient.
            if set(version).issubset(set(anchor)) and all(version.get(field) == anchor.get(field) for field in version):
                matches.append(anchor)
    if not matches:
        fail(f"{path_value}: version is not bound to a committed deployment anchor")
    valid_artifact_count += 1

for path in sorted(set(indexed) - seen):
    fail(f"manifest artifact {path}: missing from freshness registry")
if registry.get("coverage_status") == "READY":
    if not registry_entries:
        fail("coverage_status READY: no current probe evidence")
    if not verified_anchors:
        fail("coverage_status READY: no manifest-correlated deployment version anchors")
if registry.get("coverage_status") == "RED":
    fail("coverage_status RED: no evidence credit is available")
if not registry_entries and registry.get("coverage_status") != "RED":
    fail("empty freshness registry must be explicitly RED")
if errors:
    for error in errors:
        print(f"probe freshness: RED: {error}", file=sys.stderr)
    sys.exit(1)
print(f"probe freshness: PASS ({valid_artifact_count} version-bound artifact(s); max age 24h)")
PY
