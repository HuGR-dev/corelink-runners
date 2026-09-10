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
  jq -n --arg c "$commit" --arg d "$digest" --arg o "$old" '{schema_version:"evidence/v1",artifact_id:"fabricd-observability-key-bootstrap-introspect-auth-repair-failure",status:"RED",operation_mode:"introspect-auth-repair",source:{repository:"corelink-runners",commit_sha:$c},provider:{worker:"corelink-fabricd",expected_version:$o,expected_image_digest:$d,app_id:$o},rerun_guard:"armed",secret_values:"excluded",secret_hashes:"excluded"}' > "$oob/fabricd-observability-key-bootstrap-introspect-auth-repair-failure.json"
  chmod 600 "$oob"/*repair-*.json
}
base=(--mode mock --mock-wrangler "$wrangler" --curl-bin "$curl_mock" --tenant-id tenant-test --status-url https://status.test/internal/v1/status --health-url https://status.test/health --fleet-url https://status.test/internal/v1/fleet/busy --introspect-url https://api.test/internal/v1/auth/introspect --stability-seconds 0)
initial_args() { printf '%s\0' "${base[@]}" --repo-root "$root" --expected-commit "$commit" --expected-version "$old" --expected-image-digest "$digest" --oob-dir "$oob" --fleet-key-file "$oob/fleet" --new-introspect-key-file "$oob/new-introspect" --introspect-pat-file "$oob/pat" --key-file "$oob/observability" --evidence-file "$root/evidence/result.json"; }
fixture initial-duplicate; historical; export MOCK_STATE="$state" MOCK_SCENARIO=repair-duplicate-named MOCK_CURRENT_APP_ID="$old" MOCK_NEW_APP_ID="$new" MOCK_DIGEST="$digest" MOCK_VERSION="$old" MOCK_TENANT=tenant-test
mapfile -d '' args < <(initial_args); if "$harness" "${args[@]}" --repair-introspect-auth --ack "$ack" --fabricd-app-id "$old" >/dev/null 2>&1; then exit 1; fi; test ! -e "$state.secret-put-introspect"
fixture resume-duplicate; historical; progress secret-put false false false 0; export MOCK_STATE="$state" MOCK_SCENARIO=repair-duplicate-named MOCK_CURRENT_APP_ID="$old" MOCK_NEW_APP_ID="$new" MOCK_DIGEST="$digest" MOCK_VERSION="$old" MOCK_TENANT=tenant-test
mapfile -d '' args < <(initial_args); if "$harness" "${args[@]}" --resume-introspect-auth-repair --ack "$resume_ack" --fabricd-app-id "$old" >/dev/null 2>&1; then exit 1; fi; test ! -e "$state.secret-put-introspect"
fixture partial-delete; historical; progress deploy true true false 1; export MOCK_STATE="$state" MOCK_SCENARIO=repair-zero-named MOCK_CURRENT_APP_ID="$old" MOCK_NEW_APP_ID="$new" MOCK_DIGEST="$digest" MOCK_VERSION="$old" MOCK_TENANT=tenant-test
mapfile -d '' args < <(initial_args); "$harness" "${args[@]}" --resume-introspect-auth-repair --ack "$resume_ack" --fabricd-app-id "$new" >/dev/null; test "$(cat "$state.deploy")" = 1; test ! -e "$state.secret-put-introspect"; jq -e '.resumed == true and .repair.attempts.deploy == 2' "$root/evidence/result.json" >/dev/null
printf '%s\n' 'introspect-auth-repair-focused=pass'
