# Corelink — contrato de orquestração: fechar todos os WPs em três merges

Data: 2026-09-06. Dono deste plano: root. Estado: EM EXECUÇÃO.
Este é o plano operacional que o orquestrador deve seguir, por determinação explícita do usuário. Substitui integralmente a versão operacional anterior. Espelho canônico no repositório: docs/plan/execution/2026-09-06-closeout-three-bundles.md; ambos devem conter a mesma versão, não planos concorrentes.

## Âncoras operacionais — onde consultar, quem mantém e quando atualizar

Âncora é um registro existente e verificável, não a memória da conversa. Não criar planilhas paralelas com estados diferentes. Os paths abaixo são relativos à raiz do repositório, exceto o espelho pessoal explicitamente indicado.

| ID | Fonte exata | Uso obrigatório | Responsável / atualização |
|---|---|---|---|
| A0 — Mandato | Mandato, método e adendos deste plano; instruções explícitas mais recentes do usuário prevalecem | Conferir três sprints seriais, três merges, paralelismo interno e limites antes de mudar a operação | Root registra toda mudança autorizada neste mesmo plano |
| A1 — Plano único | `docs/plan/execution/2026-09-06-closeout-three-bundles.md` | Ordem de execução, checklists e regras de promoção; primeira leitura em retomadas | Root decide; integrador sincroniza e commita. Espelho pessoal: `/Users/gustavoschneiter/.codex/plans/corelink-wp-closeout-20260906.md` |
| A2 — Estado e achados | `docs/plan/delivery-ledger.json` | Sprint ativa, 54 WPs restantes, dono, estágio, dependências, achados e próxima ação | Integrador atualiza em toda transição, a partir de evidência validada por root |
| A3 — Contratos e aceite | `docs/plan/2026-09-01-reconciled-dispatch-dag.md`, `docs/plan/2026-09-01-round3-remediation-delta.md`, `docs/plan/2026-08-30-golive-remediation-plan.md`; contratos específicos apontados pelo pacote do WP | Critérios originais e interfaces congeladas; consultar trechos pertinentes antes de executar/revisar | Root resolve ambiguidade e registra decisão; agente não inventa requisito nem muda o contrato unilateralmente |
| A4 — Código e merges | Git: baseline remoto, SHA do executor, SHA aprovado, SHA do bundle e SHA de merge; branches `bundle/b1-sprint1-closeout-20260906`, `bundle/b2-sprint2-closeout-20260906`, `bundle/b3-sprint3-closeout-20260906`; PRs efetivamente criados | Identificar o alvo exato e provar incorporação. Nome de branch não substitui SHA; PR planejado não é PR existente | Integrador verifica refs/remote e atualiza A2 em cada integração/merge |
| A5 — Evidências | Artefatos referenciados pelo WP no ledger em `docs/plan/evidence/` e `docs/plan/execution/`; checks do PR e artefatos de CI correspondentes | Sustentar PASS/FAIL, revisões, aceites e deployment/version binding | Executor produz, revisor confere, integrador vincula e preserva antes da entrega |
| A6 — Retomada | `docs/handoff/2026-09-06-techlead-takeover.md` | Resumo curto: sprint ativa, bundle/SHA/PR, agentes ativos, impedimentos, última ação concluída e próxima ação de root | Integrador atualiza em checkpoints/retomadas; contém ponteiros para A1–A5, não uma segunda verdade |

**Consistência:** A1 define o método; A2 registra estado; A3 define o comportamento contratado; A4/A5 provam o estado; A6 facilita a retomada. Handoff antigo não prevalece sobre Git/evidência atual. Diante de contradição, suspender somente a promoção dependente, conferir a fonte e corrigir o registro com motivo. O restante independente da sprint ativa continua. Uma lacuna de contrato sob responsabilidade de root precisa de decisão de root; não é transferida ao executor.

**Persistência:** o plano canônico e o espelho pessoal devem ter conteúdo idêntico após sincronização. Toda atualização termina com commit do canônico e registro do SHA no handoff. Divergência é resolvida pela última decisão explicitamente registrada, não pelo horário do arquivo. Logs em `/tmp` ou worktrees não são a única evidência de uma entrega: antes de apagar ou mesclar, preservar a prova necessária em local durável referenciado, sem secrets ou inventários privados no Git. Caminhos/IDs ausentes ficam como pendentes, jamais inventados.

## Checklists de condução — usar nas transições, não preencher por ritual

**Lembrete obrigatório de root em TODOS os checklists:**

- [ ] **PARALELIZAR:** há trabalho independente elegível na sprint ativa aguardando enquanto existe capacidade? Despachar/retomar de forma assíncrona, até 15 agentes, com um dono por WP e paths sem conflito. Não esperar um lote inteiro quando um resultado já permite a próxima ação. Não iniciar a sprint seguinte nem criar trabalho artificial para ocupar vagas.
- [ ] **INTERROMPER LOOPS:** estou repetindo revisão, comando, planejamento ou discussão sem nova evidência? Identificar a causa e tomar uma ação diferente que a resolva: fechar contrato, corrigir escopo/contexto, executar o fix ou remover o impedimento real. Reutilizar evidência válida; não voltar a auditar tudo. Segurança e critérios de aceite permanecem obrigatórios.
- [ ] **AVANÇAR PARA O OBJETIVO:** qual pendência concreta será encerrada nesta rodada e como aproxima o WP do aceite, a sprint de seu merge e o backlog de zero? Registrar resultado verificável e próxima ação com dono. Agente completed com WP pendente exige encaminhamento, não abandono. Quando existir bloqueio real, registrar resolução necessária e manter os trabalhos independentes elegíveis avançando.

Essas três perguntas são verificadas na retomada, em cada retorno de agente e antes de encerrar um ciclo de orquestração. Atividade, quantidade de agentes e quantidade de testes não substituem progresso verificável. O objetivo continua: concluir todos os WPs e qualificar o go-live em três sprints seriais e três merges.


Os itens abaixo são condições de passagem, não tarefas extras nem comprovantes de execução. Marcar no registro do WP/ciclo com evidência; deixar pendente ou justificar não aplicável. Não marcar esta lista global como se valesse para todos os WPs. Segurança obrigatória não admite não-aplicável sem fundamento no escopo.

### C0 — Retomar a sessão ou receber nova orientação

- [ ] Ler A1 e A6; confirmar em A2 a única sprint ativa e o próximo marco.
- [ ] Conferir agentes reais e Git do bundle; distinguir ativo, completed, bloqueado e ainda não integrado.
- [ ] Incorporar a orientação nova sem perder objetivo, aprovações, fontes e trabalho já concluído.
- [ ] Identificar o próximo bloqueio do caminho crítico e seu dono; resolver primeiro pendências de planejamento que são de root.
- [ ] Retomar a próxima ação autorizada; não recomeçar a campanha nem abrir outro plano.

### C1 — Abrir uma sprint

- [ ] A anterior tem aceite e merge confirmados; para S1, baseline remoto conferido.
- [ ] Escopo é exatamente S1=12, S2=14 ou S3=28 WPs, sem duplicação nem transferência silenciosa.
- [ ] Um bundle/stacked PR da sprint, base e responsável de integração definidos; PR inexistente marcado pendente.
- [ ] WPs elegíveis e donos definidos; nenhuma execução de outra sprint em paralelo.
- [ ] CI/testes pesados reservados para o candidato completo; capacidade de execução conhecida.

### C2 — Delegar ou retomar um WP

- [ ] Porta de entrada de seis itens deste plano completa: baseline, dono/paths, contratos, aceite, dependências e checks.
- [ ] Código e evidências existentes localizados; não pedir reimplementação ou nova auditoria do que já está válido.
- [ ] Um executor responsável até o fim; escopo sem conflito com outros agentes ativos.
- [ ] Critério de saída e retorno exigido explícitos; limites de testes/efeitos externos definidos.
- [ ] Dono e próxima ação registrados em A2 antes da execução; ao retomar completed, usar followup_task.

### C3 — Receber trabalho e promover por revisão

- [ ] SHA/base/paths e estado do checkout conferidos; nenhuma fonte de outro WP incluída sem vínculo.
- [ ] Todos os critérios do mandato cobertos ou explicitamente pendentes; PASS apoiado em A5.
- [ ] Fixes cobrem causa, ramificações, consequências e prevenção proporcional; solução sem generalização desnecessária.
- [ ] Parecer independente consolidado: PROMOVER, CORRIGIR ou INCONCLUSIVO, com condições objetivas.
- [ ] Achados impeditivos resolvidos para o estágio solicitado; revisão de fonte não declarada como aceite live.
- [ ] Se reprovado, correções voltam ao mesmo executor; segunda reprovação aciona diagnóstico de root.

### C4 — Incorporar ao bundle

- [ ] Aprovação corresponde ao SHA e contratos/dependências reais da composição.
- [ ] Conteúdo pertence à sprint ativa; não importar acidentalmente trabalho de S2/S3 para S1.
- [ ] Conflitos resolvidos sem descartar trabalho; diff final revisado no escopo afetado.
- [ ] Checks pequenos necessários aprovados; sem reexecutar bateria pesada ou evidência inalterada por hábito.
- [ ] A2 registra SHA incorporado e critérios ainda pendentes; worktrees/evidências só são limpos depois de preservados.

### C5 — Encerrar implementação e abrir o gate de CI da sprint

- [ ] Todos os WPs da sprint têm implementação completa, revisão e incorporação comprovadas.
- [ ] Nenhum stub temporário, declaração falsa, supressão de erro ou defeito impeditivo usado para produzir verde.
- [ ] PR do bundle tem escopo, SHAs e matriz de aceite atualizados; dependências externas concretamente verificadas.
- [ ] Disparar CI completa e testes pesados uma vez no candidato completo; armazenar resultados por job/SHA.
- [ ] Cada WP que dependa de operação tem seu cartão de execução versionado e sua fonte PROMOTE incorporada; o cartão não duplica um probe production por WP.
- [ ] A qualificação compartilhada de deploy/production da Sprint 1 roda uma vez no SHA do bundle, com runtime/configuração/versão corretos; ela permanece obrigatória para o merge B1.

### C6 — Tratar CI vermelha

- [ ] Consolidar N falhas por causa e escopo; identificar cascatas e falhas ambientais sem presumir sua causa.
- [ ] Criar pacotes de correção disjuntos dentro dos WPs existentes, cada um com reprodução e critério de saída.
- [ ] Paralelizar fixes independentes; serializar/unificar arquivos conflitantes e manter a mesma sprint ativa.
- [ ] Cada fix cobre causa raiz, ramificações, consequências e prevenção proporcional.
- [ ] Incorporar correções aprovadas; repetir jobs afetados e checks exigidos no novo SHA, sem N execuções da suíte inteira.

### C7 — Promover e mesclar a sprint

- [ ] Todos os critérios de entrega aplicáveis satisfeitos; nenhum bloqueio de segurança, integridade ou qualidade obrigatório aberto.
- [ ] CI/checks exigidos verdes no SHA final e o gate compartilhado de deploy/production da Sprint 1, quando aplicável, vinculado à versão correta.
- [ ] Diff e base do PR conferidos; ordem e compatibilidade Runner/Server demonstradas quando necessárias.
- [ ] Executar somente o merge B1, B2 ou B3 autorizado pelo plano; confirmar SHA na main remota.
- [ ] Registrar WPs entregues, provas e merge em A2; atualizar A6 e liberar somente então a próxima sprint.

### C8 — Conferir 100% e go-live

- [ ] Os 54 WPs remanescentes aparecem exatamente uma vez nos três bundles, todos com seus critérios fechados.
- [ ] Três merges confirmados; nenhum WP escondido como parcial em PR auxiliar, worktree ou ledger.
- [ ] Releases/deployments compatíveis com as fontes aprovadas; a observação contínua histórica de sete dias permanece uma obrigação operacional pós-entrega, rastreável, mas não bloqueia WP, sprint, promote, merge ou go-live.
- [ ] Consequências operacionais/dados afetados reconciliadas, runbooks/rollback e artefatos finais verificados.
- [ ] Evidências duráveis preservadas, pendências zero comprovadas e estado final comunicado sem crédito fictício.

## Organização do Git — PRs, commits, branches e worktrees

Escopo esclarecido pelo usuário: organizar o trabalho acumulado no Git, não reorganizar pastas, reescrever documentação histórica ou refatorar arquitetura. Responsável: integrador global; root decide destino e prioridade. É trabalho de integração dos três bundles, sem quarto bundle e sem WP genérico de limpeza.

**Estado inicialmente confirmado:** integração limpa; checkout principal em `pr-0c-d4-openrouter` com três conflitos preexistentes (index.ts, metrics.ts e containment-intake.test.ts sob deploy/cloudflare); onze PRs #548–#558 abertos em cadeia, nove drafts; 265 worktrees registrados. A contagem não prova que um item esteja abandonado ou pertença à campanha. Commits aprovados já existem tanto na integração quanto em worktrees de autores; nenhum deles deve ficar sem destino registrado.

### Mandato finito de reconciliação

Objetivo: dar destino comprovável a cada item Git em escopo da campanha e deixar exatamente os três PRs/bundles de entrega, sem perda de trabalho. Começo: snapshot somente de metadados Git/PR e estado dirty/conflito. Meio: mapa de incorporação e destino. Fim: itens da campanha reconciliados, material necessário preservado e checklist abaixo concluído nos marcos correspondentes. Não auditar código inteiro nem disparar testes pesados para inventariar refs.

O inventário usa um único registro referenciado pelo ledger, contendo por item: tipo/ID (PR, branch, worktree ou commit), dono conhecido ou desconhecido, WP/bundle, SHA atual, dirty/conflito, prova de incorporação/reachability quando existir, destino, próxima ação e responsável. Não copiar conteúdo de secrets ou arquivos privados para o inventário. Cobrir branches locais/remotas, PRs da campanha, worktrees associados e commits conhecidos em retornos/reflogs pertinentes que ainda não estejam ligados ao candidato; não chamar cada commit histórico sem branch própria de commit perdido.

Classificação obrigatória: **ATIVO NA SPRINT**, **PRESERVADO PARA SPRINT FUTURA**, **APROVADO A INCORPORAR**, **SUBSTITUÍDO COM CONTEÚDO COMPROVADO**, **FORA DA CAMPANHA** ou **ORIGEM/DESTINO A RESOLVER**. Idade ou nome de branch não determinam descarte.

### Checklist de organização do Git

- [ ] **PRs:** mapear #548–#558 aos WPs e às correções atuais; incorporar conteúdo válido nos três bundles. Fechar/sinalizar como substituído somente depois de comprovar preservação e registrar o destino. Nenhum merge individual dessa pilha. Outros PRs só entram se pertencerem comprovadamente à campanha.
- [ ] **Commits:** todo trabalho entregue por agente tem SHA completo, revisão, destino e prova de incorporação ou pendência explícita. Conferir commits ainda só presentes em worktrees/branches/reflogs pertinentes e protegê-los contra perda antes de limpeza. SHA antigo aprovado não substitui revisão de uma correção posterior.
- [ ] **Branches:** cada branch da campanha corresponde a WP/bundle ativo, material futuro preservado ou conteúdo substituído comprovado. Remover refs redundantes somente com conteúdo preservado, sem consumidor ativo ou dependência de pilha; registrar recuperação possível. Não reescrever branches compartilhadas indiscriminadamente.
- [ ] **Worktrees:** identificar dono/uso, mudanças não commitadas, conflitos e dependências reais. Remover apenas as elegíveis da campanha depois de preservar código, evidência e arquivos necessários. Não apagar worktree dirty, desconhecida ou utilizada por outro job; não forçar remoção para reduzir contagem.
- [ ] **Checkout principal:** identificar a operação Git pendente e a intenção dos dois lados dos três conflitos, preservar trabalho e conduzir a resolução apropriada. Não usar reset, ours/theirs ou stash destrutivo como atalho de organização. Resolver no contexto correto sem misturar WPs da sprint seguinte.
- [ ] **Bundles:** S1/S2/S3 têm refs inequívocas e estado rastreável. Refs vazias planejadas são identificadas como tal, não como candidatos prontos. Cada stacked PR contém apenas sua sprint, com base atualizada e commits recuperáveis.
- [ ] **Encerramento:** nenhum item da campanha fica como origem/destino desconhecido, commit aprovado esquecido, PR antigo pendurado sem justificativa ou worktree órfã com código único. Material de outras campanhas fica identificado e preservado; não é apagado para alegar limpeza total.

**Execução por marco:** durante S1, reconciliar a pilha existente e capturar todos os destinos conhecidos, preservando o trabalho já produzido para S2/S3 sem retomar sua implementação. Após cada incorporação/merge, limpar apenas o material que se tornou seguramente redundante. Antes do merge final, reconferir o inventário completo da campanha e as três provas de merge. Organização não pode virar outra auditoria sem fim: o critério é destino/reachability/uso comprovados para os itens definidos, não estética nem zero branches no repositório.

## Mandato e invariantes

- Encerrar os 54 WPs remanescentes e qualificar o go-live, preservando o escopo e os critérios originais dos 70 WPs. Os 16 registros históricos continuam separados e sua aplicabilidade é conferida quando forem dependências; não representam novas entregas desta execução.
- Exatamente TRÊS merges de bundles de PR na main de corelink-runners. Não mesclar individualmente a antiga pilha #548–#558. A coordenação necessária com corelink-server não será escondida: mudanças pareadas têm suas próprias evidências e ordem de deployment, registradas nos bundles consumidores.
- Um executor Luna responsável por cada WP até seu aceite. Revisores e integrador são apoios. O mesmo executor recebe todas as correções; retorno completed de uma tarefa não encerra o WP.
- Root decide arquitetura, contratos, prioridade e solução dos bloqueios; acompanha o fechamento. Agentes executam código e revisão. Terra apenas nos pontos justificados, atualmente revisão crítica e integração global.
- Até 15 agentes simultâneos. Operação normal admite até 12 executores, um revisor e um integrador; root coordena. Não abrir subtarefas com donos substitutos nem ocupar vagas artificialmente. Um agente não executa dois WPs ao mesmo tempo. Com a contenção atual, testes pesados ficam proibidos durante implementação; no gate final da sprint, no máximo dois jobs pesados locais simultâneos por causa da contenção observada. Preferir a capacidade apropriada do CI do PR, sem interferir em jobs de outros projetos.
- Reutilizar código, commits, revisões e testes válidos. Nenhuma reimplementação para reorganizar a campanha. Nenhum corte de critérios, waiver implícito, número de testes usado como entrega, produção fictícia ou janela temporal acelerada artificialmente.

## Os três bundles e seus marcos

| Bundle / único merge | Escopo | Condição de fechamento |
|---|---|---|
| B1 — base operacional | 12 WPs remanescentes da Sprint 1 | 12 implementações completas, composição/revisão, CI da Sprint 1, aceites pertencentes aos WPs, merge confirmado na main |
| B2 — execução e credenciais | 14 WPs da Sprint 2 | composição compatível com B1 e Server, critérios de código e aceites aplicáveis da Sprint 2, CI e segundo merge |
| B3 — qualificação e go-live | Sprint 3: 28 WPs das antigas Sprints 3 e 4 | implementação e aceites de ambas, gates operacionais, janela real exigida, releases/runbooks/provas, CI e terceiro merge |

Cada WP aparece exatamente uma vez no inventário ao fim deste documento. Por instrução explícita do usuário, existem exatamente TRÊS sprints operacionais, serializadas: S1=B1 (12 WPs), S2=B2 (14 WPs), S3=B3 (28 WPs). A antiga divisão em quatro permanece apenas como referência histórica de origem dos critérios; não governa mais a execução. Não executar WPs da sprint seguinte antes de fechar e mesclar a atual. Cada sprint tem um único PR de bundle na pilha: S1 sobre main, S2 sobre S1, S3 sobre S2; após cada merge, reconciliar a base do sucessor com main sem alterar o escopo. Criar/publicar o sucessor quando sua sprint puder começar. Cada PR recebe CI completa e testes pesados somente quando todos os WPs de sua sprint estiverem completos e integrados. Go-live só recebe crédito com a qualificação completa.

### Política de aceite operacional compartilhado da Sprint 1

Um WP fecha o seu aceite de código quando a fonte PROMOTE, os checks focados e o cartão de execução versionado estão incorporados ao B1. As provas compartilhadas de deploy, live e CI completa não são repetidas por WP: rodam uma vez no candidato B1 composto e continuam obrigatórias para o merge. Os resultados desse gate vinculam runtime, configuração e versão aos cartões de T8-W4b, T6-W4, T6-W9, T6-W13 e aos demais consumidores aplicáveis. Esta regra centraliza a execução, não remove requisito live, nem concede crédito de deploy ou go-live antes do gate do bundle.

## Cinco etapas de execução

1. **[EM ANDAMENTO] Consolidar a base e preparar B1.** Integrador reconcilia a pilha existente com os commits aprovados e constrói um candidato B1 limpo, sem importar trabalho parcial de B2/B3. Root vincula cada WP a executor, revisão, fonte e falta original. Saída: inventário único e diff B1 rastreável; preservar todos os worktrees/evidências úteis.
2. **[EM ANDAMENTO] Encerrar B1 e fazer o merge 1.** Prioridade de implementação: T6-W15 e T9-W1, os dois parciais da Sprint 1. Os outros dez já têm implementação registrada como completa e seguem para suas pendências de aceite, sem reescrita. Assim que as 12 implementações estiverem compostas, executar CI completa da sprint. Cumprir os aceites aplicáveis, resolver falhas com os mesmos executores e confirmar o merge B1 na main.
3. **[FILA] Encerrar B2 e fazer o merge 2.** Preservar as seis implementações completas da Sprint 2; concluir seus três parciais e cinco ainda sem implementação comprovada. Dentro da Sprint 2 ativa, consumidores podem ser implementados em paralelo quando existir contrato congelado e ausência de conflito de escrita; produção depende das capacidades reais. Nenhuma execução antecipada de WPs da Sprint 2 durante a Sprint 1. Aceitar B2 no SHA composto, com compatibilidade Runner/Server e ordem de deployment demonstradas.
4. **[FILA] Encerrar B3 e fazer o merge 3.** Concluir os 28 WPs da nova Sprint 3 (origem histórica: antigas Sprints 3/4), incluindo fornecedor/inventário, monitor final, recuperação durável, canary/provas, onboarding, integrações e releases. Construir o candidato completo necessário à qualificação antes de iniciar a janela imutável. A observação contínua histórica de sete dias é iniciada e acompanhada após a entrega como obrigação operacional rastreável; não é pré-condição de WP, sprint, promote, merge ou go-live.
5. **[PENDENTE DOS TRÊS MERGES] Conferir entrega integral.** Root confere os 54 WPs remanescentes contra os critérios originais, os três SHAs de merge, artefatos/deployments e evidências. Integrador confirma main, compatibilidade dos repositórios, documentação operacional e limpeza segura. Backlog zero somente quando nenhuma obrigação permanecer escondida em prepared, partial, PR auxiliar ou worktree.

## T6-W15: titularidade e lista finita de fechamento

Executor: r2_pair_acceptance (Luna). Worktree: /private/tmp/corelink-t6-w15-owner-20260906. Integrador global: runner_final_composition (Terra). Revisão crítica: spawn_preparation_integration (Terra); demais revisões delimitadas podem usar Luna independente.

- Incorporar ingestão aaa68154 e arquivos exatos de aceitação: cold review final comprova 25 testes distintos e TypeScript. Não repetir os 25 sem alteração/falha que justifique.
- Os três codecs, o runtime witness vinculado à conta verifier e a delimitação do prefixo IAM no template verifier foram integrados com suas fontes PROMOTE. O executor do WP mantém a titularidade para registry durável, ACK_RECOVERY, page ACK humano autenticado e runtime principal restantes.
- Concluir registry durável de signatários, ACK_RECOVERY, page ACK humano autenticado e runtime principal. Root congela antes as decisões ainda ausentes dessas interfaces; não deixa o executor parado indefinidamente nem o obriga a inventar arquitetura.
- Completar suíte base original, isolamento operacional, trusted time, WORM, crash/restart/rotation e probe A6.10 3/3. Demonstrar o que a base realmente promete.
- Histórico de atribuição, sem waiver: a observação contínua de sete dias de A6.17 foi associada a T6-W12 e T6-W10. Ela permanece uma obrigação operacional pós-entrega, com rastreabilidade própria, mas não bloqueia qualquer WP, sprint, promote, merge ou go-live e não cria ciclo T6-W15 → T6-W12 → T6-W15. Fontes: remediation-plan linhas 637/766/896–897, round3 linhas 432/652–654 e DAG linha 600.

## T9-W1: responsabilidade e decisão pendente explícita

Root é responsável por resolver a decisão de arquitetura/capacidade que impede o pacote. Não despachar mais código especulativo enquanto faltar autoridade real de terminalidade/cancelamento/uso (F005/F007). Aproveitar a implementação e as provas existentes; comparar a solução concreta com o contrato. D2 continua uma decisão distinta da autorização limitada do monitor. Antes do próximo trabalho de produto, nomear um executor Luna único e entregar um pacote com decisão fechada, paths e aceite. Uma autorização adicional, se efetivamente necessária para alterar critérios, deve vir com a alternativa concreta pronta e impacto explícito; autonomia não será usada como waiver inventado.

## Protocolo obrigatório de integração e merge

1. Executor entrega SHA completo, paths próprios e evidência contra cada critério do WP.
2. Revisor independente aprova o SHA exato ou devolve uma lista única de defeitos reproduzíveis contra o contrato congelado. Root resolve ambiguidades; não aceitar critérios novos a cada rodada.
3. Integrador incorpora imediatamente código aprovado quando suas dependências permitem, com verificação de alterações e testes afetados. Isso é integração, não merge em main nem entrega.
4. Candidato do bundle contém apenas seu escopo e prerequisites aprovados. CI completa e testes pesados rodam apenas com todos os WPs da sprint ativa completos e integrados em seu único stacked PR. Em S3/B3 há uma única sprint operacional com os 28 WPs e todos os critérios herdados.
5. PR do bundle descreve o problema resolvido, a composição final, cada WP, validação, aceites e limitações reais. Mudança de head/base exige conferir o novo diff e checks aplicáveis, sem reutilizar cegamente aprovação antiga.
6. Após todos os gates correspondentes, fazer exatamente o merge B1, B2 ou B3. Confirmar SHA remoto na main e atualizar ledger. Nenhum merge individual de WP ou quarto merge de ajuste planejado.

Main REMOTA conferida em cda90940f74735c006693d9b9e3be85c37b26f1a; a referência local main está desatualizada e não é prova de estado remoto. PRs #548–#558 são onze referências de fonte, nove em draft; todos os heads já estão no histórico da integração, mas não contêm todas as correções posteriores. Reconciliar e absorver seu conteúdo nos três bundles antes de fechar os PRs substituídos. Não abrir uma nova pilha de PRs individuais.

O checkout principal do usuário tem conflitos preexistentes e permanece preservado. Usar checkouts limpos para os bundles. Não fazer merge cego da branch de integração acumulada nem descartar trabalho alheio. Alterações em corelink-server e dependências externas são requisitos explícitos, nunca um quarto bundle de Runner disfarçado.

## Método de execução obrigatório — entrada, saída e controle de retrabalho

### 1. Porta de entrada de cada WP

Root só autoriza implementação com um pacote único e verificável contendo TODOS os itens abaixo. Se faltar um, a ação é completar o pacote; não mandar o executor descobrir o que foi omitido.

| Item obrigatório | Evidência de que está pronto |
|---|---|
| Baseline e fonte aproveitável | SHA completo, worktree exclusivo, commits existentes preservados e dependências exatas |
| Titularidade e limites | Um executor, um revisor independente, lista de paths de escrita e conflitos resolvidos antes do trabalho |
| Decisões técnicas | Interfaces, wire formats, autoridades, invariantes, persistência, falhas/retries e regras de segurança decididos por root quando aplicáveis |
| Aceite original | Cada critério do WP mapeado a teste, probe ou artefato; separar teste local, composição, deployment e evidência temporal |
| Dependências | Para cada dependência: satisfeita com evidência, contrato congelado que permite implementar, ou bloqueio factual; nenhuma dependência vaga |
| Execução dos checks | Comandos e diretório corretos, dependências reais, limites de concorrência e resultado esperado; nenhuma declaração falsa para esconder composição ausente |

Um WP pode ter várias etapas internas, mas todas pertencem ao mesmo executor e à mesma lista de aceite. Não abrir um novo pacote para cada helper, erro ou arquivo. Mudança necessária do contrato é decidida por root e registrada antes de alterar a implementação.

### 2. Estados e critérios de saída

| Estado | Condição objetiva para sair |
|---|---|
| PRONTO PARA EXECUTAR | Os seis itens da porta de entrada estão completos |
| EM IMPLEMENTAÇÃO | Todos os critérios de código implementados; checks previstos executados; SHA limpo e cartão de entrega completo |
| EM REVISÃO | Parecer independente sobre o SHA e a lista integral de critérios: aprovar ou listar defeitos reproduzíveis |
| APROVADO PARA INTEGRAR | Defeitos impeditivos resolvidos e aprovação vinculada ao SHA exato |
| INTEGRADO NO BUNDLE | Commits incorporados, conflitos resolvidos sem perda e checks afetados aprovados no candidato |
| EM ACEITE | CI da sprint/bundle e provas operacionais originais satisfeitas na versão correta |
| ENTREGUE | WP incluído em um dos três merges confirmados na main, com os aceites aplicáveis e registros correspondentes |

BLOQUEADO é uma condição explícita, não um estado terminal conveniente: registrar causa, dono da resolução, evidência e próxima ação. O WP mantém seu executor. Não marcar implementação completa por ausência de testes nem exigir evidência de um sucessor como pré-condição circular do predecessor.

### 3. Revisão em lote e prevenção de loops

- O executor verifica seu trabalho contra toda a lista de aceite antes de submetê-lo. Entrega um cartão único: WP, SHA, paths, critérios cobertos, comandos/exit codes, evidências e pendências. Não enviar sucessivos commits como se cada um encerrasse o pacote.
- O revisor inspeciona o escopo completo e devolve um parecer consolidado. Cada defeito informa regra violada, reprodução/evidência, gravidade e critério de correção. Não usar preferências de estilo ou requisitos inventados como bloqueio.
- O mesmo executor corrige o conjunto recebido em uma rodada. A nova revisão verifica as correções e as regressões pertinentes; não reinicia uma investigação geral sem motivo técnico. Evidência já válida é reaproveitada quando o código e suas dependências relevantes não mudaram.
- Se a SEGUNDA submissão continuar reprovada, root interrompe a sequência automática de tentativas e identifica a causa: contrato incompleto/contraditório, contexto errado, dependência ausente, erro de implementação, teste incorreto ou ambiente inadequado. Corrige a causa e atualiza o pacote antes da próxima submissão. Não troca o agente ou abre mais frentes como reação automática.
- Defeito real novo continua sendo corrigido, mesmo descoberto tarde. O método limita retrabalho evitável, não a descoberta de problemas. Qualquer nova obrigação de aceite exige referência ao contrato original ou decisão explícita; não enfraquecer teste para produzir verde.
- Root faz julgamento e diagnóstico de processo/contrato; inspeção e correção de código continuam com agentes. Usar Terra em um diagnóstico delimitado quando a dificuldade justificar, sem converter todo o trabalho para modelos mais caros.

### 4. Revisões, auditorias e cold reviews: contratos finitos e promoção objetiva

É proibido despachar uma revisão com uma instrução aberta como "audita aí". Root fornece um mandato de revisão antes do início; isso também vale para segurança e auditoria independente. Nenhuma revisão pode redefinir silenciosamente o aceite de um WP.

**Mandato obrigatório:** identificador/WP e estágio de promoção; objetivo ou pergunta a responder; baseline e SHA alvo completos; paths/contratos e dependências em escopo; independência necessária; riscos concretos a cobrir; lista de critérios e evidências exigidas; comandos permitidos; exclusões; prazo de checkpoint; formato de retorno e condição de encerramento. Faltando entrada, solicitar a informação exata a root e realizar apenas a parte independente já delimitada. Não presumir sucesso nem sair à procura de uma nova tarefa.

**Começo:** confirmar SHA/base, diff e contexto corretos; conferir os critérios e as evidências existentes. Registrar defeitos anteriores relevantes sem atribuí-los ao patch incorretamente. Não executar testes repetidos só para produzir um log próprio quando o mandato permite verificar evidência válida.

**Meio:** inspecionar todos os critérios do mandato e as ramificações pertinentes do mecanismo alterado. Executar apenas verificações necessárias para responder às perguntas definidas. Qualidade e segurança incluem, quando aplicáveis ao escopo: comportamento correto e limites; autorização/isolamento de identidade; proteção de secrets; validação de entradas; integridade e atomicidade do estado; idempotência, concorrência e recuperação; tratamento de falhas; compatibilidade; dependências e privilégios; simplicidade/manutenibilidade. Justificar critérios não aplicáveis; não transformar essa lista em auditoria irrestrita de todo o sistema.

**Fim:** entregar UM parecer consolidado contendo cobertura de cada critério (PASS/FAIL/NÃO VERIFICADO), evidências, achados e decisão sobre a promoção solicitada. A revisão termina quando respondeu ao mandato; não fica aberta para procurar indefinidamente mais coisas. Revisão encerrada com reprovação não significa produto aprovado: o fix continua com o executor e recebe verificação delimitada.

| Decisão | Critério objetivo | Próxima ação |
|---|---|---|
| PROMOVER | Todos os critérios obrigatórios do estágio satisfeitos no SHA alvo; evidência suficiente; nenhum defeito impeditivo conhecido | Integrar ou avançar para o próximo gate definido; não inferir autorização de deploy/merge a partir de revisão de fonte |
| CORRIGIR | Um ou mais critérios obrigatórios falham, com regra violada e evidência concreta | Devolver uma lista consolidada ao mesmo executor; depois verificar fixes e regressões pertinentes |
| INCONCLUSIVO | Evidência/ambiente/entrada indispensável indisponível; a hipótese ainda não permite conclusão | Informar exatamente a lacuna, seu dono e a menor ação para resolvê-la; não promover nem condenar código por adivinhação |

**Achados objetivos:** cada achado contém ID estável, critério/contrato violado, localização, evidência (reprodução ou caminho demonstrável), impacto, gravidade fundamentada e condição verificável de resolução. Classificar como introduzido, preexistente ou hipótese ainda não comprovada. Gravidade de segurança é derivada de exposição e impacto; não exigir exploração em produção para reconhecer uma falha demonstrável. Hipótese plausível grave demanda a verificação delimitada necessária, não uma promoção por falta de exploração.

**O que impede promoção:** violação de segurança, integridade, autorização, comportamento, recuperação ou compatibilidade exigidos; critério de qualidade obrigatório descumprido; ou prova indispensável ausente. Nenhum achado impeditivo pode ser ignorado para cumprir prazo. Requisitos de qualidade devem estar no contrato/regras aplicáveis e ser verificáveis. Preferência pessoal, generalização para futuro hipotético ou refatoração opcional não são defeitos e não bloqueiam. Um defeito real de menor gravidade permanece explicitamente tratado; não usar "não bloqueante" para esconder dívida sem autorização.

**Revisão da correção:** manter os mesmos IDs e critérios. Avaliar o diff desde o SHA revisado, a resolução dos achados e os efeitos relacionados. Reabrir um item só com evidência de que a condição de resolução falhou ou houve regressão. Uma falha nova legítima recebe ID, justificativa de escopo e a análise de causa/ramificações/consequências/prevenção. Não repetir a auditoria inteira nem impor uma nova preferência em cada rodada. A segunda submissão reprovada aciona o diagnóstico de processo/contrato de root já definido neste plano.

**Cold review:** independência significa autor diferente, contexto de aceite suficiente e conclusão própria sobre o alvo exato. Não significa desconhecer o contrato, ignorar provas já válidas ou refazer tudo. Um cold reviewer que alterar produto passa a autor da alteração e outro revisor independente verifica esse ajuste.

**Promoções separadas:** aprovação de fonte habilita integração; composição válida habilita o gate completo da sprint; CI e aceites aplicáveis aprovados no SHA correto habilitam um dos três merges. Mudança de SHA requer análise do delta e revalidação proporcional, não invalidação automática de toda evidência nem reutilização cega da aprovação. CI completa/testes pesados permanecem exclusivamente no fechamento da sprint ativa. Falhas são agrupadas e corrigidas em paralelo pelo protocolo já definido.

**Controle de duração:** o mandato define um checkpoint proporcional ao escopo, nunca superior aos 60 minutos sem avanço verificável definidos neste plano. No checkpoint, o revisor apresenta cobertura concluída, achados e falta concreta; root reduz ambiguidades ou resolve bloqueios. Tempo esgotado não transforma FAIL/INCONCLUSIVO em PASS. A revisão tem fim pela cobertura objetiva do mandato, não por cansaço do agente ou do usuário.

### 5. Todo fix: causa raiz, ramificações, consequências e prevenção proporcional

Esta regra vale para TODO defeito, desde a primeira correção. O diagnóstico adicional após uma segunda reprovação continua obrigatório, mas não substitui a análise inicial. A profundidade da análise é proporcional ao impacto; um bug simples pode ser documentado em um parágrafo, sem processo artificial.

O executor entrega junto ao fix um registro objetivo com seis elementos:

1. **Causa raiz:** mecanismo que produziu o defeito e por que foi introduzido ou escapou dos controles existentes. Distinguir fato demonstrado de hipótese; não parar na mensagem de erro ou no sintoma.
2. **Ramificações:** outros caminhos, consumidores, contratos ou estados que compartilham a mesma causa. Busca limitada ao mecanismo afetado; quando houver ocorrências, corrigir todas dentro dos escopos autorizados e coordenar os owners envolvidos. Não transformar a busca em auditoria genérica do repositório.
3. **Consequências:** efeitos já produzidos ou possíveis sobre estado persistido, dados, segurança, compatibilidade, ações externas e operação. Se houver dados/efeitos a reconciliar, uma alteração de código sozinha não encerra o defeito. Registrar a remediação concreta, sua evidência e qualquer autorização específica necessária antes de ação irreversível.
4. **Correção completa e mínima:** eliminar a causa e as manifestações relacionadas, preservando contratos e tratando estados afetados. Não remendar o sintoma, não ocultar a falha e não misturar refatoração sem relação.
5. **Prevenção avaliada:** indicar se precisa de teste de regressão significativo, validação de fronteira, ajuste de contrato/configuração ou controle operacional. Escolher o menor mecanismo eficaz. Se os controles existentes já bastam, explicar brevemente por que nenhuma nova medida é necessária; não criar uma ferramenta ou abstração apenas para mostrar prevenção.
6. **Evidência:** reprodução inicial, critério de correção e verificação dos caminhos afetados. Durante implementação/retrabalho usar verificações pequenas necessárias; testes pesados permanecem exclusivos do gate de entrega da sprint.

O revisor confere esses seis pontos, a cobertura das ramificações pertinentes e o tamanho da solução. Um fix com causa eliminada mas consequências obrigatórias pendentes continua aberto. Um fix correto sem necessidade de arquitetura adicional não pode ser bloqueado por preferência de estilo ou generalização hipotética.

**Controle de overengineering:** antes de propor camada, framework, serviço, dependência, API genérica ou migração nova, o executor precisa demonstrar qual requisito atual não pode ser atendido com os mecanismos existentes. Root decide a necessidade. Preferir alterações locais, APIs existentes e testes nos limites do comportamento. Não construir para necessidades futuras imaginadas, não duplicar lógica já qualificada e não ampliar o escopo apenas para tornar a solução mais elegante. Ser objetivo não autoriza omitir segurança, recuperação, compatibilidade ou consequências exigidas pelo problema real.

### 6. Falhas de CI: reparo paralelo por causa e escopo

- CI vermelha mantém a mesma sprint ativa. Não iniciar a seguinte, abrir uma quarta sprint ou marcar entrega parcial.
- Root e o integrador consolidam as N falhas por causa provável, WP e arquivos afetados. Falhas em cascata da mesma causa são um único pacote de correção; não criar N agentes para N mensagens de erro.
- Cada pacote recebe causa/reprodução, paths exclusivos, critério de correção, executor e revisor. O executor original do WP mantém sua responsabilidade. Apoios de correção só entram com escopos de escrita disjuntos; não transferem titularidade do WP.
- Pacotes independentes executam em paralelo e de forma assíncrona até o teto de 15 agentes. Pacotes que alteram o mesmo arquivo são unidos ou serializados. Root resolve contratos antes de liberar alterações concorrentes.
- Durante o reparo, executar apenas reproduções pequenas e verificações focadas necessárias. Não relançar a suíte pesada inteira por agente ou por commit.
- Integrador incorpora correções aprovadas assim que prontas. Após a composição das correções necessárias, rodar os jobs falhos/afetados e os checks exigidos no novo SHA do PR. Gate completo é repetido quando exigido pelo CI ou pelo impacto da alteração, não por reflexo.
- Zero falhas impeditivas, evidências de aceite válidas e checks exigidos aprovados no SHA final autorizam o merge da sprint. Só depois liberar a próxima.

### 7. Concorrência e checkpoint operacional

- Escrita disjunta é condição para paralelizar. Um owner de integração por repositório; shared files entram em fila. Nunca dois agentes consertando o mesmo arquivo simultaneamente.
- Priorizar encerrar revisões/correções e incorporar trabalho pronto antes de ocupar uma vaga com um novo WP. Isso não obriga trabalho independente a esperar um bloqueio externo sem relação.
- Em até 60 minutos de execução sem mudança de estado verificável, root confere o impedimento e toma uma ação concreta. Esse intervalo é limite para diagnosticar estagnação, não promessa de duração do WP. Não repetir comando caro sem hipótese nova.
- Ao fim de cada ciclo, conferir: WPs por estado, achados ainda abertos, idade de espera de trabalho aprovado, SHAs ainda fora do bundle, jobs ativos e próximo passo de cada executor. Agente completed com WP pendente recebe followup_task para a etapa autorizada seguinte.
- Reportar avanço de implementação, aceite e merge separadamente. Métricas de controle: critérios fechados, pendências restantes, número/motivo das rodadas de correção e tempo de espera por integração. Não somar testes duplicados nem inventar percentual de pronto.

## Regras que root deve executar a cada retorno

- Consultar este plano e o ledger antes de delegar, integrar, alterar estado ou iniciar merge. Atualizar o registro a cada mudança verificável.
- Se o agente terminou mas o WP não: encaminhar revisão, integração ou correção ao mesmo executor imediatamente. Se completed exige mais trabalho autorizado, usar followup_task; send_message não retoma agente encerrado.
- Não abrir outra frente para contornar a pendência. Root decide e registra uma solução ou um bloqueio factual com responsável e próxima ação.
- Reaproveitar uma vaga somente para outro escopo elegível da MESMA sprint ativa, depois do fechamento da atribuição e da passagem clara de responsabilidade. Um WP bloqueado mantém seu dono; não fica abandonado em uma lista.
- Status ao usuário: WPs que mudaram de estágio, merges efetivos, faltas e decisões. Não apresentar commits/testes como progresso de entrega, nem ocultar implementação concluída atrás de um contador global estático.
- Qualquer desvio deste plano exige registro no próprio plano: motivo, evidência, impacto nos três bundles e ação corretiva. Alteração de escopo/aceite exige a autorização correspondente. Não criar outro plano paralelo ou recomeçar a campanha após compaction.

## Inventário dos 54 WPs por bundle

Estado inicial é extraído do ledger; complete abaixo significa implementação, não entrega. O dono do WP é registrado no ledger quando o pacote entra em execução, antes da primeira mudança. A fila não implica 54 agentes ativos.

### B1 — 12 WPs

| WP | Sprint histórica de origem | Implementação inicial |
|---|---:|---|
| T4-W4 | 1 | complete |
| T3-W10 | 1 | complete |
| T6-W2 | 1 | complete |
| T6-W4 | 1 | complete |
| T6-W15 | 1 | partial |
| T2-W3 | 1 | complete |
| T8-W4b | 1 | complete |
| T9-W1 | 1 | partial |
| T1-W5 | 1 | complete |
| T6-W9 | 1 | complete |
| T3-W18 | 1 | complete |
| T6-W13 | 1 | complete |

### B2 — 14 WPs

| WP | Sprint histórica de origem | Implementação inicial |
|---|---:|---|
| T4-W1 | 2 | complete |
| T4-W2 | 2 | complete |
| T3-W3 | 2 | complete |
| T3-W1 | 2 | complete |
| T3-W2 | 2 | partial |
| T8-W1 | 2 | partial |
| T8-W3 | 2 | partial |
| T3-W14 | 2 | unknown |
| T3-W9 | 2 | unknown |
| T8-W5 | 2 | complete |
| T8-W2 | 2 | complete |
| T2-W2b | 2 | unknown |
| T2-W4 | 2 | unknown |
| T2-W6 | 2 | unknown |

### B3 — 28 WPs

| WP | Sprint histórica de origem | Implementação inicial |
|---|---:|---|
| T5-W1 | 4 | unknown |
| T5-W2 | 4 | unknown |
| T5-W3 | 4 | partial |
| T3-W16 | 3 | unknown |
| T3-W15 | 3 | unknown |
| T8-W6 | 3 | unknown |
| T8-W7 | 4 | unknown |
| T3-W5 | 4 | unknown |
| T1-W6 | 3 | unknown |
| T1-W2 | 3 | unknown |
| T1-W3 | 3 | unknown |
| T1-W4 | 4 | unknown |
| T2-W5 | 3 | unknown |
| T3-W7 | 3 | unknown |
| T3-W8 | 3 | unknown |
| T4-W7 | 3 | unknown |
| T4-W8 | 3 | unknown |
| T5-W4 | 4 | unknown |
| T5-W5 | 4 | unknown |
| T5-W6 | 4 | unknown |
| T6-W5 | 3 | unknown |
| T6-W6 | 3 | unknown |
| T6-W7 | 3 | unknown |
| T6-W10 | 4 | unknown |
| T6-W11 | 4 | unknown |
| T7-W5 | 4 | unknown |
| T6-W12 | 3 | unknown |
| T6-W14 | 4 | unknown |


## Adendo vinculante — serialização de sprints e paralelismo interno

A instrução mais recente do usuário prevalece sobre qualquer formulação anterior: três sprints operacionais, exatamente uma ativa por vez; dentro dela, máximo paralelismo útil e assíncrono com rigor, até 15 agentes, um dono por WP e escritas sem conflito. Nenhum teste pesado durante implementação. Testes focados pequenos continuam permitidos quando necessários. Cada sprint fecha sua implementação em um stacked PR, recebe CI/testes pesados, tem as falhas reparadas em paralelo por escopo e só termina após aceite e merge. A próxima sprint não inicia antes disso. Trabalho já existente de sprints posteriores fica preservado, sem retomada de execução antecipada.
