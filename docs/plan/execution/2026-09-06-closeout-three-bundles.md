# Corelink — contrato de orquestração: fechar todos os WPs em três merges

Data: 2026-09-06. Dono deste plano: root. Estado: EM EXECUÇÃO.
Este é o plano operacional que o orquestrador deve seguir, por determinação explícita do usuário. Substitui integralmente a versão operacional anterior. Espelho canônico no repositório: docs/plan/execution/2026-09-06-closeout-three-bundles.md; ambos devem conter a mesma versão, não planos concorrentes.

## Mandato e invariantes

- Encerrar os 54 WPs remanescentes e qualificar o go-live, preservando o escopo e os critérios originais dos 70 WPs. Os 16 registros históricos continuam separados e sua aplicabilidade é conferida quando forem dependências; não representam novas entregas desta execução.
- Exatamente TRÊS merges de bundles de PR na main de corelink-runners. Não mesclar individualmente a antiga pilha #548–#558. A coordenação necessária com corelink-server não será escondida: mudanças pareadas têm suas próprias evidências e ordem de deployment, registradas nos bundles consumidores.
- Um executor Luna responsável por cada WP até seu aceite. Revisores e integrador são apoios. O mesmo executor recebe todas as correções; retorno completed de uma tarefa não encerra o WP.
- Root decide arquitetura, contratos, prioridade e solução dos bloqueios; acompanha o fechamento. Agentes executam código e revisão. Terra apenas nos pontos justificados, atualmente revisão crítica e integração global.
- Até 15 agentes simultâneos. Operação normal admite até 12 executores, um revisor e um integrador; root coordena. Não abrir subtarefas com donos substitutos nem ocupar vagas artificialmente. Um agente não executa dois WPs ao mesmo tempo. Com a contenção atual, no máximo dois jobs pesados de testes simultâneos.
- Reutilizar código, commits, revisões e testes válidos. Nenhuma reimplementação para reorganizar a campanha. Nenhum corte de critérios, waiver implícito, número de testes usado como entrega, produção fictícia ou janela temporal acelerada artificialmente.

## Os três bundles e seus marcos

| Bundle / único merge | Escopo | Condição de fechamento |
|---|---|---|
| B1 — base operacional | 12 WPs remanescentes da Sprint 1 | 12 implementações completas, composição/revisão, CI da Sprint 1, aceites pertencentes aos WPs, merge confirmado na main |
| B2 — execução e credenciais | 14 WPs da Sprint 2 | composição compatível com B1 e Server, critérios de código e aceites aplicáveis da Sprint 2, CI e segundo merge |
| B3 — qualificação e go-live | 28 WPs das Sprints 3 e 4 | implementação e aceites de ambas, gates operacionais, janela real exigida, releases/runbooks/provas, CI e terceiro merge |

Cada WP aparece exatamente uma vez no inventário ao fim deste documento. As quatro sprints originais continuam delimitando critérios e CI; a nova organização agrupa sua entrega em três merges. Planejar CI e merges sem criar dependência circular entre evidências posteriores e a base que as habilita. Go-live só recebe crédito com a qualificação completa.

## Cinco etapas de execução

1. **[EM ANDAMENTO] Consolidar a base e preparar B1.** Integrador reconcilia a pilha existente com os commits aprovados e constrói um candidato B1 limpo, sem importar trabalho parcial de B2/B3. Root vincula cada WP a executor, revisão, fonte e falta original. Saída: inventário único e diff B1 rastreável; preservar todos os worktrees/evidências úteis.
2. **[EM ANDAMENTO] Encerrar B1 e fazer o merge 1.** Prioridade de implementação: T6-W15 e T9-W1, os dois parciais da Sprint 1. Os outros dez já têm implementação registrada como completa e seguem para suas pendências de aceite, sem reescrita. Assim que as 12 implementações estiverem compostas, executar CI completa da sprint. Cumprir os aceites aplicáveis, resolver falhas com os mesmos executores e confirmar o merge B1 na main.
3. **[FILA] Encerrar B2 e fazer o merge 2.** Preservar as seis implementações completas da Sprint 2; concluir seus três parciais e cinco ainda sem implementação comprovada. Consumidores podem ser implementados antecipadamente quando existir contrato congelado e ausência de conflito de escrita; produção depende das capacidades reais. Aceitar B2 no SHA composto, com compatibilidade Runner/Server e ordem de deployment demonstradas.
4. **[FILA] Encerrar B3 e fazer o merge 3.** Concluir os 28 WPs das Sprints 3/4, incluindo fornecedor/inventário, monitor final, recuperação durável, canary/provas, onboarding, integrações e releases. Construir o candidato completo necessário à qualificação antes de iniciar a janela imutável. Usar o período obrigatório de observação para tarefas compatíveis que não alterem a identidade observada. Não prometer sete dias antes de existir um início válido e selado.
5. **[PENDENTE DOS TRÊS MERGES] Conferir entrega integral.** Root confere os 54 WPs remanescentes contra os critérios originais, os três SHAs de merge, artefatos/deployments e evidências. Integrador confirma main, compatibilidade dos repositórios, documentação operacional e limpeza segura. Backlog zero somente quando nenhuma obrigação permanecer escondida em prepared, partial, PR auxiliar ou worktree.

## T6-W15: titularidade e lista finita de fechamento

Executor: r2_pair_acceptance (Luna). Worktree: /private/tmp/corelink-t6-w15-owner-20260906. Integrador global: runner_final_composition (Terra). Revisão crítica: spawn_preparation_integration (Terra); demais revisões delimitadas podem usar Luna independente.

- Incorporar ingestão aaa68154 e arquivos exatos de aceitação: cold review final comprova 25 testes distintos e TypeScript. Não repetir os 25 sem alteração/falha que justifique.
- Incorporar três codecs já aprovados; corrigir e revisar independentemente o runtime witness para contexto da conta verifier. Corrigir a delimitação do prefixo IAM no template verifier. O executor do WP assume esses ajustes e mantém a titularidade.
- Concluir registry durável de signatários, ACK_RECOVERY, page ACK humano autenticado e runtime principal. Root congela antes as decisões ainda ausentes dessas interfaces; não deixa o executor parado indefinidamente nem o obriga a inventar arquitetura.
- Completar suíte base original, isolamento operacional, trusted time, WORM, crash/restart/rotation e probe A6.10 3/3. Demonstrar o que a base realmente promete.
- Correção de atribuição, sem waiver: os sete dias de A6.17 são qualificação posterior implementada por T6-W12 e coletada por T6-W10. Não bloqueiam a base T6-W15 nem criam ciclo T6-W15 → T6-W12 → T6-W15. Fontes: remediation-plan linhas 637/766/896–897, round3 linhas 432/652–654 e DAG linha 600. Os sete dias continuam obrigatórios para o gate final correspondente.

## T9-W1: responsabilidade e decisão pendente explícita

Root é responsável por resolver a decisão de arquitetura/capacidade que impede o pacote. Não despachar mais código especulativo enquanto faltar autoridade real de terminalidade/cancelamento/uso (F005/F007). Aproveitar a implementação e as provas existentes; comparar a solução concreta com o contrato. D2 continua uma decisão distinta da autorização limitada do monitor. Antes do próximo trabalho de produto, nomear um executor Luna único e entregar um pacote com decisão fechada, paths e aceite. Uma autorização adicional, se efetivamente necessária para alterar critérios, deve vir com a alternativa concreta pronta e impacto explícito; autonomia não será usada como waiver inventado.

## Protocolo obrigatório de integração e merge

1. Executor entrega SHA completo, paths próprios e evidência contra cada critério do WP.
2. Revisor independente aprova o SHA exato ou devolve uma lista única de defeitos reproduzíveis contra o contrato congelado. Root resolve ambiguidades; não aceitar critérios novos a cada rodada.
3. Integrador incorpora imediatamente código aprovado quando suas dependências permitem, com verificação de alterações e testes afetados. Isso é integração, não merge em main nem entrega.
4. Candidato do bundle contém apenas seu escopo e prerequisites aprovados. CI completa roda com a implementação da sprint composta, conforme regra existente. No B3, validar os escopos das duas sprints no candidato pertinente.
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

### 4. Concorrência e checkpoint operacional

- Escrita disjunta é condição para paralelizar. Um owner de integração por repositório; shared files entram em fila. Nunca dois agentes consertando o mesmo arquivo simultaneamente.
- Priorizar encerrar revisões/correções e incorporar trabalho pronto antes de ocupar uma vaga com um novo WP. Isso não obriga trabalho independente a esperar um bloqueio externo sem relação.
- Em até 60 minutos de execução sem mudança de estado verificável, root confere o impedimento e toma uma ação concreta. Esse intervalo é limite para diagnosticar estagnação, não promessa de duração do WP. Não repetir comando caro sem hipótese nova.
- Ao fim de cada ciclo, conferir: WPs por estado, achados ainda abertos, idade de espera de trabalho aprovado, SHAs ainda fora do bundle, jobs ativos e próximo passo de cada executor. Agente completed com WP pendente recebe followup_task para a etapa autorizada seguinte.
- Reportar avanço de implementação, aceite e merge separadamente. Métricas de controle: critérios fechados, pendências restantes, número/motivo das rodadas de correção e tempo de espera por integração. Não somar testes duplicados nem inventar percentual de pronto.

## Regras que root deve executar a cada retorno

- Consultar este plano e o ledger antes de delegar, integrar, alterar estado ou iniciar merge. Atualizar o registro a cada mudança verificável.
- Se o agente terminou mas o WP não: encaminhar revisão, integração ou correção ao mesmo executor imediatamente. Se completed exige mais trabalho autorizado, usar followup_task; send_message não retoma agente encerrado.
- Não abrir outra frente para contornar a pendência. Root decide e registra uma solução ou um bloqueio factual com responsável e próxima ação.
- Reaproveitar uma vaga somente depois do fechamento da atribuição e da passagem clara de responsabilidade. Um WP bloqueado mantém seu dono; não fica abandonado em uma lista.
- Status ao usuário: WPs que mudaram de estágio, merges efetivos, faltas e decisões. Não apresentar commits/testes como progresso de entrega, nem ocultar implementação concluída atrás de um contador global estático.
- Qualquer desvio deste plano exige registro no próprio plano: motivo, evidência, impacto nos três bundles e ação corretiva. Alteração de escopo/aceite exige a autorização correspondente. Não criar outro plano paralelo ou recomeçar a campanha após compaction.

## Inventário dos 54 WPs por bundle

Estado inicial é extraído do ledger; complete abaixo significa implementação, não entrega. O dono do WP é registrado no ledger quando o pacote entra em execução, antes da primeira mudança. A fila não implica 54 agentes ativos.

### B1 — 12 WPs

| WP | Sprint original | Implementação inicial |
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

| WP | Sprint original | Implementação inicial |
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

| WP | Sprint original | Implementação inicial |
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

