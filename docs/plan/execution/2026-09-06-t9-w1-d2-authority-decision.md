# T9-W1 / D2 — decisão de autoridade de compute e billing

**Data:** 2026-09-06  
**Base auditada:** `d718b412acac1f3d23f1a6482c8d474b0e383827`  
**Escopo:** decisão arquitetural bounded; nenhum código de produto é alterado por este registro.

## Decisão

D2 fica resolvido como **rectify now**. Não há waiver de quarentena. O DevEnv permanece
fail-closed enquanto os contratos abaixo não estiverem provados; essa recusa operacional é uma
barreira de segurança, não um waiver.

O waiver limitado do monitor não se aplica a D2, F-20260905-005 ou F-20260905-007. Ele não pode
autorizar criação, cancelamento, terminalidade, materialização futura, uso faturável ou hard stop.
Nenhum teste local, timer, `destroy()` bem-sucedido, soma mensal D1 ou estado do DO é autoridade de
provedor.

## Limite de autoridade

O Runner só pode consumir uma autoridade externa configurada em `FABRIC_COMPUTE_URL`. Essa URL
deve apontar para o serviço CoreLink que possui a reserva e o vínculo com o provedor; o Runner não
fala diretamente com a API do provedor e não transforma ausência de resposta em ausência de recurso.
O grant existente continua sendo a identidade da operação: `tenant_id`, `workload_kind=devenv`,
`workload_id`, `reservation_id`, `vcpu_count`, `maximum_wall_ms`, período e ceiling assinado.

O binding mínimo do serviço é:

```text
reserve(grant)  -> { reservation_id, state: prepared|active }
activate(id)    -> { reservation_id, state: active }
cancel(id)      -> { reservation_id, state: cancelled,
                      materialized: false, actual_vcpu_ms: "0", evidence_digest }
settle(id, use) -> { reservation_id, state: settled,
                      materialized: true, actual_vcpu_ms, evidence_digest }
```

Cada resposta deve ser autenticada pelo serviço de autoridade, referir exatamente o
`reservation_id` e o grant/geração que a originou, e ser durável antes de ser devolvida. A resposta
`cancelled` é uma fence: depois de aceita, qualquer `activate`/`start` da mesma reserva ou de uma
geração antiga é recusado pelo serviço e não pode criar um handle. Uma resposta ambígua, timeout,
401/429/5xx ou schema inválido mantém a obrigação durável e bloqueia novo start; nunca é convertida
em `cancelled`, `absent` ou `settled`.

`maximum_wall_ms` é deadline de recuperação e limpeza. Não é prova de hard stop. O hard stop é
exercido somente pela autoridade externa: `reserve`/`activate` recusam o consumo acima do ceiling
com o resultado tipado `over_compute`, antes de qualquer materialização. Não existe fallback local
que admita o start.

## Estados e invariantes

O journal local de `ComputeObligations` permanece a única memória de retry do Runner, com estes
fluxos permitidos:

```text
preparing -> active -> dispatched -> settling -> terminal(settled)
preparing|active -> abandoning -> terminal(cancelled|settled)
```

- `preparing|active -> cancelled` só é válido com receipt externo `cancelled`,
  `materialized=false`, uso zero e fence de não materialização futura.
- Depois de `dispatched`, a limpeza exige terminalidade do provedor e receipt de uso real;
  timeout de `destroy()` conserva a obrigação e não libera capacidade.
- `settled` exige `actual_vcpu_ms` produzido pela autoridade. O Runner não deriva uso de relógio,
  tier, conta mensal ou callback de lifecycle.
- O `idem_key` do evento de uso é derivado da sessão/reserva e o evento só é removido do outbox
  após ACK de ingestão; retries são at-least-once e idempotentes.
- A reserva, o estado do provedor, o receipt terminal e o evento de uso devem carregar a mesma
  reserva/geração. Um mismatch é recusa e retenção.

## O que já é verificável localmente

Os testes unitários podem provar apenas o comportamento do adaptador e do journal, sem alegar
autoridade real:

1. grant inválido, tenant/reserva/workload divergente, ceiling inválido e respostas fora do schema
   são recusados antes de efeito;
2. `cancel` confirmado produz `terminal(cancelled)`, uso zero e retry idempotente;
3. uma tentativa tardia de `activate`/`start` após `cancel` é recusada pelo fake authority e não
   produz handle, inclusive após recriar o DO;
4. timeout/erro ambíguo conserva a obrigação, não libera slot e agenda retry;
5. receipt `settled` com uso `actual_vcpu_ms` diferente da duração de parede gera o mesmo valor no
   evento, demonstrando que a fonte é o provedor;
6. `over_compute` falha antes de `start`, sem container, claim ou evento de uso;
7. o roteador HTTP continua recusando POST de start sem RPC autorizado, cobrindo A3.8.

Os checks já registrados em `docs/plan/execution/2026-09-05-devenv-billing.json` e
`2026-09-05-devenv-credentials.json` (focused tests e `tsc`) são evidência estrutural desses
limites, não prova de F-005/F-007 nem de billing em produção.

## Binding externo e aceitação final

O binding é configurável por `FABRIC_COMPUTE_URL`, credencial de serviço/grant e versão explícita
do contrato. Configurar a URL não equivale a provar o binding. Para fechar T9-W1, o executor deve
preservar receipts sem secrets e produzir:

- **Provider terminal/cancel:** uma execução real de start→stop/cancel com `reservation_id`,
  estado terminal, geração, `materialized` e receipt autenticado; uma execução de cancel antes do
  start seguida de tentativa tardia deve provar ausência de futura materialização.
- **Uso real:** o receipt de settlement deve trazer `actual_vcpu_ms` do provedor, com ingestão
  aceita e reconciliação do mesmo `idem_key`; relógio local e agregado mensal não contam.
- **Hard stop:** uma tentativa acima do ceiling deve receber `over_compute` da autoridade externa
  antes de materializar e sem evento faturável; indisponibilidade da autoridade deve recusar.
- **F-005/F-007:** restart, retry e concorrência devem conservar a mesma reserva/geração; ausência
  ou identidade ambígua nunca autoriza delete, release ou substituição.

O resultado é `PASS` somente quando os quatro artefatos acima estão ligados à versão/configuração
implantada. Até lá, T9-W1 permanece `partial`/RED no ledger, mesmo com todos os testes locais
verdes. Não há pergunta aberta: há gates objetivos de evidência, e a ausência de binding é uma
recusa determinística.
