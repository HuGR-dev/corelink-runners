# → corelink-server: Runners price ladder RATIFIED — unblock `max_concurrency` on introspect

**De:** corelink-runners techlead · **Para:** corelink-server techlead (via owner) ·
**Data:** 2026-06-13 · **Status:** ratificação confirmada — podem adicionar o campo `max_concurrency` (M2). ·
**Em resposta a:** `corelink-server/docs/handoff/2026-06-12-corelink-runners-auth-seam-response.md`
(§Ask-2, "falta uma ratificação sua: os números de preço da ladder de Runners").

---

## TL;DR

A única pendência que vocês listaram pra fechar 100% o seam de billing — a
**ratificação dos preços** da ladder de Runners — **está ratificada**. A ladder canônica é
`docs/product/pricing.md §2` (owner-decided 2026-06-12), e `docs/product/product.md §5`
redireciona pra ela. Podem wirar a `entitlement` de Runners → `max_concurrency` no
`POST /internal/v1/auth/introspect`.

## A ladder ratificada (pricing.md §2 — verbatim)

| Tier de Runners | $/mo | `max_concurrency` (slots) |
|---|---|---|
| Starter | $8 | **20** |
| Pro | $20 | **40** |
| Team | $50 | **80** |
| Scale | $100 | **160** |
| Max | $200 | **320** |

- Bate **exatamente** com a cap table da sua §Ask-2 (20/40/80/160/320) — concorrência
  estrutural, agora com os $ ratificados.
- Tenant **só-Cache** (sem entitlement de Runners) → `max_concurrency` **ausente/0**
  (cai no nosso fail-closed: cap 0 admite nada). Mantido.
- `rate_ceiling_per_min` **não existe** no seu modelo — confirmado, não o emitam. Nós o
  mantemos como placeholder derivado/opcional do nosso lado (TODO(owner), `plans.rs`),
  não é dimensão de pricing nem do seu nem do nosso billing vivo.
- Acima de Max: **Enterprise** (custom/BYOC, governance) — não é uma row fixa da tabela.

## O que já está pronto do nosso lado

- **`CoreLinkTokenStore`** (PR #29) consome o seu contrato congelado
  (`POST /internal/v1/auth/introspect`, header `X-Corelink-Internal-Auth`, fail-closed:
  só `200 valid:true` admite; 401/5xx/transport → 503, nunca um 401 falso de tenant real).
  `FABRIC_AUTH_BACKEND=corelink`, default `static`.
- **`PlanTier`** (`crates/corelink-fabric/src/plans.rs`) já foi **alinhado à ladder
  canônica** (Starter/Pro/Team/Scale/Max @ 20/40/80/160/320) — era a stale 1/1/4/12 de
  um draft pré-decisão. Os nomes não colidem com os tiers de Cache no nosso lado (o
  crate namespaceia o enum); o eixo de slots é separado do tier de Cache, como vocês
  especificaram.

## A ação de volta

1. Adicionem `max_concurrency` (e, se quiserem, `plan` já servido) à resposta do
   `/introspect` quando a entitlement de Runners do tenant estiver presente — campo
   aditivo, `Option` + skip-if-none, não quebra o contrato M1.
2. Provisionar o valor de `FABRIC_INTROSPECT_AUTH_KEY` (secret dedicado) via owner no go-live.

Assim que o campo sair, construímos o `CoreLinkPlanStore` (deriva o cap inline no acquire,
1 round-trip, pull — sem transporte novo) e o seam de billing fecha 100%.

## Referências de deploy (pra contexto do go-live)

- `corelink-runners/deploy/RUNBOOK.md` — como o `corelink-fabricd` é provisionado
  (Northflank combined service) e onde os secrets entram (`FABRIC_INTROSPECT_AUTH_KEY`,
  `NORTHFLANK_*`, `FABRIC_SIGNING_KEY`).
- `corelink-runners/docs/spec/hugit-integration-contract.md` v1.2.0 — o contrato de wire
  que o fabric satisfaz (pra referência cruzada do tenant_id = org).

— roteado via owner; nenhuma mudança exigida de vocês além do campo aditivo; nenhum
`path`/`git`-dependency entre repos.
