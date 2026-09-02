#!/usr/bin/env bash
# T7-W3 / A7.4-A7.5: offline, fail-closed claim/evidence checker.
set -Eeuo pipefail
IFS=$'\n\t'

script_dir="$(cd -- "$(dirname -- "$0")" && pwd)"
root="${CLAIM_ARTIFACT_ROOT:-$(cd -- "$script_dir/../.." && pwd)}"
manifest="${CLAIM_ARTIFACT_MANIFEST:-$root/docs/plan/evidence/manifest-v1.json}"
schema="${CLAIM_ARTIFACT_SCHEMA:-$root/docs/plan/evidence/schema-v1.json}"

usage() {
  cat >&2 <<'EOF'
usage: scripts/ci/claim-artifact-lint.sh [--self-test]

Checks every eligible tracked Markdown source and every committed evidence
artifact. A capability claim is marked inline:
  <!-- corelink-claim id="<claim-id>" artifact="<artifact-id>" -->
EOF
}

if [[ ${1:-} == --help ]]; then usage; exit 0; fi
if [[ ${1:-} == --self-test ]]; then
  exec python3 - "$script_dir" <<'PY'
import hashlib, json, os, pathlib, subprocess, sys, tempfile

script_dir = pathlib.Path(sys.argv[1])
checker = script_dir / "claim-artifact-lint.sh"
digest = "sha256:" + "a" * 64

def git(root, *args):
    return subprocess.run(["git", "-C", str(root), *args], check=True, text=True,
                          stdout=subprocess.PIPE).stdout.strip()

def run(root):
    env = os.environ.copy(); env["CLAIM_ARTIFACT_ROOT"] = str(root)
    return subprocess.run([str(checker)], env=env, text=True,
                          stdout=subprocess.PIPE, stderr=subprocess.STDOUT)

def expect(root, good, label):
    result = run(root)
    if (result.returncode == 0) != good:
        raise SystemExit(f"{label}: expected {'PASS' if good else 'FAIL'}; got {result.returncode}\n{result.stdout}")

def put(root, name, text):
    path = root / name; path.parent.mkdir(parents=True, exist_ok=True); path.write_text(text)

def case(root, *, text="the moat is live", marker=True, status="PASS", authority=True,
         authority_digest=digest, nested=False, duplicate=False, coverage="READY"):
    put(root, "docs/plan/evidence/schema-v1.json", (script_dir.parent.parent / "docs/plan/evidence/schema-v1.json").read_text())
    claim = '<!-- corelink-claim id="A7.4.moat" artifact="moat-live" -->'
    put(root, "docs/proof.md", f"{text} {claim if marker else ''}\n")
    authority_path = "docs/plan/deployed-authority/fabricd.json"
    authority_record = {"schema_version":"deployed-version-authority/v1",
      "kind":"deployment-authority", "digest":authority_digest,
      "observed_at":"2026-09-02T12:00:00Z"}
    put(root, authority_path, json.dumps(authority_record, sort_keys=True) + "\n")
    git(root, "init", "-q"); git(root, "config", "user.email", "selftest@example.invalid")
    git(root, "config", "user.name", "selftest"); git(root, "add", "."); git(root, "commit", "-qm", "authority")
    commit = git(root, "rev-parse", "HEAD")
    artifact_path = "docs/plan/evidence/moat-live.json"
    payload = {"schema_version":"evidence/v1", "artifact_id":"moat-live", "kind":"test",
      "status":status, "observed_at":"2026-09-02T12:00:00Z",
      "source":{"repository":"corelink-runners", "commit_sha":commit, "path":artifact_path},
      "claims":["A7.4.moat"], "version":{"digest":digest}}
    put(root, artifact_path, json.dumps(payload, indent=2) + "\n")
    auth = {"commit_sha":commit, "path":authority_path,
            "sha256":hashlib.sha256((root / authority_path).read_bytes()).hexdigest()}
    deployed = {"fabricd":{"digest":digest, "authority":auth}}
    if not authority: deployed["fabricd"].pop("authority")
    artifact_sha = hashlib.sha256((root / artifact_path).read_bytes()).hexdigest()
    manifest = {"schema_version":"evidence-manifest/v1", "manifest_id":"selftest",
      "coverage_status":coverage, "generated_at":"2026-09-02T12:00:00Z",
      "repository":"corelink-runners", "commit_sha":commit,
      "artifacts":[{"path":artifact_path,"artifact_id":"moat-live","sha256":artifact_sha}],
      "claims":[{"id":"A7.4.moat","file":"docs/proof.md","line":1,"artifact_id":"moat-live"}],
      "claim_sources":["@tracked-markdown"], "deployed_versions":deployed}
    if duplicate:
        manifest["claims"].append(dict(manifest["claims"][0]))
    if nested: put(root, "docs/plan/evidence/hidden/probe.json", "{}\n")
    put(root, "docs/plan/evidence/manifest-v1.json", json.dumps(manifest, indent=2) + "\n")

with tempfile.TemporaryDirectory(prefix="corelink-claim-selftest-") as tmp:
    root = pathlib.Path(tmp) / "valid"; root.mkdir(); case(root); expect(root, True, "valid committed evidence")
    for phrase in ("the moat is live", "cache-warm boot is live", "benchmark 3x faster"):
        root = pathlib.Path(tmp) / phrase.replace(" ", "-"); root.mkdir(); case(root, text=phrase, marker=False)
        expect(root, False, f"unmarked claim mutant {phrase}")
    root = pathlib.Path(tmp) / "negative"; root.mkdir(); case(root, status="FAIL"); expect(root, False, "negative evidence")
    root = pathlib.Path(tmp) / "self-attested"; root.mkdir(); case(root, authority=False); expect(root, False, "self-attested deploy version")
    root = pathlib.Path(tmp) / "wrong-authority"; root.mkdir(); case(root, authority_digest="sha256:" + "b" * 64); expect(root, False, "mismatched authority version")
    root = pathlib.Path(tmp) / "duplicate"; root.mkdir(); case(root, duplicate=True); expect(root, False, "duplicate claim id")
    root = pathlib.Path(tmp) / "nested"; root.mkdir(); case(root, nested=True); expect(root, False, "nested unindexed evidence")
    root = pathlib.Path(tmp) / "red"; root.mkdir(); case(root, coverage="RED"); expect(root, False, "red baseline")
print("claim-artifact-lint selftest: PASS (10 cases; offline)")
PY
fi
if [[ $# -ne 0 ]]; then usage; exit 2; fi

exec python3 - "$root" "$manifest" "$schema" <<'PY'
import hashlib, json, pathlib, re, subprocess, sys
from datetime import datetime

root, manifest_path, schema_path = (pathlib.Path(arg).resolve() for arg in sys.argv[1:])
errors = []
MARKER = re.compile(r'<!--\s*corelink-claim\s+id="([A-Za-z0-9][A-Za-z0-9._:-]{1,127})"\s+artifact="([A-Za-z0-9][A-Za-z0-9._-]{2,127})"\s*-->')
PRESENT = re.compile(r'\b(?:is|are|runs?|operates?|supports?|provides?|delivers?|enforces?|uses?|works?|deployed|live)\b', re.I)
CAPABILITY = re.compile(r'\b(?:moat|cache[- ]warm|benchmark(?:s|ed|ing)?|runner(?:s)?|fabric|containers?|cas|action cache|memoiz\w*|pricing|billing|tenant|isolation|spawn(?:s|ed|ing)?|execution|compute|probe)\b', re.I)
EXCLUDED = ("docs/handoff/", "docs/review/", "docs/audits/", "vendor/", "generated/")

def fail(message): errors.append(message)
def load(path, label):
    try:
        with path.open(encoding="utf-8") as f: return json.load(f)
    except (OSError, ValueError) as exc:
        fail(f"{label}: unreadable or invalid JSON ({exc})"); return {}
def object_only(value, allowed, label):
    if not isinstance(value, dict): fail(f"{label}: must be an object"); return False
    unknown = sorted(set(value) - set(allowed))
    if unknown: fail(f"{label}: unknown field(s): {', '.join(unknown)}")
    return True
def require(value, fields, label):
    if not isinstance(value, dict): fail(f"{label}: must be an object"); return False
    missing = [field for field in fields if field not in value]
    if missing: fail(f"{label}: missing required field(s): {', '.join(missing)}"); return False
    return True
def safe_path(value, label):
    if not isinstance(value, str) or not value or value.startswith("/") or "\0" in value:
        fail(f"{label}: path must be relative and non-empty"); return None
    candidate = (root / value).resolve()
    try: candidate.relative_to(root)
    except ValueError: fail(f"{label}: path escapes repository root"); return None
    return candidate
def timestamp(value, label):
    try:
        if not isinstance(value, str) or not re.search(r'(?:Z|[+-][0-9]{2}:[0-9]{2})$', value): raise ValueError
        datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError: fail(f"{label}: must be RFC3339 with timezone")
def git(*args):
    return subprocess.run(["git", "-C", str(root), *args], text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
def git_bytes(*args):
    return subprocess.run(["git", "-C", str(root), *args], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
def committed_bytes(commit, path, sha, label):
    if not isinstance(commit, str) or not re.fullmatch(r"[0-9a-f]{40}", commit): fail(f"{label}: invalid commit_sha"); return
    if not isinstance(path, str) or not path or path.startswith("/") or ".." in pathlib.PurePosixPath(path).parts: fail(f"{label}: unsafe path"); return
    if git("cat-file", "-e", f"{commit}^{{commit}}").returncode: fail(f"{label}: commit does not exist in checkout"); return
    result = git_bytes("show", f"{commit}:{path}")
    if result.returncode: fail(f"{label}: path is absent at declared commit"); return
    if not isinstance(sha, str) or not re.fullmatch(r"[0-9a-f]{64}", sha): fail(f"{label}: invalid sha256"); return
    if hashlib.sha256(result.stdout).hexdigest() != sha: fail(f"{label}: committed bytes do not match sha256"); return
    return result.stdout
def sources():
    result = git("ls-files", "-z", "--", "*.md")
    if result.returncode: fail("claim_sources: cannot enumerate tracked Markdown"); return []
    answer = []
    for value in result.stdout.split("\0"):
        if value and not value.startswith(EXCLUDED):
            path = safe_path(value, "claim_sources")
            if path and path.is_file(): answer.append(path)
    if not answer: fail("claim_sources: no eligible tracked Markdown source")
    return sorted(answer)

schema, manifest = load(schema_path, "schema"), load(manifest_path, "manifest")
if not isinstance(schema, dict) or schema.get("$id") != "https://corelink.dev/schemas/evidence-artifact-v1.json": fail("schema: unexpected schema identity")
manifest_fields = {"schema_version", "manifest_id", "coverage_status", "generated_at", "repository", "commit_sha", "artifacts", "claims", "claim_sources", "deployed_versions"}
object_only(manifest, manifest_fields, "manifest"); require(manifest, manifest_fields, "manifest")
if manifest.get("schema_version") != "evidence-manifest/v1": fail("manifest: schema_version must be evidence-manifest/v1")
if manifest.get("coverage_status") not in ("READY", "RED"): fail("manifest: coverage_status must be READY or RED")
if manifest.get("coverage_status") != "READY": fail("manifest: coverage is explicitly RED; claim inventory is not sealed")
if not isinstance(manifest.get("manifest_id"), str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{2,127}", manifest.get("manifest_id", "")): fail("manifest: invalid manifest_id")
if not isinstance(manifest.get("repository"), str) or not manifest.get("repository"): fail("manifest: repository must be non-empty")
if not re.fullmatch(r"[0-9a-f]{40}", str(manifest.get("commit_sha", ""))): fail("manifest: invalid commit_sha")
timestamp(manifest.get("generated_at"), "manifest.generated_at")
if manifest.get("claim_sources") != ["@tracked-markdown"]: fail("manifest.claim_sources: must be exactly ['@tracked-markdown']; subsets are forbidden")
markdown_sources = sources()
artifacts_raw, claims_raw = manifest.get("artifacts"), manifest.get("claims")
if not isinstance(artifacts_raw, list) or not isinstance(claims_raw, list): fail("manifest: artifacts and claims must be arrays"); artifacts_raw, claims_raw = [], []
if not artifacts_raw: fail("manifest: artifacts must be non-empty for READY coverage")
if not claims_raw: fail("manifest: claims must be non-empty for READY coverage")

deployed = manifest.get("deployed_versions")
if not isinstance(deployed, dict): fail("manifest.deployed_versions: must be an object"); deployed = {}
for name, value in deployed.items():
    label = f"manifest.deployed_versions[{name!r}]"
    object_only(value, {"id", "digest", "authority"}, label)
    if not isinstance(name, str) or not name or not isinstance(value, dict) or not (value.get("id") or value.get("digest")): fail(f"{label}: id or digest is required"); continue
    if value.get("digest") is not None and not re.fullmatch(r"sha256:[0-9a-f]{64}", str(value["digest"])): fail(f"{label}.digest: invalid digest")
    authority = value.get("authority")
    if not isinstance(authority, dict): fail(f"{label}: self-attested deployed version; committed authority is required"); continue
    object_only(authority, {"commit_sha", "path", "sha256"}, f"{label}.authority")
    authority_bytes = committed_bytes(authority.get("commit_sha"), authority.get("path"), authority.get("sha256"), f"{label}.authority")
    if authority_bytes is None: continue
    try:
        authority_record = json.loads(authority_bytes.decode("utf-8"))
    except (UnicodeDecodeError, ValueError) as exc:
        fail(f"{label}.authority: must be UTF-8 JSON ({exc})")
        continue
    authority_fields = {"schema_version", "kind", "id", "digest", "observed_at"}
    object_only(authority_record, authority_fields, f"{label}.authority")
    if not require(authority_record, {"schema_version", "kind", "observed_at"}, f"{label}.authority"): continue
    if authority_record.get("schema_version") != "deployed-version-authority/v1" or authority_record.get("kind") != "deployment-authority":
        fail(f"{label}.authority: invalid authority record kind/schema")
    timestamp(authority_record.get("observed_at"), f"{label}.authority.observed_at")
    for field in ("id", "digest"):
        if value.get(field) is not None and authority_record.get(field) != value.get(field):
            fail(f"{label}.authority: does not declare the same deployed {field}")

artifacts, payloads = {}, {}
for number, entry in enumerate(artifacts_raw):
    label = f"manifest.artifacts[{number}]"
    object_only(entry, {"path", "artifact_id", "sha256"}, label)
    if not require(entry, {"path", "artifact_id", "sha256"}, label): continue
    path, aid = safe_path(entry["path"], f"{label}.path"), entry["artifact_id"]
    if not isinstance(aid, str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{2,127}", aid): fail(f"{label}: invalid artifact_id")
    if aid in artifacts: fail(f"{label}: duplicate artifact_id {aid}")
    artifacts[aid] = entry
    if not isinstance(entry["path"], str) or not entry["path"].startswith("docs/plan/evidence/") or not entry["path"].endswith(".json"): fail(f"{label}: artifact path must be evidence JSON")
    if path is None or not path.is_file(): fail(f"{label}: artifact file is absent"); continue
    if hashlib.sha256(path.read_bytes()).hexdigest() != entry["sha256"]: fail(f"{label}: sha256 mismatch")
    artifact = load(path, str(entry["path"])); payloads[aid] = artifact
    fields = {"schema_version", "artifact_id", "kind", "status", "observed_at", "source", "claims", "version", "evidence", "notes"}
    object_only(artifact, fields, str(entry["path"]))
    if not require(artifact, {"schema_version", "artifact_id", "kind", "status", "observed_at", "source", "claims", "version"}, str(entry["path"])): continue
    if artifact.get("schema_version") != "evidence/v1" or artifact.get("artifact_id") != aid: fail(f"{entry['path']}: invalid schema_version or artifact_id")
    if artifact.get("kind") not in ("test", "probe", "test+probe", "decision", "obstacle", "relay", "capability"): fail(f"{entry['path']}: invalid kind")
    if artifact.get("status") not in ("PASS", "FAIL", "SKIPPED", "FAILED", "UNKNOWN", "SERVED", "CONTAINED", "RED"): fail(f"{entry['path']}: invalid status")
    timestamp(artifact.get("observed_at"), f"{entry['path']}.observed_at")
    source = artifact.get("source")
    if not isinstance(source, dict): fail(f"{entry['path']}.source: must be object")
    else:
        object_only(source, {"repository", "commit_sha", "path"}, f"{entry['path']}.source")
        if source.get("repository") != manifest.get("repository") or source.get("commit_sha") != manifest.get("commit_sha") or source.get("path") != entry.get("path"): fail(f"{entry['path']}: source does not bind repository, commit and path")
        if git("cat-file", "-e", f"{source.get('commit_sha','')}^{{commit}}").returncode: fail(f"{entry['path']}: source commit does not exist")
    version = artifact.get("version")
    if not isinstance(version, dict) or not (version.get("id") or version.get("digest")): fail(f"{entry['path']}: version id or digest is required")
    elif version.get("digest") is not None and not re.fullmatch(r"sha256:[0-9a-f]{64}", str(version["digest"])): fail(f"{entry['path']}: invalid version digest")
    claim_ids = artifact.get("claims")
    if not isinstance(claim_ids, list) or len(set(claim_ids)) != len(claim_ids) or not all(isinstance(v, str) and re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._:-]{1,127}", v) for v in claim_ids): fail(f"{entry['path']}: invalid claims array")

evidence_dir, listed = root / "docs/plan/evidence", {entry.get("path") for entry in artifacts_raw if isinstance(entry, dict)}
if evidence_dir.is_dir():
    for candidate in evidence_dir.rglob("*.json"):
        relative = str(candidate.relative_to(root))
        if relative not in ("docs/plan/evidence/schema-v1.json", "docs/plan/evidence/manifest-v1.json") and relative not in listed: fail(f"{relative}: evidence JSON is not indexed")

for name, value in deployed.items():
    authority = value.get("authority") if isinstance(value, dict) else None
    if isinstance(authority, dict) and authority.get("path") in listed:
        fail(f"manifest.deployed_versions[{name!r}]: authority must be independent of the artifact it anchors")

claims = {}
for number, claim in enumerate(claims_raw):
    label = f"manifest.claims[{number}]"
    object_only(claim, {"id", "file", "line", "artifact_id"}, label)
    if not require(claim, {"id", "file", "line", "artifact_id"}, label): continue
    cid = claim["id"]
    if cid in claims:
        fail(f"{label}: duplicate claim id {cid}")
        continue
    claims[cid] = claim
    source = safe_path(claim["file"], f"{label}.file")
    if not isinstance(cid, str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._:-]{1,127}", cid): fail(f"{label}: invalid claim id")
    if source is None or source not in markdown_sources: fail(f"{label}: claim source is not eligible tracked Markdown"); continue
    lines = source.read_text(encoding="utf-8").splitlines()
    if not isinstance(claim["line"], int) or not 0 < claim["line"] <= len(lines): fail(f"{label}: line is outside source"); continue
    marker = MARKER.search(lines[claim["line"] - 1])
    if marker is None or marker.groups() != (cid, claim["artifact_id"]): fail(f"{label}: source has no exact claim marker")
    artifact = payloads.get(claim["artifact_id"])
    if not isinstance(artifact, dict) or cid not in artifact.get("claims", []): fail(f"{label}: artifact does not name claim")
    elif artifact.get("status") not in ("PASS", "SERVED"): fail(f"{label}: non-positive artifact cannot support capability claim")
    else:
        version = artifact.get("version", {})
        if not any(isinstance(item, dict) and ((version.get("id") and item.get("id") == version.get("id")) or (version.get("digest") and item.get("digest") == version.get("digest"))) for item in deployed.values()): fail(f"{label}: artifact version lacks committed deployed-version anchor")

for source in markdown_sources:
    for line_no, line in enumerate(source.read_text(encoding="utf-8").splitlines(), 1):
        marker = MARKER.search(line)
        if "corelink-claim" in line and marker is None: fail(f"{source.relative_to(root)}:{line_no}: malformed claim marker")
        if marker:
            cid, aid = marker.groups(); record = claims.get(cid)
            if not record or record.get("file") != str(source.relative_to(root)) or record.get("line") != line_no or record.get("artifact_id") != aid: fail(f"{source.relative_to(root)}:{line_no}: marker is not represented in manifest")
        if CAPABILITY.search(line) and (PRESENT.search(line) or re.search(r"\b(?:moat|benchmark)\b", line, re.I)) and marker is None: fail(f"{source.relative_to(root)}:{line_no}: present-tense capability claim lacks artifact marker")

if errors:
    for error in errors[:100]: print(f"claim-artifact-lint: ERROR: {error}", file=sys.stderr)
    if len(errors) > 100: print(f"claim-artifact-lint: ERROR: {len(errors) - 100} additional error(s) suppressed", file=sys.stderr)
    print(f"claim-artifact-lint: FAIL ({len(errors)} error(s))", file=sys.stderr); raise SystemExit(1)
print(f"claim-artifact-lint: PASS ({len(artifacts)} artifact(s), {len(claims)} claim(s); offline)")
PY
