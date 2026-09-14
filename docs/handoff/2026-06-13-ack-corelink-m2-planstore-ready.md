# → corelink-server: ACK M2 plan — `CoreLinkPlanStore` is ALREADY built; lock the field shape

**De:** corelink-runners techlead · **Para:** corelink-server techlead (via owner) ·
**Data:** 2026-06-13 · **Status:** ACK + one shape-lock to prevent drift ·
**Em resposta a:** `corelink-server/docs/handoff/2026-06-13-corelink-runners-m2-maxconcurrency-plan.md`

---

## TL;DR

Plano M2 recebido e batido — ladder idêntica, campo aditivo, `rate_ceiling` fora,
`FABRIC_INTROSPECT_AUTH_KEY` provisionado: tudo certo. **Uma correção de timeline:** o
`CoreLinkPlanStore` **já está construído e mergeado do nosso lado** (não "depois que vocês
shiparem"). Então, no instante que o M2 ficar live-serving, é **só virar a chave** do nosso
lado — sem build novo. Abaixo, a shape EXATA que nós consumimos, pra vocês shiparem
byte-compatível e a gente não criar drift.

## 1. Confirmações batidas

- Ladder Starter/Pro/Team/Scale/Max @ 20/40/80/160/320 = nossa `pricing.md §2` e a sua
  cap-table. ✅
- `max_concurrency` aditivo (Option, skip-if-none); só-Cache → ausente → nosso fail-closed
  cap-0. ✅ É exatamente o que o `CoreLinkPlanStore` espera.
- `rate_ceiling_per_min`: não emitam — derivamos como placeholder do nosso lado. ✅
- Enterprise custom/BYOC fora da tabela. ✅

## 2. **Já construído** do nosso lado (PR #32, mergeado)

`CoreLinkPlanStore` (`crates/corelink-fabric-server/src/corelink_plans.rs`) já consome o
`max_concurrency` do mesmo `/introspect`, **fail-closed**:
- `200 {valid:true, max_concurrency:<u32>}` → cap aplicado.
- `200 {valid:true}` **sem** `max_concurrency` (estado M1 / tenant só-Cache) → `Ok(None)` →
  acquire **rejeitado over-cap** (tenant autenticado mas sem entitlement). Honesto, não 503.
- `valid:false` → `Ok(None)`. `503`/transporte/malformado → `Unreachable` → 503 (nunca um
  401/admissão falsa).
Atrás de `FABRIC_AUTH_BACKEND=corelink` (default `static`). **Nada a construir quando vocês
shiparem — só apontamos o backend.**

## 3. ⚠️ Shape-lock (pra não criar drift — o tripwire ainda não cobre este campo)

A shape que o `CoreLinkPlanStore` parseia HOJE (provisória, marcada como tal no módulo):

```jsonc
// 200, valid:true, tenant COM entitlement de Runners:
{ "valid": true, "tenant_id": "<uuid>", "plan": "pro", "max_concurrency": 40 }
//                                                       ^^^^^^^^^^^^^^^^^^^^^
//  campo TOP-LEVEL, inteiro (u32), nome exatamente "max_concurrency"
```

**Por favor shipem assim:** `max_concurrency` como **inteiro top-level** na resposta
`valid:true` (NÃO aninhado dentro de `plan`, NÃO string). Se a sua shape final divergir
(nome/tipo/aninhamento), me avisem ANTES do ship — é um ajuste de uma linha no nosso parser,
mas só se eu souber. Se bater com o acima, zero mudança nossa.

**Proposta (wire-contract law):** quando o M2 ficar firme, vamos **congelar um conformance
vector** pra esse campo (hoje o tripwire de drift §13.4 cobre `IntentMetrics` mas NÃO o
`max_concurrency` do introspect). Eu proponho a shape; vocês ratificam; comitamos
byte-idêntico nos dois repos. Fecha o último buraco de drift cross-repo.

## 4. Estado / não-bloqueio

- Nosso fabric está **LIVE na Northflank** rodando `FABRIC_AUTH_BACKEND=static` (tenant
  bootstrap), então o gap de "M1 introspect ainda não live-serving" **não nos afeta agora**.
- Trocamos pro backend `corelink` quando: (a) o M2 `max_concurrency` estiver live-serving, e
  (b) o `FABRIC_INTROSPECT_AUTH_KEY` for compartilhado via owner.
- **Avisem quando o rebuild M2 sair** (batido com o CAA-360, ~31min, 1 build) — eu confirmo
  a shape contra o §3 e a gente vira a chave + congela o vector.

— roteado via owner; nenhum `path`/`git`-dependency entre repos.
