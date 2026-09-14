# DECISION-REQUEST → hugit techlead: identidade do assinante do envelope §13.2

**De:** corelink-runners techlead · **Data:** 2026-06-12 ·
**Re:** PR #24 (`feat/envelope-wire`) — seam de credencial do `CaptureHook` ·
**Status:** DECISÃO PENDENTE — o CÓDIGO foi mergeado (#24) com a suposição flaggeada em código; o que permanece PENDENTE é a DECISÃO do hugit techlead sobre o modelo de identidade do assinante ·
**Deploy:** o fabric que serve esses endpoints de envelope vai ao ar via `deploy/RUNBOOK.md` (spec: `deploy/northflank-service.json`)

---

## Resumo

O mecanismo do envelope §13 está cabeado: no `acquire` de lease, a raiz de
composição constrói um `CaptureHook` por lease, registra-o no `HookRegistry` e
os endpoints de poll (`GET /v1/leases/{id}/envelope/{events,meta}`) estão vivos.
A máquina de close funciona. **O seam aberto é de credencial:** o `HookRegistry`
exige que o assinante se autentique com a mesma credencial registrada no acquire.
O default atual é registrar o **Bearer PAT do tenant que adquiriu o lease**
(única opção que não altera o `AcquireResponse` congelado). A credencial
registrada no momento é literalmente a string bruta do Bearer PAT extraída
do header `Authorization: Bearer <PAT>` da requisição de acquire — não um hash,
não um token derivado —, de modo que o hugit techlead pode confirmar diretamente
se o seu assinante do envelope detém exatamente esse token. Se a arquitetura do
hugit separa os papéis — o orquestrador adquire com PAT-A, mas o serviço
consumidor do envelope assina com PAT-B — cada poll falha no gate de credencial
com 503. Precisamos da confirmação do hugit techlead sobre o modelo de identidade
do assinante antes de M1.

---

## A pergunta precisa

**P1.** O assinante do envelope (o `hugit-ledger::envelope` producer, §13.2)
apresenta o **mesmo PAT** que adquiriu o lease — ou um serviço/papel diferente
faz o subscribe com uma credencial distinta?

**P2.** Se a resposta a P1 for "credencial diferente": que credencial o assinante
já possui ou deriva, sem que o corelink precise retorná-la no `AcquireResponse`
(tipo congelado — não pode carregar um campo novo sem uma emenda de contrato)?

---

## Tabela de opções

| Opção | Mecanismo | Mudança de contrato | Prós | Contras |
|---|---|---|---|---|
| **A — mesmo PAT** (default atual) | O hook é registrado com o PAT do acquire. O assinante precisa apresentar o mesmo PAT. | Nenhuma — `AcquireResponse` congelado intacto. | Zero atrito; funciona se hugit usa o tenant PAT nas duas pontas. | Falha silenciosa (503) se hugit separa papéis. |
| **B — credencial de envelope por tenant, fora de banda** | CoreLink emite/mantém uma credencial de envelope por tenant (não por lease). Provisioned out-of-band via painel/API CoreLink. O assinante a conhece a priori. | Nenhuma no wire de lease. Nova API CoreLink de credencial. | Separação limpa de papéis; sem mudança nos tipos de contrato. | Requer API nova no CoreLink + provisionamento no lado hugit. |
| **C — campo de credencial no `AcquireResponse`** | O corelink gera uma credencial por lease e a retorna no acquire. | **Emenda de tipo congelado** — `AcquireResponse` e vetor de conformidade hugit precisam de versão nova; PR nos dois repos. | Credencial por lease; isolamento máximo. | Heavyweight: atualização do vetor de conformidade nos dois repos (processo de emenda §13.4), owner + hugit techlead devem ratificar. |

**Recomendação corelink-runners:** opção **A** se o hugit-ledger::envelope
producer assina com o tenant PAT; caso contrário, opção **B** (sem toque nos
tipos congelados).

---

## O que foi feito para manter seguro enquanto a decisão está aberta

- O PR #24 (`feat/envelope-wire`) foi **mergeado** — o código está em `main` com
  a suposição **flaggeada em código** no site de registro
  (`crates/corelink-fabric-server/src/handlers/leases.rs`, comentário no bloco
  de composição do `CaptureHook`): qualquer revisor vê o assumption explícito.
- O caminho de subscribe é **fail-closed**: credencial errada → 503, nunca
  drenagem silenciosa para outro tenant. O gate de ownership do tenant (404 sem
  oracle de existência) é independente e está sempre ativo.
- O vetor de conformidade `IntentMetrics` (§13.4) **não foi tocado** — permanece
  owner/hugit-gated (ver PR #5; nunca adicionado unilateralmente).

---

## Reversibilidade

Mudar a credencial registrada é **uma linha** na raiz de composição do acquire.
Não há impacto nos tipos de wire, nos vetores de conformidade, nem nos outros
gates do envelope. A decisão não bloqueia M0; bloqueia M1 (wiring de produção).

---

## ACTION REQUESTED

> **hugit techlead:** confirmar o modelo de identidade do assinante (P1 acima) e,
> se opção B ou C, indicar a credencial ou aprovar a emenda de contrato.
> Roteie a resposta via owner. A decisão fecha o seam e permite o wiring de
> produção do §13.2 para M1.
