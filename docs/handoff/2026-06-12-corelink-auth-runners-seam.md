# Pedido de integração → corelink-server: seam de auth + billing para o fabric em produção

**De:** corelink-runners techlead · **Para:** corelink-server techlead + owner ·
**Data:** 2026-06-12 · **Status:** PROPOSTA — aguardando confirmação/ajuste do
shape do contrato (lado de vocês) ·
**Referência:** `docs/handoff/2026-06-12-corelink-auth-billing-integration-request.md`
(pedido anterior, mais aberto; este é o follow-up cirúrgico — o binário existe e
está vivo; a seam está mapeada ao byte).

---

## 1. O que mudou desde o pedido anterior

O pedido de 2026-06-12 anterior identificou os dois buracos (auth + billing) mas
foi aberto — o binário não existia ainda. Hoje o fabric está vivo:
`corelink-fabricd` (PR #21, `main`) é um servidor axum real,
gate completo verde (fmt · clippy · test · deny · audit), executando em
`corelink-runners-builder-01`. A seam de auth está implementada e testada; o que
falta é trocar a implementação M1 pelo adaptador de produção que aponta para a
plataforma de vocês. Este documento fixa o contrato exato contra o qual vamos
codificar — vocês confirmam ou ajustam, depois congelamos.

---

## 2. O estado M1 (o que existe agora)

**`crates/corelink-fabric-server/src/auth.rs`** — trait `TokenStore`:

```rust
pub trait TokenStore {
    /// Ok(Some(tenant)) → autenticado
    /// Ok(None)         → PAT desconhecido → 401
    /// Err(Unreachable) → loja inacessível → 503 fail-closed (jamais admite)
    fn tenant_of(&self, token: &str) -> Result<Option<TenantId>, TokenStoreError>;
}
```

**`StaticTokenStore`**: mapa em memória `PAT → TenantId`. Wired em
`server.rs::build_app_and_state` via `FABRIC_PAT` / `FABRIC_TENANT` — uma única
entrada bootstrap. O comentário no código diz explicitamente:

> *"M1 production expands this to the CoreLink Cache PAT backend; the seam is the
> same `TokenStore` trait."*

**`StaticPlans`**: `max_concurrency` e `rate_ceiling_per_min` por tenant, via
`FABRIC_TENANT_MAX_CONCURRENCY` / `FABRIC_TENANT_RATE_PER_MIN`. Mesma situação —
bootstrap estático até o plano real chegar da plataforma.

Ambos são drop-in substituíveis: o trait está congelado, o resto do binário não
toca nos detalhes internos da loja. Nenhuma mudança de interface é necessária —
só uma nova impl atrás do trait existente.

---

## 3. Ask 1 — PAT introspection: shape proposto

Precisamos de um endpoint que o `CoreLinkTokenStore` (o adaptador que vamos
construir) chame para cada Bearer PAT recebido.

**Request proposto:**

```
POST /internal/v1/auth/introspect
Authorization: Bearer <fabric-service-credential>
Content-Type: application/json

{ "token": "corelink_..._t_xxx.xxx.xxx" }
```

**Response proposto:**

```jsonc
// token válido e ativo:
{
  "valid": true,
  "tenant_id": "acme-corp",          // string, mesmo espaço de IDs do Cache
  "plan": "pro",                      // opcional M1; obrigatório M2
  "max_concurrency": 40,              // opcional M1; preferido sobre StaticPlans
  "rate_ceiling_per_min": 120         // opcional M1
}

// token inválido, expirado, ou desconhecido:
{
  "valid": false
  // nenhum tenant_id — jamais um default
}
```

**Semântica fail-closed (não negociável do nosso lado):**

- `valid: false` → `Ok(None)` no trait → 401 ao cliente.
- Timeout / network error / status 5xx → `Err(Unreachable)` → 503 ao cliente.
  O fabric **jamais** cai em admissão anônima.
- O campo `tenant_id` só aparece quando `valid: true`. Um response com
  `valid: false` e `tenant_id` presente é tratado como `valid: false`.

**Perguntas para vocês:**

1. O shape acima é compatível com o que `corelink-auth` já expõe (ou pode
   expor)? Se a rota existir com outro nome / método, apontamos para ela.
2. O `tenant_id` é o mesmo espaço de IDs que o Cache usa hoje (`org = tenant`,
   ADR-0002)? Confirmação explícita fecha o loop.
3. Vocês querem autenticar o próprio pedido de introspection via um credential
   de serviço fixo (secret) ou via mTLS? Implementamos qualquer um; precisamos
   saber o mecanismo para provisionar o secret.
4. Qual é o SLO de latência e o comportamento esperado sob degradação parcial?
   Precisamos calibrar o timeout antes de acionar `Unreachable`.

---

## 4. Ask 2 — Billing: o SKU de slot e o ponto de integração

O modelo de preço está decidido (`docs/product/pricing.md`, owner 2026-06-12):
**flat por concorrência, nunca por minuto**. A unidade faturável é o **slot**
(lease concorrente ativo). O fabric já emite `SlotOccupancyEvent` + pico por
tenant (WP-BIL1) e mapeia tier → cap (WP-BIL2). O que falta é ligar isso ao
Stripe de vocês em vez de subir uma segunda integração.

**O que propomos:**

O fabric reporta ao billing de vocês a quantidade de slots *comprada* (N) por
tenant no momento de onboarding/upgrade, e a plataforma materializa isso como uma
assinatura flat no Stripe (licensed quantity, não usage meter). Occupancy
instantânea não é cobrada — o cliente paga pelo tier, os caps são enforcement,
não medição.

A única pergunta de medição que sobra é o **vCPU-hours ceiling** (hard cap
anti-perda; ver `pricing.md §3`): o fabric pode reportar vCPU-h consumido por
tenant quando o ceiling é atingido para acionar upgrade, se vocês quiserem
guardar esse número no billing.

**Perguntas para vocês:**

1. O `corelink-billing` suporta hoje um SKU de assinatura flat (licensed
   quantity = slots comprados) além dos usage meters existentes? Se sim, qual
   é o `price_id` Stripe que deveremos referenciar (ou vamos criar um novo)?
2. Como o plano comprado chega até nós como source of truth para o cap —
   **push** (webhook em plan_change) ou **pull** (consultamos no acquire)?
   A nossa `PlanRegistry` aceita os dois; precisamos saber qual vocês suportam
   para wiring M2.
3. O princípio "nunca cobrar duas vezes" (`pricing.md §1`, CLAUDE.md): um job
   memoizado consome zero slot e deve faturar zero. O fabric já codifica isso
   (sem acumulador de duração por construção); confirmem que o billing de vocês
   honra "slot ocupado é a única unidade" — sem meter por request, por minuto,
   ou por cache hit.

---

## 5. O que vamos construir no nosso lado (fronteira clara)

Vocês **não** precisam importar nenhum tipo deste repo. O contrato é HTTP/JSON,
transcrito em cada lado — mesma disciplina do seam hugit↔runners
(`docs/spec/hugit-integration-contract.md` v1.2.0 e os conformance vectors em
`conformance/`). Nenhum `path`-dependency, nenhum `git`-dependency entre repos
(`deny.toml` enforces crates.io only em ambos).

| O que o fabric constrói | Gating |
|---|---|
| `CoreLinkTokenStore`: impl do trait `TokenStore` que chama o endpoint de introspection de vocês; timeout configurável; `Err(Unreachable)` em qualquer falha | Atrás de `FABRIC_AUTH_BACKEND=corelink` (default: `static`); drop-in para `StaticTokenStore` sem mudar nada no resto do binário |
| `CoreLinkPlanStore`: impl de `PlanStore` que consulta (pull) ou recebe (push) o plano do tenant e popula `max_concurrency` / `rate_ceiling_per_min` | Atrás de `FABRIC_PLANS_BACKEND=corelink` (default: `static`); substitui `StaticPlans` |
| Reporter de slot-occupancy + vCPU-h ceiling | Já existe como `SlotOccupancyEvent`; a wiring ao endpoint de billing de vocês é a única adição |

Nenhuma dessas peças requer mudança no contrato público do fabric (`corelink-fabric-api`).
O binário de produção continua sendo `corelink-fabricd`; o changeset é
interno ao `corelink-fabric-server`.

---

## 6. Ação solicitada

**corelink-server techlead → owner:** confirmar ou ajustar os dois shapes abaixo
e responder às perguntas numeradas nas seções 3 e 4. Roteie via owner.

**Shape 1 — PAT introspection** (seção 3): confirmar rota, método,
formato de resposta, mecanismo de autenticação do pedido de serviço, e SLO.

**Shape 2 — Billing slot SKU** (seção 4): confirmar suporte a licensed-quantity
SKU, mecanismo de entrega do plano (push/pull), e honra ao princípio
"slot = única unidade faturável."

Uma vez congelados os dois shapes, abrimos o WP de integração no nosso lado
(`CoreLinkTokenStore` + wiring de billing) sem nenhuma mudança no `corelink-server`.
