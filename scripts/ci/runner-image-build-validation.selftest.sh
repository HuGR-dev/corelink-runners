#!/usr/bin/env bash
# Offline tests for the PR runner-image build validator.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
validator="$script_dir/runner-image-build-validation.sh"
workflow="$script_dir/../../.github/workflows/image-build-impact.yml"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/runner-image-validation.XXXXXX")"
trap 'rm -rf -- "$fixture"' EXIT

git -C "$fixture" init -q
git -C "$fixture" config user.email selftest@example.invalid
git -C "$fixture" config user.name runner-image-validation-selftest
mkdir -p "$fixture/deploy/runner" "$fixture/bin"
printf '%s\n' 'FROM scratch' > "$fixture/deploy/runner/Dockerfile"
printf '%s\n' 'base' > "$fixture/README.md"
git -C "$fixture" add .
git -C "$fixture" commit -qm base
base="$(git -C "$fixture" rev-parse HEAD)"

printf '%s\n' 'changed' >> "$fixture/deploy/runner/Dockerfile"
git -C "$fixture" add .
git -C "$fixture" commit -qm runner-change
head="$(git -C "$fixture" rev-parse HEAD)"

cat > "$fixture/bin/docker" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "${DOCKER_STUB_LOG:?}"
case "${1:-}" in
  build) exit 0 ;;
  image) exit 0 ;;
  *) echo "unexpected docker command: ${1:-}" >&2; exit 1 ;;
esac
EOF
chmod +x "$fixture/bin/docker"
DOCKER_STUB_LOG="$fixture/docker.log" PATH="$fixture/bin:$PATH" \
  GITHUB_RUN_ID=123 GITHUB_RUN_ATTEMPT=2 \
  "$validator" --repo "$fixture" --base "$base" --head "$head" > "$fixture/build.out"
grep -q 'RUNNER_IMAGE_BUILD_PASSED tag=corelink-runner-pr-123-2' "$fixture/build.out"
grep -q '^build --pull=false --tag corelink-runner-pr-123-2 ' "$fixture/docker.log"
grep -q '^image rm --force corelink-runner-pr-123-2$' "$fixture/docker.log"
if grep -qE 'push|login|wrangler' "$fixture/docker.log"; then
  echo 'validator selftest observed a publication command' >&2
  exit 1
fi
echo 'PASS validator builds changed runner context without publication'

printf '%s\n' 'docs-only' >> "$fixture/README.md"
git -C "$fixture" add .
git -C "$fixture" commit -qm docs-only
docs_head="$(git -C "$fixture" rev-parse HEAD)"
DOCKER_STUB_LOG="$fixture/no-build.log" PATH="$fixture/bin:$PATH" \
  "$validator" --repo "$fixture" --base "$head" --head "$docs_head" > "$fixture/no-build.out"
grep -q 'RUNNER_IMAGE_BUILD_NOT_REQUIRED' "$fixture/no-build.out"
[[ ! -e "$fixture/no-build.log" ]]
echo 'PASS validator skips unrelated changes'

# Mutation tests for the workflow's trust and publication contract. Each
# mutation must make this small contract checker fail; a green mutation means
# the guard is vacuous.
check_contract() {
  local candidate=$1
  grep -qE '^  pull_request:[[:space:]]*$' "$candidate" || return 1
  grep -qE '^  contents:[[:space:]]+read[[:space:]]*$' "$candidate" || return 1
  grep -q 'persist-credentials: false' "$candidate" || return 1
  grep -q 'github.event.pull_request.number' "$candidate" || return 1
  grep -q 'runner-image-build-validation.sh' "$candidate" || return 1
  ! grep -qE 'pull_request_target|secrets\.|docker push|containers push|wrangler deploy' "$candidate"
}
check_contract "$workflow"
mutated="$fixture/mutated-workflow.yml"
sed '/persist-credentials: false/d' "$workflow" > "$mutated"
if check_contract "$mutated"; then
  echo 'workflow mutation (credential persistence) was not detected' >&2
  exit 1
fi
sed 's/pull_request:/pull_request_target:/' "$workflow" > "$mutated"
if check_contract "$mutated"; then
  echo 'workflow mutation (trusted trigger) was not detected' >&2
  exit 1
fi
awk '{ print } END { print "docker push attacker/image:latest" }' "$workflow" > "$mutated"
if check_contract "$mutated"; then
  echo 'workflow mutation (publication) was not detected' >&2
  exit 1
fi
echo 'PASS workflow trust/publication mutations are rejected'
