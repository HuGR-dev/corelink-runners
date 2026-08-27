# RELATÓRIO DEFINITIVO — AUDITORIA COMPLETA corelink-runners

**Data:** 2026-08-25 · **Repo:** `/Users/gustavoschneiter/Documents/HuGR/corelink-runners` @ `e228249` (working tree limpo)
**Método:** 10 agents paralelos (authz/spawn · billing · scheduler · webhook-reliability · perf · infra/secrets · sandbox · contracts · data-model · higiene+economia) sobre 112k LOC.
**Exclusões:** bugs já corrigidos e documentados no próprio CHANGELOG (#432 free-tier seeding, #433 403→503 attribution) — auditados os vizinhos, não re-reportados.
**Padrão:** toda flag com file:line + quote. Dúvida → flag. Refutações/nada-encontrado declarados explicitamente.

---

# PARTE 0 — SUMÁRIO EXECUTIVO

## Veredito em uma frase (do agent de authz)

> **"Abuse-resistant by authorization logic, fragile by configuration posture."**

A lógica de autorização é correta. Mas a propriedade inteira descansa sobre UMA checagem runtime + DUAS defesas opcionais desarmadas (`INSTALLATION_ALLOWLIST` unset, `PINNED_IMAGE_DIGEST` unset). E o dinheiro tem dois buracos medidos, um deles sistemático.

## Os achados que doem mais

1. **Jobs >2h faturam ZERO** (RC1). Identidade de tenant para billing de completion mora em KV com TTL de 2h (`JOB_PAT_TTL_S=7200`); job longo expira o próprio registro antes do `completed`. Até 4h × 4 vCPU × $0.30 ≈ **$14,40/job não-faturado, recorrente, invisível** — e escala exatamente com os melhores clientes (os que rodam jobs longos).
2. **Orphan boxes: vistos, não mortos** (RC2). Teardown por instance-name resolve `idFromName()` para um DO errado, destrói nada, **retorna 204 sucesso**. Medido: 3 boxes × 10.2h ≈ 122 vCPU-h ≈ $12/incidente, recorrente até existir kill-path (ADR-0010).
3. **Um job de cliente pagante pode ser PERDIDO para sempre HOJE** (RC3). Webhook acka 202; `driveSpawnGuarded` roda em `waitUntil` (~30s de teto); spawn lento estoura → isolate morto após claim, antes de qualquer dead-letter → sem redelivery (GitHub entrega `queued` 1×) → job fica queued até o GitHub cancelar ~24h depois. Zero sinal pro cliente.
4. **Root instantâneo dentro da box** (RH4): `runner ALL=(ALL) NOPASSWD:ALL` no Dockerfile — job não-confiável = root; root pode ptrace o Listener (segredos), ler `/proc/*/environ` (jitconfig, cred ticket), tamperar o `clw` → envenenar cache AC do próprio tenant → persistência cross-run via cache.
5. **Egress irrestrito de IP compartilhado** (RH5): scanning/DDoS de um tenant malicioso sai pelo IP de saída compartilhado → RBL/abuse report degrada egress de TODOS os tenants. IMDS nunca verificado ao vivo ("probe owed").

## Números-âncora

| Métrica | Valor |
|---|---|
| Pior leak de receita por job | ~$14,40 (>2h) |
| Pior incidente orphan medido | ~$12–13 (~120 vCPU-h) |
| Overcommit por race de claim | ~2× COGS por job corrido |
| Spawn tax (queued→RUNNING, nó quente) | ~15–30s |
| Margem slot-h (incluso / overage) | ~40% floor / 67% |
| Custo fixo mensal freeload | ~$45 proxy |
| Fleet cap | 250 (CF account ~343 práticos) |

---

# PARTE I — CATÁLOGO: CRÍTICOS

## RC1 — Jobs >2h faturam ZERO ✅🆕

`index.ts:816,1230-1231,2519-2525,2861-2863` · `const JOB_PAT_TTL_S = 7200`

Identidade de tenant p/ completion-billing vive em `jtenant:<jobId>` KV com `expirationTtl = JOB_PAT_TTL_S` (2h). Não existe cap interno de duração de job: o keepalive sweep mantém boxes BUSY vivas além de todos os TTLs internos (boxes medidas com 10.2h), e o próprio código se sabe errado — `index.ts:856`: *"`JOB_PAT_TTL_S`, 2 h … That is correct for its purpose and WRONG as the fleet's [tenant record]"* e `index.ts:814-815` assume "well past the longest CI job" (drift). Todo job warm cujo `completed` chega >2h após spawn perde o `jtenant`. Consequência em cadeia: `maybeBillCompletedJob` skipa (sem `billedTenant`) → `recordCompletedJobUsage` skipa (`if (!derivedTenant) return false`) → ledger WP-F nunca recebe row → reconciler não recupera (fonte GitHub não carrega installation_id → sem tenant). `CLW_TENANT` fallback é dogfood-only (:1533).

**Box-hours reais, zero `runner_vcpu_seconds`, irrecuperável — sistemático, não flake.**
Fix: escrever `jtenant:` com TTL 60d (espelhar `USAGE_LEDGER_TTL_S`), ou stampar tenant no ledger `usage:` no START, ou derivar do pat-row durável keyed por jobId. **Uma linha guardando o eixo inteiro de receita metered.**

## RC2 — Orphan boxes: detecção sem execução ✅🆕

`scripts/orphan-box-check.sh:20-27` · ADR-0010 PROPOSED-only · medido 2026-08-23

Boxes sem `sbox:` são invisíveis ao `reapStaleBoxes`. O detector novo é detection-only: `POST /v1/teardown` por instance-name → `idFromName()` resolve DO fresco e alheio → destrói nada → **204 sucesso**. Namespace disjunto (`cf-runner-<8hex>` vs UUID nu). Allowlist de classes = nova classe de container entra silenciosamente sem cobertura.
**Fix estrutural:** landar ADR-0010 (DO names generativos/enumeráveis) + gate de registro de classe no detector. Enquanto isso: todo incidente de record-loss é unrecoverable-by-construction; o leak de ~120 vCPU-h foi drenado À MÃO.

## RC3 — Job de pagante perdido permanentemente (waitUntil window) 🆕

`index.ts:3067 (+896-900, 1093-1133)` · loss-matrix completa do agent de reliability

Caminho legal mais lento: mint ~1–2s + JIT mint + 3×8s start-timeout + backoff ≈ **25–28s contra o lifetime do `waitUntil` da plataforma** (o mecanismo é reconhecido pelo próprio repo: `index.ts:3397-3399` — "the background driveSpawnGuarded (waitUntil) is killed by the platform before its catch releases the claim"; a figura exata de ~30s é conhecimento de plataforma, não citada no repo). Isolate morto ⇒ sem dead-letter escrita (ela vive SÓ no catch do guard, `lib.ts:1114`), sem `placedMs`. Nota pós-verificação: o stale-claim blocking já foi derrotado pro scan first-party (#293 — `releaseSpawnClaim` antes do re-claim, `index.ts:3414`) — a perda residual é para repos EXTERNOS (fora de `RECONCILER_REPOS`, que não têm scan nenhum). GitHub não redeliveria mesmo se pudesse. Cliente vê "queued" morrer ~24h depois, zero sinal nosso.
**Fix pequeno e conhecido:** write-ahead dead-letter (`orphan:<jobId>`, pending-state) ANTES do spawn começar; clear on confirmed placement. Alternativa: drive via DO alarm/Queue consumer (lifetime não preso à response).

---

# PARTE II — CATÁLOGO: HIGH

## RH1 — Free compute a uma misconfig de distância

`wrangler.jsonc` (INSTALLATION_ALLOWLIST ausente) + `index.ts:3036-3045` + `lib.ts:482-487`

Gate de identidade pré-mint deliberadamente DESARMADO em prod (`wrangler.jsonc:77`: *"Leaving INSTALLATION_ALLOWLIST out keeps the gate DISARMED"*). Único hard-deny para instalação-alheia = mint 403 server-side. Se `CORELINK_RUNNER_MINT_AUTH_KEY` ausente/misconfigured: `buildContainerEnv` short-circuita para `authz:"ok"` **COLD** (`lib.ts:482-487`; o próprio comentário chama de "legacy fail-open", :478) → box sobe e roda código de atacante grátis, nada mais recusa. Comment-drift relacionado ⚠️(corrigido na verificação: o comentário vive em `index.ts:2996-2999`, não lib.ts): diz sentar-se DEPOIS do allowlist ("Placed AFTER… the allowlist gate") — ordem real é limiter (:3014) ANTES allowlist (:3036); flood gasta budget + mint load igualmente (sem bypass authz — allowlist ainda recusa antes de claim/slot).
**Fix:** armar allowlist agora (valor documentado pronto) + deploy-time assertion: worker recusa servir `/webhook` sem mint-key E allowlist armados.

## RH2 — Bearer único = compute arbitrário + argv arbitrário

`index.ts:675-680, 3128-3225, 3232-3283`

`CLOUDFLARE_SPAWN_AUTH_TOKEN` estático autoriza TUDO: `/v1/spawn` (containers + env arbitrários), `/v1/exec` (**argv arbitrário** em check-hosts), teardown, status, egress-cutoff. Sem rotação, sem scoping. `PINNED_IMAGE_DIGEST` guard INERT (opcional, unset, :154 admite). Leak = compute imediato na conta CF do operator.
**Fix:** tokens por domínio ou mTLS/CF Access service-auth fabricd↔worker (padrão Inc-3 já existe rumo ao corelink-server); setar PINNED_IMAGE_DIGEST.

## RH3 — DO acquire fail-OPEN bypassa o sistema de caps inteiro num erro de infra

`index.ts:1608-1617`: `catch → { admitted: true }` — "never block a legitimate job on a DO error". Todo spawn funila por UM singleton DO. Janela de throw (overload — que é exatamente quando bursts batem; eviction; erro) = zero accounting: fleet cap E entitlements pagos bypassados. Bound físico só `max_instances:250`.
**Fix:** split por warmth — warm/paid ⇒ fail-CLOSED (refuse + dead-letter; recovery existe), cold ⇒ fail-open ok. Metric + alert de DO-error.

## RH4 — `NOPASSWD:ALL` = root na box

`deploy/runner/Dockerfile:514-516`. Job não-confiável = root imediato. Root → ptrace `Runner.Listener` (segredos do job), `/proc/*/environ` (jitconfig, `CLW_CRED_TICKET`), tamper `/usr/local/bin/clw` → snapshot envenenado gravado no CAS do próprio tenant → artefatos venenosos restauram nos builds futuros LIMPOS daquele tenant. Cross-run persistence through the cache — o moat virado pra dentro.
**Fix:** remover sudo (bake deps no image); se apt necessário, variante builder privilegiada separada.

## RH5 — Egress irrestrito de IP compartilhado + IMDS nunca verificado

`index.ts:553-563,:18-46`: `enableInternet=true`; `deniedHosts` REMOVIDO (quebrou egress GH); metadata denylist = glob de host exato; raw sockets bypassam proxy SDK (comentário próprio admite G2 aberto). Zero egress allowlist. Job malicioso escaneia/spamma do IP de saída COMPARTILHADO → reputação danificada pra todos. ⚠️**IMDS sub-claim CORRIGIDO na verificação**: o texto "live-account smoke is still owed" (`index.ts:45`) é DOC STALE — a probe REAL rodou e fechou GO (`docs/handoff/…probe-NOT-reachable-GO-closed.md`, run `o7-metadata-probe.yml` #28719027473: "NOTHING REACHABLE"); ironia: `index.ts:561` cita "G2 is settled by the metadata probe" três linhas abaixo do "still owed" — contradição interna no mesmo arquivo. O achado real virou: docs contraditórios sobre postura de metadata, e o fechamento G2 depende de evidência pontual, não de controle contínuo. Egress allowlist na camada de rede segue sendo o close real (o código mesmo diz, :42).
**Fix:** egress policy na camada de rede da plataforma (allowlist github.com/api.github.com/corelink-api) + teste vivo do IMDS.

## RH6 — Billing batch ilimitado vs cap 1024 server-side = poison-pill retry flood

`crates/corelink-fabric-server/src/corelink_billing.rs:279-299` (⚠️ path corrigido na verificação — não é fabric/) ↔ server `billing_ingest.rs:121,501-503`

`flush_now` posta o buffer INTEIRO num POST. Server: `MAX_BATCH_RECORDS=1024`, over-limit → 400 `batch_too_large`. Buffer cap 100k ⇒ após outage ≥1024 backlog, batch NUNCA mais succeede → retido pra sempre → shed do mais-velho aos 100k. **Bug vivo hoje.** Precedente próprio: a classe já disparou 1× com o flood `bad_region` (`corelink_billing.rs:802`, fixado na Inc-3 — o poison de TAMANHO continua). Par: doc promete `{accepted,deduped,rejected,total}` partial-accept; server real é `{accepted,deduped,total}` all-or-nothing (`billing_ingest.rs:75-79`: *"the WHOLE batch is rejected before any persist"* + test pin) — um record malformado (tenant não-UUID: fabric aceita `"acme"`, server exige UUID canônico) = loop 400 permanente.
**Fix:** chunk flush ≤512/POST (idem_key torna idempotente) + validação client-side pré-enqueue + fix doc.

## RH7 — Sentinel collision: ceiling corrupto = UNLIMITED (a classe #432 reproduzida consumer-side)

`corelink_plans.rs:131-155` (`parse_max_vcpu_h_ceiling_ms`)

`max_vcpu_h` string/negativo/∞/**overflow i64** → tratado como ABSENT → sentinel 0 → ledger SKIPPA o check = sem ceiling metered (`ledger.rs:821-826`). `0` significa simultaneamente "deliberadamente unmetered" E "read failed/corrupt" — estado "existe mas ilegível" INEXPRESSÍVEL; doc do próprio consumer admite fail-open (`corelink_plans.rs:117-119`: "a missing or malformed value yields the disabled sentinel… NEVER a 503"). Pior: ceilings fracionários (<1h, tipo trial 0.5 vCPU-h) → `f as u64` floors to 0 → DISABLED → unlimited. Trial tier = compute grátis pra sempre. ⚠️Reframe pós-verificação: a "divergência fail-closed vs fail-open cross-repo" estava OVERSTATED — server-side ABSENT = *wall-off* (deixa job passar sem cap), que é funcionalmente o MESMO outcome do sentinel disabled; ambos os extremos terminam em "sem ceiling". Server estruturalmente não pode emitir garbage (coluna typed). O achado real e fechado: **consumer converte garbage/fração em silêncio-unlimited**, e nenhum dos lados tem estado representável para "ceiling existe mas ilegível".
**Fix:** resultado 3-estado (present/absent/malformed); malformed → log+metric LOUD, nunca silent-disable; clamp overflow; suporte fracionário (domínio ms já existe); vector de garbage no conformance JSON.

## RH8 — Rotation ainda é operator-prayer

Mint key vive em 3 superfícies (spawn-worker secret, fabricd worker→container, verifier no server). Fabricd container lê env SÓ NO BOOT (`secret-inventory.md:77-81`) — `wrangler secret put` sozinho não faz NADA até rollout forçado por digest novo. Mecanismo exato do incidente J8 stale-key (2026-07-21). Boot guard valida PRESENCE do trio, não correção; self-check de boot cobre introspect key apenas — **nenhum boot-probe valida a mint credential**. Wrong mint key = boots healthy, fail-open cold spawns silencioso.
**Fix:** mint-key boot self-check (dry-probe `/internal/v1/runner/mint`, espelhar classify_introspect_bootcheck) → drift vira loud-at-boot. Inventory drift também: `secret-inventory.md` sem `COLD_ORGANIC_TENANT_PAT`, `CORELINK_CF_ACCESS_CLIENT_ID/SECRET`, `FABRIC_TEST_MINT_KEY/TENANTS`, `PINNED_IMAGE_DIGEST`.

## RH9 — 429 pode desabilitar o webhook FLEET-WIDE, silenciosamente

Limiter devolve **429** (`index.ts:3014-3023`) e HMAC-fail devolve **401 sem counter nenhum** (:2797-2800 — zero metric bump). ⚠️Premise afiada na verificação: a regra "GitHub desabilita webhooks após 20 deliveries não-2xx consecutivas" vale para REPO hooks; para GitHub APP webhooks o comportamento não está verificado, e o repo é SILENTE sobre ambos (grep: zero menção a auto-disable; pior — `index.ts:3002-3003` assume quase o oposto: *"GitHub sends workflow_job.queued exactly ONCE and never redelivers a non-2xx"*). Risco real independente da premissa: streaks de non-2xx não são monitoradas nem contadas (401-path invisível), e nenhuma alerting wiring existe no worker (counters MetricsDO pull-based, nada pagina — sem pager/betterstack/email).
**Fix:** 202 no limiter-refused (dead-letter já grava) + metric `webhook_auth_failed` + paging externo em streaks (`webhook_rate_limited`, `orphan_retry_giveup`, `rate_limit_deadletter_capped`).

## RH10 — claimSpawn KV não-atômico: janela cross-colo real, interleaving documentado

`lib.ts:113-120` get→put sem CAS; comentário alega residual "EXACTLY-concurrent" — falso: colos geodistribuídos + KV eventual-consistency = duas entregas em colos diferentes dentro da janela de propagação AMBAS passam. Interleaving completo mapeado pelo agent: dupla mint (P2 sobrescreve patId de P1 → P1 vazia até TTL), DO re-admit idempotente (=1 slot contábil), 2 containers bootam, GitHub assigna um, o outro dorme idle 15min (COGS puro) OU pega o próximo job → 2 boxes/job composto no burst.
**Fix:** mover claim para `ConcurrencySlotsDO.acquire` (set-add atômico; DO já serializa; 1 round-trip). KV claim vira hint.

---

# PARTE III — MEDIUM

| ID | Achado | Onde |
|---|---|---|
| M1 | Offboarding TOCTOU: revocation pendura só no completed-webhook ou PAT-TTL — suspenso mid-job roda até 2h com `cas:rw` vivo | `index.ts:1625-1731` |
| M2 | `/v1/leases/{id}/runner-diag` SEM auth/rate-limit/lease-validation → log-injection `error`-level ilimitada, poisoning forense | `index.ts:3107-3122` |
| M3 | Cred ticket MULTI-USE até 7200s; completion revoke FAIL-OPEN (catch→swallow) → PAT plaintext vive 2h pós-job, exfiltratable, usável pós-box-destruída | `lib.ts:448-458` + `index.ts:1313-1329` |
| M4 | `SLOT_TTL_S=2700` < duração real de job ⚠️(mecanismo confirmado: slot nunca renovado — keepAlive renova só o CONTAINER, `index.ts:2102`; premissa empírica de jobs >45min não evidenciada no repo, circular) — slot morre mid-job, fleet count sub-reporta → admission em fleet fisicamente cheio → start-failures + churn pra OUTROS tenants | `lib.ts:594,647` + `index.ts:2102` |
| M5 | Queue semantics: NENHUMA. At-cap >30min (`ORPHAN_TTL_S=1800`) → giveup + delete → job PERMANENTEMENTE dropped (GitHub não redeliveria). Sem FIFO/prioridade/posição. Padrão certo EXISTE mas INERT: `pg_queue` (deficit round-robin) vive em `crates/corelink-fabric/src/pg_queue.rs` com zero call sites (`with_durable_queue` unwired, FEATURES.md:556 "⚫ INERT") — portar/wire | `lib.ts:1102,3541-3551` |
| M6 | Mint ANTES do slot-check: dead-letter retry queima mint+revoke pair por tick (60s) por job stuck, agrega sem bound | `index.ts:1653→1678` |
| M7 | JIT config no ARGV (`--jitconfig "$VAR"`) → `/proc/<pid>/cmdline` world-readable o lease inteiro | `entrypoint.sh:199` |
| M8 | Stale-Pending reaper: delete ANTES do teardown; teardown flake = box LEAKED, logged 1×, zero retry (Held-reaper faz certo — teardown-first) | `reaper.rs:946-952` |
| M9 | Spawn bookkeeping write-once-swallow: rhandle:/sbox: puts `.catch(logEvent)` APÓS start — KV falha = standard-4 rodando SEM record (classe exata dos 3 boxes de 10.2h) | `index.ts:1274-1296` |
| M10 | Sem handler `installation.deleted` → uninstall não purga nada; maps env nunca podadas programaticamente (hoje safe: mint 403 hard-deny; residual = noise + falsa confiança de cobertura) | webhooks :2789-2854 |
| M11 | Dual tenant-resolution (installation_id vs PAT-introspect) sem owner-of-record nem validação de deployed-state; incidente SHIPPED: deploy declarativo trocou `REPO_TENANT_PAT_MAP` prod por `{}` → CAS+billing atribuídos ao tenant ERRADO, zero erros | `runner_cas_mint.rs:394+`, `wrangler.jsonc:67-74` ⚠️(linhas corrigidas) |
| M12 | Cold at-ceiling refusal = stranded job sem estado terminal (recordOrphan early-return sem installationId) + slot leak segura fantasma 45min → burst de refusals | `index.ts:1671-1706,1780` |
| M13 | Billing: suspend/rollback Crashed emitido SEM durable acquire stamp → terminal_without_acquire skipped → slot-seconds somem da reconciliação | `app.rs:1465` vs `reaper.rs:536` |
| M14 | Spawn→pickup gap não-faturável (box no `queued`, billing começa no pickup GH) = COGS nosso | `lib.ts:1626` vs spawn timing |
| M15 | Prod vCPU ceiling ADVISORY (warn, never stops) — proteção de revenue == invoice-collection funcionando; materializer s/ STRIPE_PRICE_ID_RUNNER_* setado = dormant = sem cap em lugar nenhum | `runner_mint.ts:472-475`, `warnIfNearVcpuCeiling` |
| M16 | Canary observa counters, não comportamento: zero synthetic transaction (acquire→spawn→release→assert slots voltam a 0). Leak sistemático invisível até pagante passar fome | `cloudflare-canary/src/index.ts` |
| M17 | cas_cred handlers emitem `{"error": msg}` ad-hoc fora do vocabulário frozen `ErrorBody{code,message}` em rota PÚBLICA — mesma classe costume-change do 503/403 do server | `handlers/cas_cred.rs:43` |
| M18 | GH action: `${{ inputs.* }}` interpolado direto em bash script (injeção via reusable-workflow callers; Buildkite hook faz CERTO c/ env-indirect) | `action.yml:133-140` |
| M19 | Lease-existence oracle no cred redeem (401 vs 404 vs 410; ids sequenciais-enumeráveis; ticket 256-bit infeásavel — disclosure minor) | `index.ts:3078-3104` |
| M20 | Secret-inventory drifted: 4+ secrets ausentes da tabela que afirma completude | `docs/runbook/secret-inventory.md` |
| M21 | Fabricd secret pickup exige image churn (rebuild+repin+deploy p/ cada rotação); rollback digests em prosa comentada | wrangler.jsonc:195,204 |
| M22 | Plans doc drift interno num arquivo ($16/$40 ladder vs enum docs $8/$20) | `plans.rs` |

---

# PARTE IV — LOW

Lease-oracle já listado (M19) · replay window >2h re-spawn 1 box desperdiçada (requer secret roubado; aceitar/notar) · exec-server bearer-in-env (real só qd código não-confiado dividir container) · pids ulimit fail-open · memoize key SEM componente tenant (isolamento descansa 100% no CAS-side scoping — dependency note, não bug) · CLI keyset rotation pega primeira key arbitrária (`main.rs:174`) · action `shell: python3` quebra runners bash-only · `CLW_TENANT` fallback no revokeCompletedJob (:1329 — matar padrão antes que copiem pra billing) · recordOrphan get-then-put benign · NF `run_to_completion` 1s×600 ok · README: 8 crates listadas 7; "Firecracker fleet" mentira (é CF Containers; FC é roadmap)

---

# PARTE V — PERFORMANCE: WATERFALL E VEREDICTO DO PITCH

## Critical path direto (customer path, spawn-worker)

| # | Step | Classe | Nota |
|---|------|--------|------|
| 1-4 | webhook→verify→gates→claim→**202** | ~50-100ms | fast-ack correto |
| 5 | mintCasPat → server | 50-500ms | serial |
| 6-8 | stash+KV+slot DO | ~10-40ms | serial |
| 9 | JIT mint (token+jitconfig) | 200-800ms/attempt | cache miss paga JWT+2 RTT GH |
| 10 | `await bumpMetrics` INLINE | ~1-10ms | **puro desperdício** → waitUntil |
| 11 | `container.start()` vs timeout 8s ×3 | **5-60s+** | schedule + multi-GB image pull; slow first pull = ghost churn BY DESIGN (:1121) |
| 13-15 | hydrate lock → runner register → assign | ~5-15s | hydrate backgrounded mas `clw run` serializa atrás do lock |

**Total quente: ~15–30s queued→RUNNING.** Fabricd path: 6+ network legs SERIAIS dentro do handler ANTES do ack (10s exposure do GitHub!) — introspect duplicada (#1 sync e #2 bypassing cache), JIT∥CAS-mint paralelizável, provision async-able.

**Recovery tail:** RECONCILE_MIN_AGE=90s + grace 180s = minutos perdidos quando falha. Direct jobs-API probe ~20-30s fecharia pra segundos.

## Top fixes perf

1. **Image weight vs 8s cap** — medir distribuição do `start()`; pulls >8s queimando ghosts+retries em cold nodes (win: tens de seconds p95)
2. **Fabricd chain**: CachedIntrospect thread (-1 RTT) + JIT∥CAS parallel (-1) + provision async/202 (mata hazard 10s)
3. **Warm-pool tier** — gap estratégico: sleep-after-idle torna pre-booted boxes baratas em baixa concurrency; é o que universaliza o pitch

## Veredito honesto do pitch

Spawn tax de 15–30s **undermine o pitch em jobs pequenos/rápidos** (perde pra pools pre-warmed hosted de 5–15s; cache não repaga em job de 2min). Segura pra compile-heavy Bazel/Rust onde warm AC/CAS economiza minutos. E a matemática assume nós quentes — cold-image node entrega o OPPOSTO da promessa. Fix estratégico: warm-pool/snapshot-restore.

---

# PARTE VI — ECONOMIA

**Charge side (ratificado):** tiers $16/$40/$100/$200/$400; incluso 100–2400 vCPU-h; efetivo **~$0.67/slot-h**; overage $0.30/vCPU-h = $1.20/slot-h (3× COGS).
**Cost side:** base documentada NF $0.10/vCPU-h = **$0.40/slot-h worst-case** ⚠️ taxa REAL de CF Containers aparece EM LUGAR NENHUM no repo — todas as margens herdam proxy. Margem incluso ~$0.27 (~40% floor); overage 67%.
**Freeload fixo:** fabricd singleton sempre-on ~$15/mo proxy · CheckHost std-4 max_instances=1 zero-tráfego ≤$29/mo se warm (prior audit pegou 4 boxes idle billed ~1 mês) · resto ~negligível. Total fixo ~$45/mo — trivial vs um Team sub.
**O buraco estrutural:** "impossível perder dinheiro" hoje descansa em (a) billing collection fail-open com KV backstop, (b) `max_vcpu_h` UNSET server-side (owner-gated), (c) cold-spawn dado de graça (bounded COLD_REPO_CAP=40, mas grátis), (d) hit-rate de memoização UNMEASURED carregando o claim "85–95%" (pricing.md ADMITE).
**Fleet sizing:** cap 250 vs account ~343 compartilhado c/ cache fleet. Spike além do cap → refusal limpo → job queued FOREVER (fires once; externos sem reconciler coverage) = **churn machine no exato momento de growth-success**. Headroom NOT ready (const compilada + edit wrangler + deploy + limit raise CF).
**Single change de maior impacto: flipar `max_vcpu_h` server-side + verificar que Stripe FATURA o overage emitido.** Tudo downstream já shipou esperando esse ato de config. Runner-up: fleet cap env-tunable com queue-backpressure — converte bottleneck em upsell.

---

# PARTE VII — VERIFICADAMENTE EXCELENTE

- **Zombie-box stack profunda:** PID1 trap+SIGKILL escalation (#487), durable idle-backstop stop→destroy ×2, keepAlive via GitHub busy, reapStaleBoxes destroy-only-on-GitHub-idle-verdict, ghost-confirm sweep, stranded accounting. Correlation discipline post-2026-08-02 (never kill on job-keyed lookup; require runner_id match) textbook.
- **Tenancy boundary real:** per-lease Firecracker VM (blast radius memória/CPU/disk = própria box); cross-tenant read/write fechado; disk-fill = own-VM OOM.
- **Webhook crypto:** HMAC-SHA256 constant-time ✓, fail-closed missing-secret ✓, ticket 256-bit random constant-time compare, PAT plaintext nunca logada (redacting-Debug contract nos tipos Rust), env-0 raw-PAT double-flag refusal.
- **Release paths idempotentes:** releaseSlotByJob filter-based não infla; leaked slot self-heals ≤45min; double-release impossível; completed-before-spawn races self-heal.
- **Deploy hygiene:** TODOS images digest-pinned (@sha256 verificados; tarballs SHA256-pinned no Dockerfile); deploys manual-dispatch test-gated; busy-fleet gate recusa roll não-idle (`busy==0 AND unverifiable==0` — ignorance≠idle, direção certa); rollback = re-pin recorded digest.
- **Conformance culture:** vectors byte-exact triple-locked Rust+Py+TS; strict lens deny_unknown_fields separada do parser runtime tolerante (deliberado); manifest.sha256 hash-pinned golden.
- **Hygiene geral:** zero todo!/unimplemented! em código (1 hit = doc-comment); prod-path unwrap/expect = 33, todos invariant-documented ou boot-only; deps workspace exact-pinned `=`; lockfile 46KB; ~1300 tests (fabric-server 726); dual-emitter disjointness (BLAKE3 vs SHA-256 id-spaces) deliberate + tested; Buildkite hook exemplary (env passthrough, arrays, traps).

# PARTE VIII — PLANO DE EXECUÇÃO

**Fase 0 — hoje (dinheiro + config posture):**
1. RC1: `jtenant:` TTL 60d (ou stamp no START) — uma linha, guarda o eixo metered
2. RH1: armar INSTALLATION_ALLOWLIST + fail-closed config assertion
3. RH6: chunk billing flush ≤512 + validar UUID client-side + fix doc
4. RH7: 3-estado parse do max_vcpu_h + suporte fracionário + vector garbage
5. Flip `max_vcpu_h` server-side + verificar primeira invoice Stripe do overage

**Fase 1 — esta semana (perda de job + fleet integrity):**
6. RC3: write-ahead dead-letter pré-spawn (ou DO-alarm drive)
7. RH9: 429→202 + metrics paging streaks
8. RH3: DO acquire fail-CLOSED p/ warm path
9. RH10: claim → ConcurrencySlotsDO
10. RC2 início: ADR-0010 design sign-off (estrutural p/ kill-path)

**Fase 2 — sandbox/egress (pré-traffic externo):**
11. RH4: remover sudo · RH5: egress allowlist + IMDS live-test
12. M2 diag auth · M3 revoke fail-loud + TTL curto · M7 jitconfig off-argv · M8 leak tombstone retry
13. RH8: mint-key boot self-check + secret-inventory backfill

**Fase 3 — estrutura/perf:**
14. Warm-pool tier design · fabricd chain (introspect cache + parallel mints + async provision)
15. Image-start measurement + timeout strategy · durable queue (port pg_queue pattern)
16. Slot heartbeat renewal (M4) · install.deleted handler (M10) · deployed-map self-check (M11)
17. ADR-0010 land (enumerable DO names) · synthetic canary job (M16) · ErrorBody vocabulary (M17) · action.yml env-indirect (M18)

**Fase 4 — contínuo:** docs truth-pass (README crates/Firecracker, plans.rs pricing drift, secret-inventory) · CF rate measurement real (substituir proxy $0.10) · fleet cap env-tunable + queue position surfacing · memoize hit-rate instrumentation (fundamentar o claim 85–95%)

---

# CONTAGEM FINAL

| Severidade | N | Destaques |
|---|---|---|
| Críticos | 3 | billing-zero >2h · orphans unkillable · job-loss window |
| High | 10 | allowlist disarmado · bearer único · DO fail-open · sudo root · egress · poison-pill billing · sentinel collision · rotation prayer · webhook-disable · claim race |
| Medium | 22 | TOCTOU ≤2h · diag sink · multi-use cred · slot TTL · queue none · argv secret · … |
| Low | ~10 | oracle, replay-window, bearer-env, ulimit, memoize-dependency, CLI keyset… |
| Perf | waterfall + top-4 | spawn 15–30s; pitch-verdict honesto |
| Economia | margens + freeload | flip max_vcpu_h = single biggest lever |

**Total substantivo: ~45 achados.** Clean-checks explícitos registrados por agent (release paths, tenancy VM boundary, webhook crypto, deploy pinning, conformance culture).

---

# PARTE IX — TRIPLE-CHECK (verificação independente pós-relatório)

**Método:** minhas mãos nos fatos binários (greps/seds diretos: TTL, sudo, allowlist, fail-open catch, digest pin, MAX_BATCH server-side, parse max_vcpu_h, FLEET consts) + 6 verificadores independentes sobre todos os claims. Contexto descoberto: HEAD `e228249` está na branch `chore/repin-runner-image` (main = `1ad009d`).

## Veredito por seção

| Claim | Veredicto | Correção aplicada |
|---|---|---|
| RC1 billing-zero >2h | ✅ CONFIRMED (a-d) · (e) ⚠️ | Argumento do "GH 6h cap" REMOVIDO — repo nunca cita o cap; correto é keepalive-sem-duration-cap + o próprio código confessando (`index.ts:856` "WRONG as the fleet's…") |
| RC2 orphans unkillable | ✅ CONFIRMED integral (4/4 sub-claims, quotes medidos) | — |
| RC3 waitUntil job-loss | ✅ CONFIRMED (a,d,e,f) · (b,c) ⚠️ | ~30s = conhecimento de plataforma não-citado no repo (mecanismo sim: :3397-3399); stale-claim blocking já derrotado pro scan first-party (#293, releaseSpawnClaim :3414) — perda residual = repos externos |
| RH1 allowlist disarmado | ✅ CONFIRMED | Comment-drift mora em `index.ts`:2996-2999 (não lib.ts); cold≠totalmente unauthenticated (mint 403 server-side permanece) |
| RH2 bearer único | ✅ CONFIRMED integral | — |
| RH3 DO fail-open | ✅ CONFIRMED verbatim | Fix ganha nuance: circuit-breaker bound nos fail-opens consecutivos |
| RH4 sudo root | ✅ CONFIRMED (+agravante: CF substrate sem cap-drop/no-new-privileges; boundary real só microVM) | Fix honesto ≠ remover sudo (quebra CI legit): parar de alegar non-root como hardening OU sudoers escopado |
| RH5 egress | ⚠️ PARTIAL — maior correção do triple-check | IMDS probe RODOU e fechou GO (run #28719027473, handoff doc) — sub-claim "probe owed" era doc-stale com contradição interna no mesmo arquivo (:561 vs :45). Egress allowlist gap PERMANECE (raw sockets, sem filtro de rede) |
| RH6 poison-pill billing | ✅ CONFIRMED 3/3 + precedente próprio (:802 flood bad_region já disparou 1×) | Path corrigido → fabric-server/ |
| RH7 sentinel collision | ✅ mecânica CONFIRMED · divergência cross-repo REFRAMED | Ambos os extremos terminam fail-open-equivalente ("sem ceiling"); achado real fechado: consumer converte garbage/fração em silent-unlimited + nenhum lado representa "ilegível" |
| RH8 rotation prayer | ✅ CONFIRMED (+rico: server-side key name DIFERE — CORELINK_PAT_MINT_AUTH_KEY; J8 presente só em handoffs, ausente de CHANGELOG/inventory) | — |
| RH9 webhook-disable | ✅ paths CONFIRMED · premissa AFIADA | Regra dos 20-não-2xx vale p/ repo hooks; App-webhook não-verificado; repo SILENTE e assume quase-oposto (:3002-3003); risco permanente via streaks não-monitoradas + zero alerting |
| RH10 claim race | ✅ CONFIRMED integral (interleaving + patId overwrite P2>P1 :1669-1673) | — |
| M1-M3, M6-M11 | ✅ CONFIRMED | M6 boundado (~≤27 pairs/job pela janela absoluta — churn real, não unbounded); M7 scope caveat (PID-ns da box, não literal world); M11 linhas 67-74 |
| M4 slot TTL | ⚠️ código ✅ / premissa empírica circular | Slot nunca renovado confirmado (`index.ts:2102` renova só container) |
| M5 queue none | ✅ + reforço | pg_queue existe mas INERT/unwired (FEATURES.md:556) — padrão certo comprado e na gaveta |
| M12-M22 | ✅ 11/11 | M13 caveat: perde só pós-restart do fabricd; M17 enforcement routes operator-gated (cas_cred é o público) |
| Lows spot | ✅ 4/4 | — |
| Perf waterfall | ✅ 5/5 refs verificadas | — |
| Economia | ✅ c/ 1 tensão NOVA | pricing.md:171 ainda afirma hard-stop ("no more compute runs. No overage that leaks") CONTRADIZENDO o modelo advisory+overage do código — drift interno adicional ao M22 |

## Score final do triple-check

**~60 claims verificados: 52 confirmados integralmente · 6 confirmados com afinação de escopo/framing (RC1e, RC3bc, RH5-IMDS, RH7-asymmetry, RH9-premise, M4-premissa) · 1 path errado corrigido (RH6) · 1 file-ref corrigido (RH1) · 0 refutações totais.**

Novos micro-achados da verificação (incorporados): `index.ts:856` self-confession do RC1 · precedência do flood bad_region no RH6 · contradição interna :561-vs-:45 no RH5 · pricing.md:171 vs código · J8 ausente do CHANGELOG/inventory · branch context.

---

*Gerado por 10 agents read-only sobre HEAD `e228249` (branch `chore/repin-runner-image`; main = `1ad009d`), working tree limpo. Exclusões CHANGELOG-documentadas respeitadas. Triple-check independente incorporado na Parte IX — correções marcadas ⚠️inline. Relatório independente dos audits do corelink-server (mesma data, repos distintos).*