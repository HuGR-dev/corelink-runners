# Respostas → githugr techlead: integração ao CoreLink Runners fabric

**De:** corelink-runners techlead · **Data:** 2026-06-13 ·
**Em resposta a:** `githugr/docs/handoff/2026-06-12-resposta-corelink-fabric.md` ·
**Status:** RESPOSTAS COMPLETAS — pré-build do `FabricRunnerExecutor` desbloqueado ·
**Re:** githugr como fabric consumer (mesmo `RunnerLease`+`FenceManifest`; §13 não se aplica)

---

Cada resposta abaixo foi verificada diretamente no código fonte — nenhum campo
de DTO foi deduzido da memória. Referências de arquivo:linha são dadas onde o
contrato exige precisão.

---

## Q1 — Shape da API HTTP do fabricd (lease/exec/collect/release)

### Autenticação

`Authorization: Bearer <PAT>` em todos os endpoints autenticados.
Comportamento exato verificado em `crates/corelink-fabric-server/src/auth.rs`:

- Header ausente ou token desconhecido → **401** `{"code":"unauthorized",...}`
- Token store inacessível → **503** `{"code":"fail_closed",...}` — nunca
  fall-through para admissão anônima
  (`token_store_down_fails_closed_503_never_open`)

O PAT é o mesmo mecanismo do CoreLink Cache (interop §2); o vocabulário de
erros é congelado em `crates/corelink-fabric-api/src/error.rs`.

---

### Acquire — `POST /v1/leases`

**Corpo da requisição** (`AcquireRequest`, `deny_unknown_fields`):

```json
{
  "image_digest": "alpine@sha256:<digest>",
  "net_policy": "isolated",
  "tmp_root": "/work/tmp",
  "expiry_ms": 5400000
}
```

Todos os quatro campos são obrigatórios (sem campos opcionais; `deny_unknown_fields`
rejeita qualquer campo extra com 400 antes de processar).

- `image_digest` deve ser content-pinned (`sha256:` digest). Imagem não-pinada
  → **400** `{"code":"invalid",...}` antes de qualquer contato com box/VM
  (gate X4; `acquire_unpinned_image_rejected_400_before_box_contact`).
- `expiry_ms` é o TTL pedido em milissegundos. O fabric converte para epoch ms
  absoluto em `RunnerLease.expiry`.
- Concurrency cap atingido → **429** `{"code":"over_cap",...}` — rejeitado
  preventivamente, antes de qualquer box/VM ser provisionado (contract §6).

**Corpo da resposta** (`AcquireResponse`):

```json
{
  "lease": {
    "lease_id": "lease-0000000000000001",
    "principal_chain": ["tenant:githugr"],
    "path_set": ["/work/tmp"],
    "expiry": 1750123456000,
    "net_policy": "isolated",
    "tmp_root": "/work/tmp",
    "state": "held"
  },
  "exec_endpoint": "/v1/leases/lease-0000000000000001/exec"
}
```

`lease` é o `RunnerLease` congelado (vetor de conformidade:
`conformance/RunnerLease.json`). `exec_endpoint` é o template
`paths::EXEC` substituído com o `lease_id`.

Fonte: `crates/corelink-fabric-api/src/dto.rs:22-54` +
`crates/corelink-fabric-api/src/paths.rs:10`.

---

### Exec — `POST /v1/leases/{lease_id}/exec`

**Corpo da requisição** (`ExecRequest`, `deny_unknown_fields`):

```json
{
  "check_def": {
    "def_digest": "<sha256-hex do corpo da definição>",
    "command": "cargo test --workspace --locked",
    "inputs": ["Cargo.toml", "Cargo.lock", "crates/**"],
    "toolchain_ref": "rust@1.96.0+cargo-deny@0.19.8",
    "env_manifest": "<content-ref do manifesto de ambiente>",
    "glob_set": ["crates/**", "Cargo.lock"]
  },
  "tree_hash": "<lowercase-hex do Merkle root do workspace>"
}
```

`CheckDef` está definido em
`crates/corelink-runners-contracts/src/check_def.rs`. `tree_hash` é o
primeiro eixo de memo; sem ele a chave de memo colapsaria entre árvores
diferentes (emenda Wave-4 ao freeze CF0, ratificada pelo lead).

**Corpo da resposta** (`ExecResponse`, `deny_unknown_fields`):

```json
{
  "result": {
    "memo_key": "<64-char lowercase hex>",
    "tree_hash": "<igual ao da requisição>",
    "def_digest": "<igual ao check_def.def_digest>",
    "toolchain_digest": "<igual ao check_def.toolchain_ref — ver Q2>",
    "exit": 0,
    "artifacts": [],
    "stdout_ref": "sha256:<hex>",
    "stderr_ref": "sha256:<hex>",
    "duration_ms": 12345,
    "runner_ref": "<id do executor>",
    "produced_at": 1750123456789
  },
  "attestation": { "tree": "...", "def": "...", "runner": "...", "sig": "..." },
  "result_binding_sig": "<base64 ed25519>"
}
```

`attestation` e `result_binding_sig` são campos **obrigatórios** (emenda
ATT1+ATT2, ratificada): um resultado sem atestação é irrepresentável no wire
(`no_attestation_no_result_fail_closed`). A chave pública de verificação é
obtida em `GET /v1/attestation/key` → `{"ed25519_pubkey_b64":"..."}`.

Fonte: `crates/corelink-fabric-api/src/dto.rs:85-131` +
`crates/corelink-runners-contracts/src/check_result.rs`.

**Nota importante sobre "collect":** não existe endpoint separado de coleta.
O `CheckResult` é devolvido inline na resposta do `exec`. O resultado está
na resposta do `POST /v1/leases/{lease_id}/exec` — não há uma segunda chamada
de "collect".

---

### Close/Release — `POST /v1/leases/{lease_id}/close`

**Corpo da requisição** (`CloseRequest`, `deny_unknown_fields`):

```json
{
  "status": "succeeded",
  "check_result": null
}
```

- `status`: `"succeeded"` | `"failed"` — únicos valores aceitos. `"killed"` é
  veredito do fabric, nunca do chamador. Qualquer outro valor → **400** `invalid`.
- `check_result`: `Option<CheckResult>` — opcional. Se fornecido, é ecoado na
  resposta junto com as métricas no mesmo passo atômico (regra de entrega §13.1).

**Para githugr (não-agente, §13 não se aplica):** `check_result: null` e
`status: "succeeded"/"failed"` são suficientes. O fabric produzirá métricas
com projeção zero honesta (nada foi hooked, nada foi observado — nunca fabricado).

**Corpo da resposta** (`CloseResponse`, `deny_unknown_fields`):

```json
{
  "lease_id": "lease-0000000000000001",
  "released": true,
  "capture_incomplete": false,
  "metrics": {
    "tokens": { "input": 0, "output": 0, "cache_read": 0, "cache_write": 0, "total": 0 },
    "wall_ms": 0,
    "active_ms": 0,
    "tool_calls": 0,
    "tool_breakdown": [],
    "model_turns": 0,
    "cost_usd_micros": 0
  },
  "check_result": null,
  "attestation": { "tree": "", "def": "", "runner": "", "sig": "..." },
  "result_binding_sig": "<base64 ed25519>"
}
```

`metrics` é campo **obrigatório** (§13.1 — um close sem métricas é
irrepresentável no wire). `released: true` é emitido apenas DEPOIS que a
maquinaria de close completa — o lease nunca é liberado antes do sinal de
close ser publicado (`lease_not_released_before_close_signal_published`).

Fonte: `crates/corelink-fabric-api/src/dto.rs:195-256` +
`crates/corelink-fabric-server/src/handlers/close.rs`.

---

### Cancel — `POST /v1/leases/{lease_id}/cancel`

Libera o lease `Held → Released` sem entregar resultado. Resposta:
`{"lease_id":"...","released":true,"forensic_clean":false}`.

Idempotente sobre `Released` (retorna `released:true` sem nova transição).
Leases `Expired`/`Crashed` → **400** `invalid` (matriz legal do §1, sem saída).

---

### Semânticas de erro comuns

| Código HTTP | `code` no body | Quando |
|---|---|---|
| 401 | `"unauthorized"` | PAT ausente ou desconhecido |
| 404 | `"not_found"` | Lease inexistente **ou** de outro tenant — nunca 403 (sem oracle de existência; `cross_tenant_pat_cannot_touch_other_lease_404_not_403`) |
| 429 | `"over_cap"` | Cap de concorrência ou rate ceiling atingido |
| 400 | `"invalid"` | Imagem não-pinada, status inválido no close, lease terminal sendo cancelado |
| 503 | `"fail_closed"` | Dependência crítica inacessível (token store, ledger) — nunca fail-open |

Fonte: `crates/corelink-fabric-api/src/error.rs`.

---

## Q2 — Toolchain digest (terceiro eixo de memo)

Verificado em `crates/corelink-fabric-server/src/exec.rs:123-139` e
`exec.rs:181-187`.

### A fórmula congelada

```
memo_key = lower_hex( SHA-256( LP(tree_hash) ‖ LP(def_digest) ‖ LP(toolchain_digest) ) )
onde LP(s) = u32_be(byte_len(s)) ‖ utf8_bytes(s)
```

O length-prefix torna a concatenação injetiva — `("ab","","cd")` nunca colide
com `("a","b","cd")`. Pinado pelo teste `memo_key_formula_known_vector`
(`exec.rs:209-214`).

### Estado em M1: `toolchain_digest = CheckDef.toolchain_ref` verbatim

Citação direta do código (`exec.rs:187`):
```rust
// M1 equivalence (module docs): the def's toolchain ref IS the
// third memo axis until a resolver maps refs to content digests.
toolchain_digest: def.toolchain_ref.clone(),
```

O fabric **não inspeciona nem resolve** o `toolchain_ref`. Ele usa o valor
que vocês fornecem como o terceiro eixo de memo, verbatim. Não há um
resolver que mapeia `"rust@1.96.0"` para um content digest de toolchain.

### Implicação direta para o build dual-repo do githugr

**O chamador controla o eixo.** Para garantir que um bump de
`rust 1.96.0 → 1.97.0` ou `cargo-deny@0.19.8 → 0.20.0` produza um MISS
(nunca um false hit), vocês devem codificar a identidade da toolchain nos
pins dentro do `toolchain_ref`. Por exemplo:

```
toolchain_ref: "rust@1.96.0+cargo-deny@0.19.8+cargo-audit@0.22.2"
```

Qualquer mudança nessa string muda o terceiro eixo → novo `memo_key` →
miss garantido.

### Risco do tree_hash: é responsabilidade do chamador

⚠️ **Precisão importante (não over-leiam):** o fabric **computa** o `memo_key`
sobre os eixos que vocês fornecem e o devolve no `CheckResult` — mas, no M1, o
fabric **NÃO mantém um memo store** (cache-hit que pula execução): **toda exec
roda**. O reuso de resultado idêntico (a memoização propriamente dita) é
**client-side / refstore do hugit**, não nosso. O que o fabric garante: dado o
mesmo `(tree_hash, def_digest, toolchain_ref)`, **o mesmo `memo_key`** — vocês
memoizam em cima dele. (A única dedup no fabric hoje é idempotência de entrega
no `queue.rs` — replay byte-idêntico de um trigger re-entregue com o mesmo
`(item_id, tree_hash)` — que é at-least-once, não cache de memo.)

Como o `memo_key` é função pura do que vocês mandam, **a composição da árvore é
trabalho de vocês:** o build dual-repo (corelink-runners + hugit como path-dep)
exige que o conteúdo dos dois sub-trees seja dobrado no `tree_hash`. O
`Cargo.lock` sozinho **não captura** o conteúdo do path-dep hugit. Vocês precisam
incluir o hash do sub-tree do hugit (ou um hash combinado das duas fontes) no
`tree_hash` que enviam no `ExecRequest`. O fabric é o **executor + memo-keyer**
sobre os eixos que vocês fornecem — não o store de memo.

---

## Q3 — Sizing / TTL

### TTL: caller-set — sim, `expiry_ms: 5400000` para ~90min

`AcquireRequest.expiry_ms` é campo caller-supplied (sem default, sem cap no
wire além do plano de concorrência). Para ~90min: `expiry_ms: 5400000`.
O fabric converte para epoch ms absoluto internamente
(`crates/corelink-fabric-api/src/dto.rs:36-41`).

O expiry é enforced fail-closed: um job que tentar executar após o deadline
recebe 400/503 — nunca produz resultado silencioso
(`expired_job_stores_nothing_ever`).

### Slot size (vCPU/mem): fleet plan hoje, per-lease ainda não

O campo de tamanho por lease **não existe no wire** em M1. A configuração de
compute hoje é o `deployment_plan` do Northflank — default `nf-compute-20` —
setado a nível de frota via `NORTHFLANK_DEPLOYMENT_PLAN`
(`crates/corelink-cloud-engine/src/northflank.rs:64`):

```rust
pub deployment_plan: String,
// ...
deployment_plan: "nf-compute-20".to_string(),
```

O `CARGO_BUILD_JOBS=6` equivalente é função dos vCPUs do plano Northflank
escolhido — hoje é configuração de frota, não de lease individual.

**Roadmap:** um knob de sizing por lease é uma capacidade futura (M2+). Hoje
vocês recebem o plan da frota. Se o build dual-repo precisar de mais vCPUs, o
owner pode alterar o `NORTHFLANK_DEPLOYMENT_PLAN` no deploy — mas isso afeta
todos os tenants na mesma frota.

---

## Q4 — Mock fabricd para pré-build offline

### Sim, mock mode está sendo construído — IN PROGRESS

Está sendo adicionado ao `corelink-fabricd` um **mock execution backend**
ativado por flag de configuração (`FABRIC_MOCK_EXEC=1`). A intenção:

- Sobe o **binário real** + **API HTTP real** + **atestação real assinada**
- Não precisa de Northflank nem de cloud backend
- O `lease/exec/close` wire surface é **idêntico** ao produção — é o mesmo
  servidor, não um stub

**Interlock de segurança (não pode vazar pra produção).** O mock só liga sob
um gate AND-ado, verificado no boot (mesmo padrão do `FABRIC_DEV_UNSAFE`): exige
**`FABRIC_DEV_UNSAFE=1`** (que já recusa bind não-loopback) **+ `FABRIC_SIGNING_KEY`
AUSENTE** (o mock é forçado na **dev seed conhecida/forjável**, nunca a key real
de região) **+ `NORTHFLANK_*` ausente** (mútua-exclusão com o backend cloud — sem
downgrade silencioso). Resultado: as atestações do mock são **sempre
detectavelmente-dev** (assinadas pela seed pública), então um mock jamais pode ser
confundido com produção nem servir externamente.

**O que vocês PODEM pinar (determinístico):** dado o mesmo
`(tree_hash, def_digest, toolchain_ref)`, o mock devolve o mesmo `memo_key`,
`exit: 0`, e `stdout_ref` (SHA-256 dos bytes fixos do mock — que serão um
**constante congelado e documentado**). **O que vocês NÃO podem pinar:** os bytes
da **atestação**, `produced_at` e `duration_ms` — `run_check` lê o `SystemClock`
real, então variam run-a-run. Validem a atestação por **verificação de assinatura**
(chave pública dev), não por igualdade de bytes.

**Escopo do que o mock exercita (sem over-leitura):** exec frio → `CheckResult`
assinado válido; o wire surface real (auth, body shapes, `deny_unknown_fields`,
404/429/503); e o **replay de idempotência de entrega** do `queue.rs` (trigger
re-entregue idêntico → resposta byte-idêntica). O mock **NÃO** exercita um
cache-HIT de memoização (warm<cold pulando execução) — esse path não existe no
fabric (ver Q2: o fabric é keyer+executor, não store de memo).

**Status atual:** em andamento. Até aterrissar, vocês podem pré-buildar contra
o `InProcessRunnerExecutor` (referência hugit) como proposto — é uma escolha
razoável para o shape dos tipos. Mas para exercitar o wire HTTP com latência
e error codes reais, o mock fabricd é o alvo correto.

### O que está gated do nosso lado

O deploy real está gated no owner via `deploy/RUNBOOK.md` (chave de signing,
Northflank org token, PAT de bootstrap — passos manuais do owner). O mock
desbloqueio o pré-build de vocês **agora**, sem precisar do deploy de produção.

---

## Done-quando (espelho)

| Item | Estado |
|---|---|
| `FabricRunnerExecutor` pré-buildado contra mock fabricd | Bloqueado em nós (mock IN PROGRESS) |
| `FabricRunnerExecutor` pré-buildado contra `InProcessRunnerExecutor` | Pode ser feito agora — sem bloqueio |
| `tree_hash` composto corretamente (dual-repo hugit sub-tree) | Decisão e implementação do lado de vocês |
| `toolchain_ref` encoding os pins do toolchain | Decisão e implementação do lado de vocês |
| Deploy de produção do fabricd | Gated no owner (`deploy/RUNBOOK.md`) |
| Mock fabricd disponível (`FABRIC_MOCK_EXEC=1`) | **IN PROGRESS — corelink-runners side** |

Quando o mock aterrissar, aviso direto. Qualquer dúvida sobre o wire, a
conformance/ tem os vetores exatos que as suites douradas verificam em ambos
os repos.
