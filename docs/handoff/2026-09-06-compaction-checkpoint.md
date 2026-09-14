# Checkpoint de retomada — Corelink

Data: 2026-09-06. Este arquivo é ESTADO DE RETOMADA, não um novo plano. Pedido atual do usuário: "Ok salva o estado pra compact".

## Atualização de checkpoint — 2026-09-08 (prevalece)

- Estado Git recuperado após o crash: B2 está limpo em `/private/tmp/corelink-b2-integration-20260907`, branch `bundle/b2-sprint2-closeout-20260906`, tip `20cc115d5ffbbd89d6c5e596da53f7bbd3e81d1e`. O ledger mantém B2 em 11/14 implementações formais; não há novo PR, aceite ou merge de B2.
- Entrega efetiva: 1/3 merges de bundles, correspondente ao B1/#559 em `0cc2d82373b9786450414a77dba656ea43e015a0`. B2 ainda não foi entregue/mesclado; B3 permanece em 0/28. Commits, agentes, testes e worktrees não são crédito de WP ou merge.
- GitHub é o provedor ativo do fluxo CoreLink (GitHub App, `workflow_job.completed` e integrações aplicáveis). Githugr e Hugit são projetos externos descontinuados para este produto: não são dependências, consumidores, autoridades, ambientes, verificadores, gates ou bloqueios de CoreLink. Histórico permanece preservado; não confundir Githugr com GitHub. Ver `docs/adr/0014-corelink-standalone-product-boundary.md`.
- CI pesada local disponível: B1 passou 12/12; a linha B2 registrou 1119/1119 e o candidato posterior de admission-freeze 1123/1123, além de testes focados e typechecks. GitHub Actions não é o caminho operacional nesta campanha por causa do billing; isso não equivale a aprovação de B2.
- Evidência operacional de quiescência anterior: Worker `1418af47-d71a-488f-89a6-cbb9173402bd` a 100%, Fabricd no digest `sha256:2e7bcea926f4ce2b38edb1a381f3821fcf4c898377e4f988b763fd3232c0e565`, DevEnv no digest `sha256:d105e11f92718d610b390a71c37acb9bfb668da278b2f80c1f617f7cd068768c`, fleet `busy=0 checked=0 unverifiable=0`, intake/redrive pausados `1/1`. É evidência histórica verificável, não promoção nem substituto para T2-W2b.
- T2-W2b continua parcial: falta evidência version-bound de 10/10 cold starts; falhas anteriores e recuperação estão preservadas nos artefatos `docs/plan/evidence/T2-W2b-A2.7-final-20260908.json`, `T2-W2b-incident-recovery-20260908.json` e `T2-W2b-deploy.json`. T2-W4 ainda retém A2.8/A2.9 conforme o ledger; T2-W6 aguarda AU1.8 real e rotação de secrets.
- Incidente interno de exposição de secrets: a rotação forward-only v2 e o controller estão preparados, mas ainda pendentes. Não registrar valores de secrets nem executar rotação/freeze live antes de manifesto não secreto do `corelink-fabricd`/Cloudflare, inventário das instâncias CoreLink ativas, provas Corelink-native e revalidação operacional. Ausência de Hugit/Githugr não é pré-requisito.
- Restrição de ambiente: host/disco já esteve quase cheio; somente caches recriáveis foram limpos e worktrees, evidências, secrets e o checkout sujo foram preservados. Não apagar/prunar worktrees ou dados sem destino comprovado.
- Preservar exatamente o arquivo não rastreado AU1.8 `scripts/ops/au1.8-fabricd-cred-ticket-rotation.sh`; ele não pertence a commits B2 até revisão/integração própria.

## Primeira ação após compactação

1. Ler o plano único completo: `/Users/gustavoschneiter/.codex/plans/corelink-wp-closeout-20260906.md`.
2. Ler este checkpoint e o ledger canônico; conferir o retorno do integrador `runner_final_composition` para a sincronização mais recente e o inventário Git.
3. Retomar a Sprint 2 no candidato B2 `20cc115`, sem reiniciar auditorias ou recontar trabalho; concluir somente os 3 WPs formais restantes antes do gate da sprint.
4. Root precisa resolver sua pendência real de arquitetura dos serviços restantes do T6-W15. O executor está esperando contratos, não falta de autorização para corrigir código. Finalizar decisões e fornecer entrada concreta ao MESMO executor; evitar mais ciclos de planejamento genérico.

## Mandato mais recente do usuário (prevalece)

- Root = orquestrador, product owner, tech lead e representante do stakeholder. Não escreve/revisa produto pessoalmente; agentes executam código/revisão, root decide e coordena.
- Luna padrão; Terra somente necessário. Até 15 agentes ativos, máximo paralelismo assíncrono útil DENTRO da sprint ativa, paths sem conflito e um dono por WP até aceite.
- EXATAMENTE 3 SPRINTS SERIAIS, uma de cada vez: S1=12 WPs, S2=14, S3=28. A antiga divisão em quatro é apenas histórica. Nenhum trabalho da sprint seguinte antes do merge/fechamento da atual; código já existente futuro fica preservado.
- Cada sprint = UM stacked PR/bundle e UM merge na main Runner. Exatamente três merges de bundles. Não mesclar individualmente a pilha antiga #548–#558.
- CI completa/testes pesados SÓ no candidato completo da sprint. Verificações pequenas necessárias permitidas durante implementação. Não repetir suites aprovadas inalteradas.
- CI com N falhas: agrupar por causa/WP/escopo, paralelizar fixes disjuntos, serializar arquivos conflitantes, integrar e repetir gates afetados/exigidos no novo SHA. Mesma sprint fica ativa.
- TODO fix cobre causa raiz/por que ocorreu, ramificações, consequências, correção completa e mínima, prevenção proporcional e evidência. Evitar overengineering, abstrações hipotéticas, burocracia e auditoria sem fim.
- Reviews/audits/cold reviews têm mandato finito: SHA, objetivo, escopo, riscos, critérios, evidências, começo/meio/fim e promoção PROMOVER/CORRIGIR/INCONCLUSIVO. Parecer consolidado; re-review dos fixes/regressões, não reinício geral. Segunda submissão reprovada exige diagnóstico da causa por root.
- Plano deve nortear root, com âncoras A0–A6 e checklists C0–C8. Lembrete em todo checklist: PARALELIZAR, INTERROMPER LOOPS, AVANÇAR PARA O OBJETIVO.
- Organização do repo significa PRs penduradas, commits não incorporados, branches e worktrees sem destino. NÃO reorganizar diretórios/refatorar arquitetura. Preservar trabalho antes de limpar.
- Autonomia já dada; não criar confirmações rotineiras. Email fornecido não autoriza envio de mensagens: `gustavomalleths@gmail.com`.

## Âncoras e estado persistente

Plano pessoal: `/Users/gustavoschneiter/.codex/plans/corelink-wp-closeout-20260906.md`.
Espelho canônico a sincronizar pelo integrador: `docs/plan/execution/2026-09-06-closeout-three-bundles.md`.
Ledger: `docs/plan/delivery-ledger.json`.
Handoff principal: `docs/handoff/2026-09-06-techlead-takeover.md`.
Contratos: canonical DAG `docs/plan/2026-09-01-reconciled-dispatch-dag.md`, round3 delta, remediation plan; detalhes monitor em execution/2026-09-06-monitor-wave2-contract.md, monitor-runtime-contract.md, token contract.
Skill TechLead relida: `/Users/gustavoschneiter/Documents/HuGR/techlead/skills/techlead/SKILL.md`; loop relida. Plan skill também lida. Não atribuir limitação de ferramentas inexistentes a automação armada.

Plano pessoal hash SHA256 no checkpoint: 06307751902236105e09bd17f84915fc0cd7ebb97860fe61124b1cd9914d2477.
Integração B2 observada após recuperação: `20cc115d5ffbbd89d6c5e596da53f7bbd3e81d1e`, worktree limpa.
O espelho canônico e o inventário Git continuam referências de coordenação; conferir o SHA efetivamente integrado antes de qualquer promoção.

## Repositórios, branches e realidade do progresso

Runner integração: `/private/tmp/corelink-techlead-takeover-20260906`, branch `delivery/techlead-takeover-20260906`, limpa na conferência.
Server integração: `/private/tmp/corelink-server-budget-20260906`, último HEAD qualificado conhecido `32f0ae4819c95b93e78dd89ff6b06c937dcd6389`.
Main REMOTA Runner verificada via gh API: `cda90940f74735c006693d9b9e3be85c37b26f1a`. main LOCAL `387c1b1cfa55f11ed1d219e5d671cc50a292d47d` está desatualizada; não usar como remoto.
Checkout principal usuário `/Users/gustavoschneiter/Documents/HuGR/corelink-runners`: branch `pr-0c-d4-openrouter`, behind2, conflitos UU index.ts/metrics.ts e DU containment-intake.test.ts sob deploy/cloudflare. Untracked incluem .atlas/.vite/node_modules e handoffs; não resetar/descartar.
265 worktrees registrados. Não presumir todos da campanha/descartáveis. Não apagar worktree dirty ou dependência fonte ainda usada.
Os bundles operacionais são `bundle/b1-sprint1-closeout-20260906`, `bundle/b2-sprint2-closeout-20260906` e `bundle/b3-sprint3-closeout-20260906`. B1 já tem o único merge confirmado desta campanha; B2 está no tip limpo `20cc115` e B3 permanece sem merge.
PRs abertos #548–#558: 11, nove drafts; todos heads ancestrais da integração, nenhum inclui todas correções posteriores. 548/549 T3-W18, 550/551/552 T8-W4b,553 T6-W4,554 T6-W9,555 T4-W4,556 T3-W10,557 T6-W2,558cleanup misto. Não mesclar ou fechar individualmente sem reconciliação.

Ledger efetivo já migrou para três sprints operacionais: scopes12/14/28; historical_sprint e historical_sprint_scope preservados. Checker/scripts/tests foram ajustados, testes mecânicos10/10 e ledgercheckPASS. Isso é tooling/registro, não CI pesada de produto.
Contagem: 16 registros históricos de entrega +16 OUTROS WPs implementation complete +6 partial +32 unknown backlog =70. Os16 históricos não foram recertificados agora; os16 completos não são todos novos desta sessão. Entrega efetiva da campanha: 1/3 merges de bundles; B2 11/14 formais e ainda não entregue; B3 0/28. S1 está mesclada em `0cc2d823...`; não transformar estado de implementação em crédito de merge.

## Pacotes atuais e evidência — não repetir trabalho

T6-W15 executor único = `r2_pair_acceptance` (Luna), worktree `/private/tmp/corelink-t6-w15-owner-20260906`. Dono registrado no ledger. Root é orquestrador; integrador global `runner_final_composition` (Terra); cold critic `spawn_preparation_integration` (Terra).

Já integrados e aceitos na entrega Runner: state, SNS adapter, S3 immutable journal, ACK crypto, incidents, trusted RFC3161 time/floor, audit log, outbox, witness+fresh head, types/config, lifecycle, scheduler, AWS secret/witness adapters, paired Canary schedule wire/HistoricalTerminal.
INGEST final `aaa68154d16524a2973eb80edbe43a3a47a892fc` JÁ INTEGRADO. Terra comprovou25/25DISTINTOS (23 primeirorun +2 credentialisolation recuperadosEXATOS do613ad) +typecheck. Produção stack9owncommits f0bd→aaa aplicado92837d5→c2b43ee. 4a73test-only jápresente comoe27276f; não duplicar. Fixes incluem originalACK integrity/currenttrust, pendingbeforeaudit, freshtimeafterintent, quarantineincidentoutbox, futureUNKNOWN, lifecycle/transitionclassification, lateimmutableAPPLIEDproducer_late e healthyfirsttickzeroalerta. Não contar apenas18/7authorclaimsemchecararquivos.
Três codecs JÁ INTEGRADOS após cold review R2: manifest7a400e14ad03024587634d8a60eb0157c314c024, recoveryd735ff3bbda8d6bd70396f7e7e6f110d52a99d99, page7e1bf113cd9f8b17e4a108c67869d503ba2c68e8 (integrationffaea8c→de13d45→c600bd4). Focados9/9+tsc. Codecs não são os serviços de autorização.

Witness runtime+infra: R2 corrigiu fonte existente no ownercheckout; reviewer independente `registry_contract` acaba de PROMOVER C1..C5 todosPASS, runtime4/4+tsc+pyinfraPASS. Fonte stack runtime `f50c9e6b5d5b843ae894693e2a65e89857be0420` → `3af3c67576f3d773e4c642fa2675c4f7ae333945` → `4ada190fc6bf8d5a1e3c05cc43bdcaaee214d912` → fixR2 `cc9fa84c09cc4f654549db97403606620f719cc3`. Infra `703c9701e2c562f18499dfb2fc40a09cb2aa82ed` → fix `eacd95a5337dd065181e30dffd87129a60d23f55`. Fixcontext exigeverifierAccountId, não monitor; IAMprefix exige/ final e não abrangejournal-other. Aprovaçãofontepermiteintegrar, não live. Integrador foi avisado noúltimofollowup; incorporação AINDA NÃO CONFIRMADA. Não reabrir codecs/journal/monitorinteiro.

## Pendência central de ROOT — não esconder no executor

Ainda faltam registry durável de signatários, serviço ACK_RECOVERY originalCAS+revocation/currentmanifest, page ACK humano autenticado+schedule+replayprev, e runtime principal. ROOT NÃO CONGELOU contratos de serviços completos. Executor aguardava essas decisões; não mandar simplesmente "implemente tudo/descubra interface" nem só criar mais codecs.
Contratos wire exatos existem no round3 e token contract: ACK14campossigned+sig, Recovery20+sig, Manifest19+sig, PageACK15+sig. AsyncSigner/PublicSigningIdentity já reais; epochsSTRING. Registry deve verificar persistência/highwater e fresh nonce witness contra rollback; nenhum callbacktrue é autorização produtiva. AuditReceipt não contémpayload: AuditLog.verify + ImmutableJournal.read(receipt.journalReceipt) permite conferircanonicalJSON(record.payload) com payload esperado. Root precisa definir fluxo de preseal/manifest/roles/custody semciclos antesauthordispatch.
Runtimeprodutos faltantes não são mera integraçãofinal; são implementação real. Só rotinasjácontratadas podem andar enquanto decisãofecha.

## Correção importante de planejamento — sete dias NÃO bloqueiam T6-W15 base

Auditoria bounded `admission_scope_contract` confirmou: ledger carregava indevidamente7days na base. Já corrigido pelointegrador. T6-W15 exige suasfontesbase/trustedtime/WORM/isolamento/crash/rotation e probe A6.10 3/3. Setedias A6.17 são posteriores: T6-W12 implementa, T6-W10 coleta, bloqueiamgatefinal/go-live. Fonteplan766/DAG600 e round3432/652–654. Não criar ciclo T6W15→T6W12→T6W15. Não é waiver; janela real obrigatóriaAINDA NÃO COMEÇOU.

## Bloqueios externos/de produto preservados

T9-W1 é segundo WP parcialS1. D2 é decisão/waiver separado; monitorwaivernãocobre. F005/F007 faltam autoridadequalificadaterminal/cancel/no-futurematerialization/usagehardstop. Timer/destroy/accountbillingaggregate não são prova. Root precisa escolhercaminhocorreto e sóentãopacoteexecutorúnico; nãocódigoespeculativo/silentdefer.
Legacycredentialsinventory privado:137ativosunknown+1invalidmetadata. Não imprimir/commitaridentidades nem revogarindiscriminadamente; rever açãoqualificada antes.
clwSigned0.1.12/5targets/3consumerpins/realHIT: release não publicado.
PG/rearm/live3x75s/7days/providerproofs não concluídos.

Monitor waiver APROVADO e limitado: ledger observedoperations emvezproviderexhaustivecursor, observedrecordintegrity semprova deSNSsemomissões, at-least-once duplicatespossible. Ainda exigeWORM/independência/trustedtimeUNKNOWNfailclosed. Não re-perguntar; não aplicar aT9/PAT/PG/live/7days. AWSread-onlycapabilitiesjáverificadas; nenhumaAWSprovision/deploy/SNSPublish/email nestaexecução.
AWSaccountsmonitor975306274105, sensitivity286590629898, verifier888348805607; rolesIAMindependentes/liveoperationalaindaqualificar. RealTSAprobeDigiCertfeitoeRSA/OpenSSLfixturesaprovadas; não refazerporrotina.

## Trabalho em andamento na hora do checkpoint

Último followup `runner_final_composition`: mandato GIT-CLOSEOUT-01 foi concluído no inventário de metadados; a retomada deve trabalhar a partir de B2 `20cc115`, sem reiniciar coleta. O inventário cobre PRs/branches/worktrees/commits conhecidos e ainda possui itens sem destino; resolver por grupos delimitados, preservando material sem prova de redundância. Githugr/Hugit não entram como destino, gate ou bloqueio.
Demais agentes na maioria completed em tarefas de componente/review; isso NÃO equivale a WPdelivered. Não descrever15agentesativossemconferir. Não abrirnovafrenteparasairdeblockedroot.

## Regras de ferramentas/ambiente

Usuárioaprovação/autonomia persistente. Ferramentas permissions nunca: NÃO usar sandbox_permissions. send_message nãoacordaagentcompleted; followup_task sim. Modeloverrides sóLuna/Terraseuser; agentes existentes continuam. SemmensagensSlack/emailsem autorização explícita. Rootnão criaGoaltoolinferidodepedidoordinário.
Hostmuito sobrecarregado porAtlas/OpenCode/QEMUdeoutrosprojetos, discoquasecheio. Não matarprocessosexternos. Agora pesadosSÓgatessprint; não repetirRSA/OpenSSL/fullmonitor porrotina. Usar depsreaisintegracao/deploy/cost-monitor/node_modules, não declarationsfake/symlinksquebrados; não limpar worktrees servindofontesdeps. SemDockerdaemonbuild/pushfeito.

## Próximas ações após ler plano

1. Receber integrador: validarplanohash, inventárioGit, runtime+infra integração e refs3bundles.
2. ResolvercontratosfaltantesrootT6W15 e caminhoT9, usando evidênciasexistentes; fornecerpacotesclaroscomaceite aoexecutorWP, nãooutraredecomponentes.
3. Dentro da Sprint 2 ativa, concluir os 3 WPs formais restantes com dono e escopo sem conflito; manter o trabalho futuro preservado e não iniciar B3.
4. Compor um único bundle B2, executar o gate pesado local uma vez, corrigir por causa-raiz em paralelo, aceitar e então fazer o merge 2/3. Só depois iniciar B3. Não marcar WP/sprint como entregue antes dos gates.

## Atualização final recebida durante a gravação — prevalece sobre o snapshot anterior

Integrador concluiu GIT-CLOSEOUT-01 em `30aa091c8f903002429e633f3bb9545675584e6c`. Plano CANÔNICO agora confirmado idêntico ao pessoal, SHA256 `06307751902236105e09bd17f84915fc0cd7ebb97860fe61124b1cd9914d2477`. Não repetir sincronização/inventário já feitos.
Inventário existe em `docs/plan/execution/2026-09-06-git-closeout-inventory.json`: 11 PRs, 265 refs, 265 worktrees e 6 fontes conhecidas. **344 itens ainda estão origin_destination_to_resolve**: inventariar não é organizar/limpar concluído. Root deve definir roteamento delimitado por itens/grupos com base nesse arquivo, sem reiniciar coleta ou apagar desconhecidos.
INGEST/codecs classificados substituted_proven; runtime `cc9fa84` e infra `eacd95` APPROVED_TO_INTEGRATE ainda aguardam incorporação em B1/T6-W15. C1–C5 já aprovados pelo reviewer; não repetir cold review por rotina.
Primary conflitado foi preservado; não houve limpeza destrutiva, push, close de PR ou merge durante esta recuperação. O estado confirmado é 1/3 merges (B1), B2 no tip limpo `20cc115` e B3 sem merge. Integrador terminou a tarefa de inventário; não repetir esse ciclo.
