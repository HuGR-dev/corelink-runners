# Sprint 1 / B1 — matriz operacional de aceites

**Data:** 2026-09-06  
**Escopo:** os 12 WPs da Sprint 1  
**Fonte de estado:** `docs/plan/delivery-ledger.json` (snapshot de
`/private/tmp/corelink-journal-worm`, commit `f64a72d489dc65e7b96ac12242497d64e0c3f924`)  
**Registro canônico:** `docs/plan/2026-09-01-reconciled-dispatch-dag.md`

Esta matriz é um despacho de aceites. `acceptance-ready` significa que a
implementação e os checks focados registrados estão prontos para o aceite
correspondente; não significa que o aceite live já passou. `blocked` significa
que o próximo passo depende de um contrato, autoridade ou decisão externa
concreta. Nenhum teste pesado foi executado para produzir este documento.

## Estado executivo

| Estado | Quantidade | WPs |
|---|---:|---|
| `acceptance-ready` | 8 | T3-W18, T8-W4b, T6-W4, T6-W9, T4-W4, T3-W10, T1-W5, T6-W2 |
| `blocked` | 4 | T6-W13, T6-W15, T2-W3, T9-W1 |

O ledger marca dez itens como `implementation: complete`, mas isso não basta
para autorizar aceite. O routing de fontes encontrou oito WPs integrados nos
ancestrais do head; T6-W13, T2-W3 e T9-W1 ficaram
`ambiguous_missing_inventory`. Essas três linhas estão explicitamente
bloqueadas até o inventário provar a presença do source/evidence correto.
Nenhuma delas deve ser contada como entregue ou reaberta como código sem uma
diferença concreta. Para os oito `acceptance-ready`, o trabalho restante é
aceitação, configuração, publicação/pinagem ou evidência live explicitamente
faltante.

## Matriz operacional

| WP | Status real | SHA / fonte registrada | Lacuna concreta para aceite | Check focado mínimo para dispatch | Dono recomendado |
|---|---|---|---|---|---|
| **T3-W18** | `acceptance-ready` | `bb165bad54125460ebae213e405df3a75baa6566`; `2d2930ac7fb0ebdcc5c76291fa12f94abb586dae` · `T3-W18-containment-live.json`, `2026-09-06-sprint1-source-closure.json` | Executar matriz live version-bound de três estados **10/10**, resume ordenado não vazio, e observar ausência de efeitos indevidos e instâncias em execução. O artefato atual continua RED nesses pontos. | Conferir versão/config digest do alvo; rodar somente o driver 10/10 + resume não vazio; arquivar respostas, efeitos e contagem de instâncias. | Sol / live-risk |
| **T8-W4b** | `acceptance-ready` | `bb165bad54125460ebae213e405df3a75baa6566`; `2026-09-05-auth-bridge-review.json` | Validar a integração final no sprint completo e, depois, evidência de processo/imagem implantados. Reabrir código somente se o bridge do DevEnv mudar. | Smoke focado do auth-file bridge nas superfícies Rust, boot, Worker e DevEnv; confirmar que a imagem/processo usado no aceite contém o SHA aprovado. | Luna / security |
| **T6-W4** | `acceptance-ready` | `bb165bad54125460ebae213e405df3a75baa6566`; `f77e4d03fa0db053522bd6571a9ef5286099a130`; `f8270299f581e9eefa7274bfcf1a0304acb2a8f4`; `e1501f6246fba66d6deaa9aad9ddd4be0be71586` · envelope, source-review, stress e secret-scan evidence | Falta apenas o aceite de composição: config do owner e evidência live/named-host; o produtor e seus checks focados já estão aprovados. | Exercitar uma sequência capacity-1 enqueue → ACK/terminal, incluindo retry/crash boundary e flag inválida; confirmar o envelope de evidência e o host nomeado. | Sol / live-risk |
| **T6-W9** | `acceptance-ready` | `bb165bad54125460ebae213e405df3a75baa6566`; `04743acf9a7a7f2385ccce1ebd13882530b7d4f1`; `47ae1bc370c6a68f79aa126e203ecf661b59415a` · `2026-09-05-rule-matrix-and-billing-review.json` | Faltam binding externo real, detector implantado e aceite end-to-end C1–C5; não há crédito live registrado. | Validar a tabela C1–C5 em teste focado (positivo, malformed/auth e resposta drenada) e confirmar o binding/detector version-bound antes do probe live. | Sol / alerting |
| **T6-W13** | `blocked` (`ambiguous_missing_inventory`) | Ledger lista `bb165bad54125460ebae213e405df3a75baa6566` e `2d2930ac7fb0ebdcc5c76291fa12f94abb586dae`; routing não comprovou esses artefatos no ancestral do head · `2026-09-06-sprint1-source-closure.json` não é suficiente sozinho | Primeiro provar que o source, testes e evidence do WP estão no tip de integração. Só então faltará a evidência implantada dos ciclos metrics-key current/stale, página e ACK autenticado. | Check de inventário: `git merge-base --is-ancestor` para cada SHA aplicável, listar paths canônicos do DAG e verificar evidence file. Sem esse resultado, não rodar aceite nem contar entrega. | Sol / alerting |
| **T6-W15** | `blocked` | Sem SHA de implementação; contratos citados em `2026-09-06-techlead-takeover.md` e `2026-09-06-f008-credential-lifecycle-integration.md` | O contrato `O-MONITORHOST` ainda não está qualificado: trusted time/TSA, WORM, independência de três contas, crash/rotação/isolamento e sete dias reais de observação. O desenho AWS Option B aguarda qualificação; não há aceite nem deployment. | Check de contrato, não de código: emitir a capability artifact com runtime, storage, scheduler, credential, trusted-time e receipt/cursor; rejeitar qualquer estado UNKNOWN como sucesso. | Sol / alerting + owner do monitorhost |
| **T4-W4** | `acceptance-ready` | `bb165bad54125460ebae213e405df3a75baa6566`; `2026-09-05-admission-review.json` | Implementação de admission está aprovada. Resta incluir sem alteração na validação da Sprint 1 e cumprir a configuração/deployment acceptance aplicável. | Rodar os vetores de boundary admission/capacity já definidos e conferir que o tip composto preserva o SHA revisado. | Sol / architecture |
| **T3-W10** | `acceptance-ready` | `bb165bad54125460ebae213e405df3a75baa6566`; `7142f239effd7d54fd17962d8327ac1ab804a345`; `c2fd9bff2550b51dcaf8e2fbd99c5b44a4390d3e`; `dd7ea7f80980d453c57bca9f6f8fa5afe6149fb5`; `5b44b72c3bdbe4964f8d1fc63f0eef591494446d`; `9fe16556b992175e0164ccfa69a743b392ef2322`; `dbe9b4340c0512cd80637e0388d42922fe55b62e`; `a6d04957d1b48b0f9a74e9a5fef4f13e7886d0ba` · held-cleanup, provider-binding e provider-recovery evidence | Código e metadados fechados. Resta aceite runtime no tip completo; a descoberta de identidades unbound/historical fica registrada separadamente como `F-20260905-005`. | Reexecutar somente o cenário focado de teardown/reaper e conferir a classificação de identidade; não ampliar o escopo para novo design. | Sol / architecture |
| **T1-W5** | `acceptance-ready` | `f43898914dab1ed65c37bbac196b394aa7acaa89`; `af0795ef4099a49d3b6650d301bdcf0203d13e60` · `2026-09-05-mint-readiness.json` | Código e fontes de teste integrados; faltam runtime/full-CI/live keyed acceptance no tip completo. | Smoke de startup/readiness com chave válida, ausente e incorreta; confirmar recusa fail-closed, ausência de segredo no log e digest version-bound. | Sol / architecture |
| **T6-W2** | `acceptance-ready` | `bb165bad54125460ebae213e405df3a75baa6566`; `7489786a35e402732c58a74c4d005486ed3e4aba`; `4d4d4f2dfec88355cee51ff5c2a0dadf0adb011e`; `8260f79d6a74062e10bbcd53bf289ad2fb1fb781`; `2d2930ac7fb0ebdcc5c76291fa12f94abb586dae` · `ADR-0011`, `2026-09-06-sprint1-source-closure.json` | Publicar `clw` assinado em 0.1.12, fixar consumidores imutáveis e provar HIT/restore autenticado real. A recusa 78 existente não é prova de HIT. | Verificar pin/digest do consumidor e executar um único caso autenticado HIT → restore sem execução do child; manter o caso refusal separado. | Sol / contract |
| **T2-W3** | `blocked` (`ambiguous_missing_inventory`) | Ledger lista `2171dd91034e46d18cce6d6e50624a6d44879858`; routing não comprovou a integração no ancestral do head · `2026-09-05-action-pins.json` é fonte declarada, não prova de presença no tip | Provar integração do conjunto de workflows e do evidence file no tip correto; só depois fazer o inventário final de pins. Não presumir que a lista do ledger representa conteúdo entregue. | Check de inventário: testar ancestralidade do SHA, enumerar `.github/workflows/*.yml`, confirmar paths e comparar pins contra o manifesto. Sem isso, nenhum aceite. | Luna / CI |
| **T9-W1** | `blocked` (`ambiguous_missing_inventory`) | Implementação parcial listada no ledger: `f1836f94b4dcdc840bd756738589d5fe85b1b478`; `9b87d3b5e588fe19357741fb76c270bad3fe7437`; `2158fbc9ea122ddedbc5ddbe025aba3d4fc121b8`; `64509e86133207a9a187f2036cad82d11c75875a`; `4a0c3c3bd3ca3bdbc8b38a34e8adac59ac12639b`; `dc336b89a034d9961d8cd6a6c5b239edbbc494f9`; `33d808240cfe3126bd3c42284ae7dacc2ddb00e4`; `de74cc32fe2b2d425f22b701785dfea2e3dafa5b`; `241950da28cffebad6813680b74787f7881440b4` · routing não confirmou inventário no ancestral do head; devenv billing/credentials, shared-compute verification e takeover handoff | Primeiro provar quais blobs/path/evidence integram o tip. Depois ainda falta fechar D2: autoridade qualificada de terminal/cancelamento e uso real F005/F007, hard-stop/provider producers, baseline externo reconciliado, chaves públicas de produção, migração pareada e aceite live. A reserva configurada de 8h não prova hard-stop. | Check de inventário/contrato: ancestralidade dos SHAs e paths canônicos; registrar budget shared vs separate, autoridade terminal e baseline/key provisioning; só depois regressão compute-budget (bearer válido e 503 sem settlement). | Sol / decision-gated + owner do contrato server |

## Ordem de dispatch

1. Disparar em paralelo T4-W4, T3-W10, T1-W5 e T6-W2; eles têm checks
   focados independentes, respeitando os predecessores registrados. T2-W3,
   T6-W13 e T9-W1 ficam fora até o inventário ambíguo ser resolvido.
2. Fechar T3-W18 primeiro no lane live-risk. Com esse aceite, disparar T8-W4b
   e T6-W4 em paralelo.
3. Depois de T6-W4, executar T6-W9. T6-W13 só entra após o check de
   inventário provar a presença do source/evidence correto. Essas linhas exigem
   evidência externa, mas não exigem reabrir o código aprovado.
4. Manter T6-W15, T2-W3, T6-W13 e T9-W1 fora da fila de aceite até seus
   contratos/inventários serem materialmente qualificados. Um `prepared`,
   `partial` ou SHA listado sem ancestralidade não é promoção.

O critério de fechamento da matriz é finito: cada linha termina em `ACCEPT`,
`FIX` ou `BLOCKED-CONTRACT`, com artefato e SHA do tip observado. Não contar
commits, testes históricos ou configuração sem prova live como aceite.
