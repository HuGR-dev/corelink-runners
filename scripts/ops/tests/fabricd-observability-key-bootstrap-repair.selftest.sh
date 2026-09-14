#!/usr/bin/env bash
set -Eeuo pipefail
umask 077
here="$(cd -- "$(dirname -- "$0")" && pwd)"
harness="$here/../fabricd-observability-key-bootstrap.sh"
wrangler="$here/mock-observability-bootstrap-wrangler.sh"
curl_mock="$here/mock-observability-bootstrap-curl.sh"
ack='ACK-CORELINK-FABRIC-OBSERVABILITY-REPAIR-INTROSPECT-AUTH-LIVE-20260909'
resume_ack='ACK-CORELINK-FABRIC-OBSERVABILITY-REPAIR-INTROSPECT-AUTH-RESUME-LIVE-20260909'
digest='sha256:1111111111111111111111111111111111111111111111111111111111111111'
old='13e744e1-921e-4dfb-8423-7662a804222e'; new='a035e7f9-26d7-4537-9863-568872efc6ba'
final='a03ab526-374e-4596-a553-fe42b7344b42'; final_version='08ba9b85-1111-4111-8111-111111111111'
tmp="$(mktemp -d "${TMPDIR:-/tmp}/corelink-obs-repair-test.XXXXXXXX")"; trap 'rm -rf -- "$tmp"' EXIT; chmod 700 "$tmp"
fixture() {
  root="$tmp/root-$1"; oob="$tmp/oob-$1"; state="$tmp/state-$1"
  mkdir -p "$root/deploy/cloudflare-fabricd" "$root/evidence" "$oob"; chmod 700 "$root" "$root/evidence" "$oob"
  printf '{"image":"registry.example/corelink@%s"}\n' "$digest" > "$root/deploy/cloudflare-fabricd/wrangler.jsonc"
  git -C "$root" init -q; git -C "$root" config user.email repair@example.invalid; git -C "$root" config user.name repair; git -C "$root" add .; git -C "$root" commit -qm baseline; commit="$(git -C "$root" rev-parse HEAD)"
  for f in fleet pat; do printf '%s-key\n' "$f" > "$oob/$f"; chmod 600 "$oob/$f"; done
  printf '%s\n' observability-existing-key > "$oob/observability"; printf '%s\n' replacement-introspect-key > "$oob/new-introspect"; chmod 600 "$oob/observability" "$oob/new-introspect"
}
historical() {
  jq -n '{schema_version:"evidence/v1",artifact_id:"fabricd-observability-key-bootstrap-introspect-recovery-failure",status:"RED",operation_mode:"introspect-recovery",source:{repository:"corelink-runners",commit_sha:"37565619dae31a61f67a095daa0cd15b06386237"},provider:{worker:"corelink-fabricd",expected_version:"c38233a3-4ede-4803-8e1c-1a3b5ad4d667",app_id:"a030ba5d-9a44-409e-b5f1-a2e6cfa50ea7",expected_image_digest:"sha256:300d5fb008877d5ba9de82b5555572894b1bbae180b7567a909f777ae2d0b5f5"},phase:"container-recreate",secrets:{FABRIC_INTROSPECT_KEY_put_completed:true,FABRIC_OBSERVABILITY_KEY_put_completed:true},gates:{refreeze_result:"green"},outcome:"RED",rerun_guard:"armed",secret_values:"excluded",secret_hashes:"excluded"}' > "$oob/fabricd-observability-key-bootstrap-introspect-recovery-failure.json"
  printf 'in-progress\n' > "$oob/.fabricd-observability-key-bootstrap-introspect-recovery.in-progress"; chmod 600 "$oob/fabricd-observability-key-bootstrap-introspect-recovery-failure.json" "$oob/.fabricd-observability-key-bootstrap-introspect-recovery.in-progress"
}
progress() {
  local phase="$1" put="$2" del="$3" dep="$4" attempts="$5"
  jq -n --arg c "$commit" --arg d "$digest" --arg o "$old" --arg n "$new" --arg p "$phase" --argjson put "$put" --argjson del "$del" --argjson dep "$dep" --argjson attempts "$attempts" '{schema_version:"evidence/v1",artifact_id:"fabricd-observability-key-bootstrap-introspect-auth-repair-progress",status:"RED",operation_mode:"introspect-auth-repair",source:{repository:"corelink-runners",commit_sha:$c},provider:{worker:"corelink-fabricd",expected_version:$o,expected_image_digest:$d,app_id:$o},historical:{source_commit:"37565619dae31a61f67a095daa0cd15b06386237",version:"c38233a3-4ede-4803-8e1c-1a3b5ad4d667",app_id:"a030ba5d-9a44-409e-b5f1-a2e6cfa50ea7",image_digest:"sha256:300d5fb008877d5ba9de82b5555572894b1bbae180b7567a909f777ae2d0b5f5"},repair:{wrong_name:"FABRIC_INTROSPECT_KEY",correct_name:"FABRIC_INTROSPECT_AUTH_KEY",phase:$p,pre_repair_app_id:$o,final_app_id:(if $dep then $n else "" end)},steps:{secret_put:{intent:($put | not),completed:$put,attempts:1},delete:{intent:($del | not),completed:$del,attempts:(if $del then 1 else 0 end)},deploy:{intent:($dep | not),completed:$dep,attempts:$attempts}},secrets:{wrong_binding_name:"FABRIC_INTROSPECT_KEY",correct_binding_name:"FABRIC_INTROSPECT_AUTH_KEY",values:"excluded",hashes:"excluded"},secret_values:"excluded",secret_hashes:"excluded"}' > "$oob/fabricd-observability-key-bootstrap-introspect-auth-repair-progress.json"
  jq -n --arg c "$commit" --arg d "$digest" --arg o "$old" --arg p "$phase" '{schema_version:"evidence/v1",artifact_id:"fabricd-observability-key-bootstrap-introspect-auth-repair-failure",status:"RED",operation_mode:"introspect-auth-repair",source:{repository:"corelink-runners",commit_sha:$c},provider:{worker:"corelink-fabricd",expected_version:$o,expected_image_digest:$d,app_id:$o},phase:$p,rerun_guard:"armed",secret_values:"excluded",secret_hashes:"excluded"}' > "$oob/fabricd-observability-key-bootstrap-introspect-auth-repair-failure.json"
  chmod 600 "$oob"/*repair-*.json
}
final_args() { printf '%s\0' "${base[@]}" --repo-root "$root" --expected-commit "$commit" --expected-version "$final_version" --fabricd-app-id "$final" --expected-image-digest "$digest" --oob-dir "$oob" --fleet-key-file "$oob/fleet" --new-introspect-key-file "$oob/new-introspect" --introspect-pat-file "$oob/pat" --key-file "$oob/observability" --evidence-file "$root/evidence/result.json"; }
finalize_progress() {
  jq --arg pre "$new" --arg app "$final" --arg version "$final_version" '.provider.app_id=$pre | .repair.pre_repair_app_id=$pre | .repair.final_app_id=$app | .repair.pre_repair_version=.provider.expected_version | .repair.final_version=$version' "$oob/fabricd-observability-key-bootstrap-introspect-auth-repair-progress.json" > "$oob/progress.tmp"; mv "$oob/progress.tmp" "$oob/fabricd-observability-key-bootstrap-introspect-auth-repair-progress.json"
  jq --arg pre "$new" --arg app "$final" --arg version "$final_version" '.provider.app_id=$pre | .repair={wrong_name:"FABRIC_INTROSPECT_KEY",correct_name:"FABRIC_INTROSPECT_AUTH_KEY",pre_repair_app_id:$pre,pre_repair_version:.provider.expected_version,final_app_id:$app,final_version:$version,intents:{secret_put:false,delete:false,deploy:false},attempts:{secret_put:1,delete:1,deploy:1}}' "$oob/fabricd-observability-key-bootstrap-introspect-auth-repair-failure.json" > "$oob/failure.tmp"; mv "$oob/failure.tmp" "$oob/fabricd-observability-key-bootstrap-introspect-auth-repair-failure.json"
}
base=(--mode mock --mock-wrangler "$wrangler" --curl-bin "$curl_mock" --tenant-id tenant-test --status-url https://status.test/internal/v1/status --health-url https://status.test/health --fleet-url https://status.test/internal/v1/fleet/busy --introspect-url https://corelink-api.humangr.com/internal/v1/auth/introspect --stability-seconds 0)
initial_args() { printf '%s\0' "${base[@]}" --repo-root "$root" --expected-commit "$commit" --expected-version "$old" --expected-image-digest "$digest" --oob-dir "$oob" --fleet-key-file "$oob/fleet" --new-introspect-key-file "$oob/new-introspect" --introspect-pat-file "$oob/pat" --key-file "$oob/observability" --evidence-file "$root/evidence/result.json"; }
fixture initial-duplicate; historical; export MOCK_STATE="$state" MOCK_SCENARIO=repair-duplicate-named MOCK_CURRENT_APP_ID="$old" MOCK_NEW_APP_ID="$new" MOCK_DIGEST="$digest" MOCK_VERSION="$old" MOCK_TENANT=tenant-test
mapfile -d '' args < <(initial_args); if "$harness" "${args[@]}" --repair-introspect-auth --ack "$ack" --fabricd-app-id "$old" >/dev/null 2>&1; then exit 1; fi; test ! -e "$state.secret-put-introspect"
fixture resume-duplicate; historical; progress secret-put false false false 0; export MOCK_STATE="$state" MOCK_SCENARIO=repair-duplicate-named MOCK_CURRENT_APP_ID="$old" MOCK_NEW_APP_ID="$new" MOCK_DIGEST="$digest" MOCK_VERSION="$old" MOCK_TENANT=tenant-test
mapfile -d '' args < <(initial_args); if "$harness" "${args[@]}" --resume-introspect-auth-repair --ack "$resume_ack" --fabricd-app-id "$old" >/dev/null 2>&1; then exit 1; fi; test ! -e "$state.secret-put-introspect"
fixture partial-delete; historical; progress deploy true true false 1; export MOCK_STATE="$state" MOCK_SCENARIO=repair-zero-named MOCK_CURRENT_APP_ID="$old" MOCK_NEW_APP_ID="$new" MOCK_DIGEST="$digest" MOCK_VERSION="$old" MOCK_TENANT=tenant-test
mapfile -d '' args < <(initial_args); "$harness" "${args[@]}" --resume-introspect-auth-repair --ack "$resume_ack" --fabricd-app-id "$new" >/dev/null; test "$(cat "$state.deploy")" = 1; test ! -e "$state.secret-put-introspect"; jq -e '.resumed == true and .repair.attempts.deploy == 2' "$root/evidence/result.json" >/dev/null

# Legacy final-proof progress has only the pre-repair provider version. The
# active final deployment is a different version/app, so proof resume must not
# replay any secret put, delete, deploy, or refreeze command.
fixture final-proof-legacy; historical; progress final-proof true true true 1; finalize_progress
export MOCK_STATE="$state" MOCK_SCENARIO=repair-final-proof MOCK_CURRENT_APP_ID="$final" MOCK_NEW_APP_ID="$final" MOCK_DIGEST="$digest" MOCK_VERSION="$final_version" MOCK_TENANT=tenant-test
mapfile -d '' args < <(final_args); "$harness" "${args[@]}" --resume-introspect-auth-repair --ack "$resume_ack" >/dev/null
test ! -e "$state.secret-put-introspect"; test ! -e "$state.deleted"; test ! -e "$state.deploy"; jq -e '.resumed == true and .repair.pre_repair_app_id == "a035e7f9-26d7-4537-9863-568872efc6ba" and .provider.expected_version == "08ba9b85-1111-4111-8111-111111111111"' "$root/evidence/result.json" >/dev/null

# A drifted active final version closes the proof before any mutation path.
fixture final-proof-drift; historical; progress final-proof true true true 1; finalize_progress
export MOCK_STATE="$state" MOCK_SCENARIO=final-version-drift MOCK_CURRENT_APP_ID="$final" MOCK_NEW_APP_ID="$final" MOCK_DIGEST="$digest" MOCK_VERSION="$final_version" MOCK_TENANT=tenant-test
mapfile -d '' args < <(final_args); if "$harness" "${args[@]}" --resume-introspect-auth-repair --ack "$resume_ack" >/dev/null 2>&1; then exit 1; fi; test ! -e "$state.secret-put-introspect"; test ! -e "$state.deleted"; test ! -e "$state.deploy"

# Wrong final lineage is refused even when the provider is otherwise healthy.
fixture final-proof-wrong-lineage; historical; progress final-proof true true true 1; finalize_progress; jq --arg app "$new" '.repair.final_app_id=$app' "$oob/fabricd-observability-key-bootstrap-introspect-auth-repair-progress.json" > "$oob/progress.tmp"; mv "$oob/progress.tmp" "$oob/fabricd-observability-key-bootstrap-introspect-auth-repair-progress.json"
export MOCK_STATE="$state" MOCK_SCENARIO=repair-final-proof MOCK_CURRENT_APP_ID="$final" MOCK_NEW_APP_ID="$final" MOCK_DIGEST="$digest" MOCK_VERSION="$final_version" MOCK_TENANT=tenant-test
mapfile -d '' args < <(final_args); if "$harness" "${args[@]}" --resume-introspect-auth-repair --ack "$resume_ack" >/dev/null 2>&1; then exit 1; fi

# Direct proof remains fail-closed without Access service-token headers.
fixture final-proof-no-access; historical; progress final-proof true true true 1; finalize_progress
export MOCK_STATE="$state" MOCK_SCENARIO=repair-final-proof-no-access MOCK_CURRENT_APP_ID="$final" MOCK_NEW_APP_ID="$final" MOCK_DIGEST="$digest" MOCK_VERSION="$final_version" MOCK_TENANT=tenant-test
mapfile -d '' args < <(final_args); if "$harness" "${args[@]}" --resume-introspect-auth-repair --ack "$resume_ack" >/dev/null 2>&1; then exit 1; fi

# Supplying only one Access credential file is refused before provider access.
printf '%s\n' access-id > "$oob/access-id"; chmod 600 "$oob/access-id"
if "$harness" "${args[@]}" --access-client-id-file "$oob/access-id" >/dev/null 2>&1; then exit 1; fi

# A complete owner-only pair is sent as CF-Access headers to the canonical URL.
printf '%s\n' access-secret > "$oob/access-secret"; chmod 600 "$oob/access-secret"
export MOCK_STATE="$state" MOCK_SCENARIO=repair-final-proof MOCK_CURRENT_APP_ID="$final" MOCK_NEW_APP_ID="$final" MOCK_DIGEST="$digest" MOCK_VERSION="$final_version" MOCK_TENANT=tenant-test
mapfile -d '' args < <(final_args); "$harness" "${args[@]}" --resume-introspect-auth-repair --ack "$resume_ack" --access-client-id-file "$oob/access-id" --access-client-secret-file "$oob/access-secret" >/dev/null
test -e "$state.access-client-id-header"; test -e "$state.access-client-secret-header"

# A caller-supplied direct URL cannot move proof off the canonical endpoint.
fixture canonical-url; historical; progress final-proof true true true 1; finalize_progress
export MOCK_STATE="$state" MOCK_SCENARIO=repair-final-proof MOCK_CURRENT_APP_ID="$final" MOCK_NEW_APP_ID="$final" MOCK_DIGEST="$digest" MOCK_VERSION="$final_version" MOCK_TENANT=tenant-test
mapfile -d '' args < <(final_args); if "$harness" "${args[@]}" --resume-introspect-auth-repair --ack "$resume_ack" --introspect-url https://api.test/internal/v1/auth/introspect >/dev/null 2>&1; then exit 1; fi
printf '%s\n' 'introspect-auth-repair-focused=pass'
