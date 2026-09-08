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
# mutation must make the structural checker fail; a green mutation means the
# guard is vacuous.
checker="$script_dir/verify_build_only_workflow.py"
python3 "$checker" "$workflow" "$validator"
mutated="$fixture/mutated-workflow.yml"
sed '/persist-credentials: false/d' "$workflow" > "$mutated"
if python3 "$checker" "$mutated" "$validator"; then
  echo 'workflow mutation (credential persistence) was not detected' >&2
  exit 1
fi
sed 's/pull_request:/pull_request_target:/' "$workflow" > "$mutated"
if python3 "$checker" "$mutated" "$validator"; then
  echo 'workflow mutation (trusted trigger) was not detected' >&2
  exit 1
fi
sed "/scripts\/ci\/runner-image-static-check\.sh'/d" "$workflow" > "$mutated"
if python3 "$checker" "$mutated" "$validator"; then
  echo 'workflow mutation (static-check trigger path) was not detected' >&2
  exit 1
fi
sed "/scripts\/ci\/runner-image-static-check\.selftest\.sh'/d" "$workflow" > "$mutated"
if python3 "$checker" "$mutated" "$validator"; then
  echo 'workflow mutation (static selftest trigger path) was not detected' >&2
  exit 1
fi

for trigger in \
  '.github/workflows/build-cf-container-images.yml' \
  '.github/workflows/image-build-impact.yml' \
  'scripts/ci/image-build-impact.sh' \
  'scripts/ci/image-build-impact.selftest.sh' \
  'scripts/ci/runner-image-build-validation.sh' \
  'scripts/ci/runner-image-build-validation.selftest.sh' \
  'scripts/ci/runner-image-static-check.sh' \
  'scripts/ci/runner-image-static-check.selftest.sh' \
  'scripts/ci/verify_build_only_workflow.py'; do
  python3 - "$workflow" "$mutated" "$trigger" <<'PY'
from pathlib import Path
import sys

source, destination, trigger = sys.argv[1:]
needle = f"      - '{trigger}'"
text = Path(source).read_text(encoding='utf-8')
if needle not in text:
    raise SystemExit(f'trigger mutation anchor disappeared: {trigger}')
Path(destination).write_text(text.replace(needle + "\n", '', 1), encoding='utf-8')
PY
  if python3 "$checker" "$mutated" "$validator"; then
    echo "workflow mutation (missing trigger: $trigger) was not detected" >&2
    exit 1
  fi
done

mutate_command() {
  local replacement=$1
  python3 - "$workflow" "$mutated" "$replacement" <<'PY'
from pathlib import Path
import sys

source, destination, replacement = sys.argv[1:]
text = Path(source).read_text(encoding="utf-8")
needle = "bash scripts/ci/image-build-impact.sh"
if needle not in text:
    raise SystemExit("mutation anchor disappeared")
Path(destination).write_text(text.replace(needle, replacement, 1), encoding="utf-8")
PY
}

expect_rejected() {
  local label=$1
  shift
  mutate_command "$1"
  if python3 "$checker" "$mutated" "$validator"; then
    echo "workflow mutation (${label}) was not detected" >&2
    exit 1
  fi
}

expect_rejected 'docker buildx --push' 'docker buildx build --push .'
expect_rejected 'docker image push' 'docker image push registry.example/image:tag'
expect_rejected 'buildx imagetools publisher' 'docker buildx imagetools create src dst'
expect_rejected 'docker registry output' 'docker build --output type=registry .'
expect_rejected 'build output push=true' 'docker buildx build --output type=image,push=true .'
expect_rejected 'nerdctl publisher' 'nerdctl push registry.example/image:tag'
expect_rejected 'buildctl publisher' 'buildctl build --output type=registry'
expect_rejected 'crane publisher' 'crane push image.tar registry.example/image:tag'
expect_rejected 'skopeo publisher' 'skopeo copy image.tar docker://registry.example/image:tag'
expect_rejected 'oras publisher' 'oras push registry.example/image:tag image.tar'
expect_rejected 'podman publisher' 'podman push registry.example/image:tag'
expect_rejected 'buildah publisher' 'buildah push image registry.example/image:tag'
expect_rejected 'regctl publisher' 'regctl image copy src dst'
expect_rejected 'wrangler publisher' 'wrangler containers push image:tag'
expect_rejected 'package publisher' 'npm publish image.tgz'
expect_rejected 'HTTP registry publisher' 'curl -X PUT registry.example/v2/image'
expect_rejected 'workflow dispatch' 'gh workflow run build-cf-container-images.yml'

mutate_step() {
  local replacement=$1
  python3 - "$workflow" "$mutated" "$replacement" <<'PY'
from pathlib import Path
import sys

source, destination, replacement = sys.argv[1:]
text = Path(source).read_text(encoding='utf-8')
needle = '      - name: Verify image metadata and bounded publication path\n'
if needle not in text:
    raise SystemExit('step mutation anchor disappeared')
Path(destination).write_text(text.replace(needle, replacement + '\n' + needle, 1), encoding='utf-8')
PY
}

expect_step_rejected() {
  local label=$1
  shift
  mutate_step "$1"
  if python3 "$checker" "$mutated" "$validator"; then
    echo "workflow mutation (${label}) was not detected" >&2
    exit 1
  fi
}

expect_step_rejected 'build-push action publication' '      - name: Forbidden publisher
        uses: docker/build-push-action@v6
        with:
          context: .
          push: true'
expect_step_rejected 'docker login action' '      - name: Forbidden login
        uses: docker/login-action@v3
        with:
          registry: registry.example
          username: attacker
          password: ignored'
expect_step_rejected 'github token interpolation' '      - name: Forbidden token
        uses: actions/checkout@v6
        with:
          token: ${{ github.token }}'
expect_step_rejected 'runtime token interpolation' '      - name: Forbidden runtime token
        uses: actions/checkout@v6
        with:
          token: ${{ env.ACTIONS_RUNTIME_TOKEN }}'
expect_step_rejected 'shell token interpolation' '      - name: Forbidden shell token
        uses: actions/checkout@v6
        with:
          token: $GITHUB_TOKEN'
echo 'PASS workflow trust and publisher mutations are rejected'
