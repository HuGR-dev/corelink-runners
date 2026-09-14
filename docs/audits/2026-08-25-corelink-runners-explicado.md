# CoreLink Runners — O Relatório Definitivo, Explicado

**Data:** 2026-08-25 · **Repositório:** `corelink-runners` · **Versão:** final (triple-check incorporado)
**Este documento** explica tudo que a auditoria encontrou em linguagem direta. Cada afirmação tem a linha exata do código (`arquivo:linha`) para quem quiser conferir. A versão técnica densa está em `comprehensive-audit.md`, no mesmo diretório.

**Como navegar:**
- Tem 3 minutos → Parte 1 (as histórias).
- Vai decidir prioridade → Parte 1 + tabela de vazamentos da Parte 7 + Plano da Parte 10.
- Vai executar → Partes 4–5 + Parte 10, com `comprehensive-audit.md` aberto do lado.

> **Segunda-feira de manhã:** ① uma linha no TTL do caderno de cobrança para o vazamento que escala (RC1) · ② ligar o filtro de instalações aprovado e fazer o deploy falhar sem ele (RH1) · ③ fatiar o envio de cobranças em ≤512 antes que a bomba-relógio estoure (RH6) · ④ flipar `max_vcpu_h` no server e conferir a primeira invoice do Stripe. Quatro gestos, um dia, os quatro maiores riscos contidos.

---

## Parte 1 — A história inteira em 5 minutos

O CoreLink Runners vende **velocidade de CI**: você roda seus builds em caixas que nós ligamos na hora, e essas caixas vêm com o cache do CoreLink pré-aquecido — então seu build de 8 minutos vira 3.

Funciona assim, em uma frase por passo:

1. Você manda um job pro GitHub com a label certa.
2. O GitHub avisa nossos servidores (uma vez só — ele nunca repete o aviso).
3. Nós verificamos se você pagou, alugamos uma caixa, conectamos ela ao seu job.
4. A caixa baixa o cache quente e roda seu build.
5. No fim, mandamos a conta: segundos de caixa × preço.

A auditoria (10 analistas independentes + verificação tripla de tudo) concluiu:

> **A lógica de segurança é correta. A postura de configuração é frágil. E o dinheiro tem dois buracos medidos.**

As três feridas graves, contadas como histórias:

**🩸 História 1 — O job que nunca foi cobrado.** Para faturar um job no fim, precisamos lembrar *de quem era aquele job*. Essa lembrança vive num caderno que se autodestrói em **2 horas** (`index.ts:816`). Builds grandes duram mais que isso. Quando o job termina, olhamos o caderno: cinzas. Sem nome, sem cobrança. A caixa custou nosso dinheiro; o cliente pagou $0. E o pior: acontece **sempre** com jobs longos — ou seja, **quanto maior e mais fiel o cliente, menos ele paga**. O próprio código confessa: *"correct for its purpose and WRONG as the fleet's [tenant record]"* (`index.ts:856`). Correção: **uma linha** (guardar a lembrança por 60 dias em vez de 2 horas).

**🩸 História 2 — As caixas fantasmas.** Às vezes perdemos o recibo de uma caixa (a escrita falha em silêncio, `index.ts:1274-1296`). Caixa sem recibo = invisível para o sistema de limpeza. Ficamos espertos e construímos um detector de fantasmas — mas o botão "matar" está **conectado na tomada errada**: você ordena "destrói a caixa X" e o sistema responde "204 feito ✅" sem ter destruído nada, porque procura pelo nome na gaveta errada (nomes em formatos diferentes, `container-instances.sh:310-315`). Caso real medido: **3 caixas ligadas 10,2 horas à toa (~$12)** — o gatilho daquele incidente específico foi consertado, mas o buraco estrutural permanece: qualquer caixa que perca o recibo continua sem botão de destruição. Enquanto o conserto estrutural (ADR-0010) não sai, toda caixa nessa situação é irrecuperável por construção.

**🩸 História 3 — O job que sumiu.** Passo 2 acima: o GitHub avisa **uma vez só**. Respondemos "recebido!" em milissegundos e prometemos processar em segundo plano. Se o spawn demora demais (caixa fria, imagem pesada), a plataforma Cloudflare **mata nosso processamento no meio** — e o bilhete "esse job precisa de resgate" só é escrito *depois* que um erro tem tempo de ser notado. Bilhete nunca escrito, aviso nunca repetido: o job fica em "queued" até o GitHub desistir, ~24 horas depois. O cliente encara a tela girando. Nós nem ficamos sabendo (`index.ts:3067`, `lib.ts:1114`). Correção conhecida e pequena: escrever o bilhete de resgate **antes** de começar, apagar quando der certo.

Depois delas, mais 10 problemas sérios, 22 médios e ~10 menores — todos catalogados com linha e correção nas partes seguintes. Também tem a parte boa: **o que está excelente de verdade** (Parte 8), porque mexer sem saber onde o piso é forte é como reformar casa sem saber onde são as vigas.

---

## Parte 2 — Mini-dicionário (30 segundos)

| Termo | Significado aqui |
|---|---|
| **Caixa** (*box*, no código) | A máquina virtual que roda o job do cliente (padrão: 4 vCPU) |
| **Slot** | A "cadeira" contábil que uma caixa ocupa no limite global de 250 |
| **PAT** | Senha temporária que a caixa usa pra ler/escrever cache (morre em ≤2h) |
| **Spawn** | Ligar uma caixa nova |
| **Fabricd** | Nosso coordenador Rust (caminho alternativo ao worker TypeScript) |
| **DO** | Durable Object — "funcionário único" do Cloudflare que garante fila justa |
| **KV** | Armazenamento chave→valor simples, rápido, mas eventualmente consistente |

---

## Parte 3 — O fluxo inteiro, desenhado

```
 SEU JOB                              NOSSOS SERVIDORES                    A CAIXA
┌────────┐   webhook (1x só!)   ┌────────────────────┐    liga VM    ┌─────────────────┐
│ GitHub ├─────────────────────▶│   spawn-worker     ├──────────────▶│ Caixa (microVM)   │
│ queued │◀───── "ok" (202) ────┤   (TypeScript)     │               │                 │
└───┬────┘                      └──────┬─────────────┘               │ ① clw hydrate   │
    │                                  │ mint PAT (senha do cache)   │  (cache quente) │
    │                                  │ slot no DO (cadeira)        │ ② registra-se   │
    │      assigna o job direto        │ JIT config (registro)       │    no GitHub    │
    └──────────────────────────────────────────────────────────────▶ │ ③ roda o build  │
                                       ▼                             └───────┬─────────┘
                                container.start()                            │ termina?
                                                                             │
   CONTA:  caixa ligada ──▶ slot-segundos ──▶ usage events ──▶ D1 ──▶ Stripe ◀──┘
           (a conta começa no PICKUP do GitHub — não quando a caixa ligou!)
```

Guarde dois detalhes desse desenho — eles explicam metade dos achados:

- O aviso do GitHub chega **uma vez**. Qualquer coisa que morra no meio do caminho = job órfão.
- A conta começa quando o GitHub *pega* o job, mas a caixa liga **antes** disso. Todo minuto de fila é dinheiro nosso virando calor (M14).

---

## Parte 4 — Os 10 HIGHs, mastigados

*Ordem = gravidade × proximidade do dinheiro. Cada um abre com o problema numa frase; o mecanismo vem logo abaixo.*

### RH1 — A porta da frente está destrancada de propósito (e a corrente também não está posta)

**Em 1 frase:** existe um filtro de "só instalações aprovadas podem usar nossos runners", mas ele está deliberadamente DESLIGADO em produção (`wrangler.jsonc:77`: *"keeps the gate DISARMED"*).

Como funciona hoje: qualquer pessoa instala nosso GitHub App → cria um repo com `runs-on: corelink` → o webhook chega → a ÚNICA coisa entre o código dela e uma caixa na nossa conta é a verificação de entitlement no servidor (o "mint 403"). E se a chave dessa verificação estiver mal configurada? O código assume "tudo certo" e liga a caixa mesmo assim, no modo frio, grátis (`lib.ts:482-487` — o próprio comentário chama de *"legacy fail-open"*).

**Por que importa:** compute grátis na nossa conta depende de *uma* checagem runtime + *zero* defesas armadas.
**Correção:** ligar o filtro (o valor pronto tá documentado: `"150584374,<install-id>"`) + fazer o deploy FALHAR se o filtro ou a chave não estiverem postos. Meia hora de trabalho.

### RH2 — Uma senha única abre todas as portas

**Em 1 frase:** um token estático (`CLOUDFLARE_SPAWN_AUTH_TOKEN`) autoriza ligar caixas, **executar comandos arbitrários**, destruir coisas e cortar internet (`index.ts:675-680, 3128-3346`).

Vazou essa senha = alguém roda o que quiser na nossa conta Cloudflare. Não tem rotação, não tem escopo por função. E o guarda de imagem (`PINNED_IMAGE_DIGEST`) é opcional e está desligado (`index.ts:146-154` admite: *"OPTIONAL by construction… INERT"*).
**Correção:** uma senha por função (ligar ≠ executar ≠ destruir) — o padrão já existe no código para outro domínio — + ligar o pin de imagem.

### RH3 — Quando o contador engasga, ele libera geral

**Em 1 frase:** se o funcionário que controla as cadeiras (o DO) der qualquer erro, o código decide *"admitir mesmo assim"* (`index.ts:1600-1617`).

Parece gentil, mas: erro de DO acontece exatamente quando todo mundo bate de uma vez (o burst É a carga). Nesses momentos, **nem o limite global de 250, nem o limite pago de cada tenant são aplicados** — todo mundo entra. O único freio que resta é o teto físico da plataforma.
**Correção:** clientes PAGOS → erro = recusar com bilhete de resgate (fail-closed); modo frio pode continuar generoso. + circuit-breaker: N erros seguidos = parar de admitir até o DO voltar.

### RH4 — Dentro da caixa, o job do cliente é root

**Em 1 frase:** a imagem dá ao usuário `runner` poder de `sudo` TOTAL sem senha (`Dockerfile:515`: `"runner ALL=(ALL) NOPASSWD:ALL"`).

O job do cliente (código não-confiável por definição) vira root da própria caixa. Root pode: espiar a memória do agente do GitHub (roubar segredos do job), ler variáveis de ambiente (pegar a senha do cache), e **trocar o binário `clw` pelo dele** — fazendo nossa própria infraestrutura gravar artefatos venenosos no cache do tenant, que serão restaurados nos builds futuros *limpos* daquele mesmo cliente. O cache — nosso argumento de venda — virando arma contra o próprio usuário.
**Matiz honesta:** remover sudo quebra CI legítimo (ecossistema Actions espera sudo). Correção honesta: sudoers ESCOPADO (lista de comandos) ou parar de alegar "non-root" como defesa nos docs — porque hoje o boundary real é só a microVM.

### RH5 — A saída da internet é compartilhada e sem regras

**Em 1 frase:** a caixa tem internet irrestrita e sai por UM IP compartilhado entre todos os tenants (`index.ts:553-563`; o filtro de hosts foi REMOVIDO porque quebrava o GitHub, :556-563; sockets raw nem passam pelo proxy).

Um cliente malicioso escaneia a internet / ataca gente / minera → tudo sai pelo NOSSO IP → blacklist, abuse report do provedor, egress degradado **pra todos os tenants ao mesmo tempo**.
**Matiz pós-verificação:** a preocupação com IMDS (metadata da cloud) já foi testada de verdade e fechou GO — mas os docs se contradizem sobre isso no mesmo arquivo (:561 diz "resolvido", :45 diz "ainda devemos testar").
**Correção:** allowlist de egresso na camada de rede (github.com, api.github.com, nosso API — fim da lista).

### RH6 — O cano de dinheiro entope sozinho

**Em 1 frase:** nosso lado manda os eventos de cobrança em pacotes do tamanho que forem; o servidor aceita no máximo 1024 por pacote e devolve erro 400 em qualquer coisa maior (`fabric-server/src/corelink_billing.rs:279-299` vs server `billing_ingest.rs:121`).

Consequência matemática: bastam 1025 eventos presos (uma queda do endpoint de cobrança) e **nenhum pacote volta a passar — nunca mais**. O buffer cresce até 100 mil e começa a jogar eventos fora (os mais antigos = as cobranças mais velhas). Isso já aconteceu uma vez por um motivo irmão (região errada, `corelink_billing.rs:802`) — o da região arrumaram; o do tamanho continua. De bônus, a documentação promete aceitação parcial que o servidor não faz.
**Correção:** fatiar o envio em pedaços de ≤512 (a chave de idempotência torna cada pedaço seguro de reenviar). Horas de trabalho, remove uma bomba-relógio.

### RH7 — Limite corrompido vira "sem limite"

**Em 1 frase:** se o valor do limite mensal de CPU chegar corrompido (texto, negativo, número gigante), o código trata como "não tem limite" — em silêncio (`corelink_plans.rs:131-151` → sentinel `0` → ledger pula o check, `ledger.rs:821-826`).

É o mesmo DNA do bug #432 (corrigido): um design onde um estado seguro é impossível de expressar. Aqui, "limite existe mas não consegui ler" e "não existe limite" são a MESMA coisa. E tem um bônus perverso: limite fracionário (ex.: trial de 0,5 vCPU-h) vira 0 na conversão → também vira "sem limite". Trial = compute grátis pra sempre.
**Correção:** três estados (existe / não-existe / ilegível), ilegível grita alto (log+metric) em vez de calar; suportar fração (o domínio interno já é em milissegundos!).

### RH8 — Trocar a senha principal ainda é reza manual

**Em 1 frase:** a chave de mint vive em 3 lugares, um deles só lê no boot da imagem, nenhum teste automático valida se a chave está CERTA (só se está PRESENTE), e o último incidente desse tipo (J8, 2026-07-21) está documentado em handoffs mas ausente do CHANGELOG e do inventário de secrets.

Chave errada = sistema sobe saudável, sorri, e falha silenciosamente em produção no primeiro uso.
**Correção:** self-check de boot que dá um "mint seco" de mentira e confirma a resposta (espelha o check que JÁ EXISTE pra outra chave, `server.rs:1101`) → drift vira barulho no boot, não mistério em produção.

### RH9 — Nossas respostas de erro podem desligar a campainha

**Em 1 frase:** sob burst, respondemos **429** ao GitHub; respostas não-2xx em sequência podem fazer o GitHub parar de ENTREGAR webhooks — aí param de chegar jobs, e nosso sistema nem fica sabendo (ele só vê o que chega).

Matiz pós-verificação: a regra dos "20 erros consecutivos" é documentada para hooks de REPO; para App-webhooks o comportamento não está verificado — mas o repo é silencioso sobre ambos, e o código até assume quase o oposto (`index.ts:3002`). Independente da premissa: falhas de assinatura HMAC não são contadas em metric nenhuma, e nada que existe dispara pager.
**Correção:** responder 202 nos limitados (o bilhete já é gravado) + contar falhas de auth + pager externo nos streaks.

### RH10 — Dois avisos iguais = duas caixas (e uma delas sem coleira)

**Em 1 frase:** o "já vi esse job?" usa leitura+escrita sem atomicidade no KV — e KV é eventualmente consistente entre datacenters, então dois avisos chegando em continentes diferentes dentro da janela de propagação AMBOS passam (`lib.ts:113-120`).

Resultado passo a passo (interleaving completo verificado): duas PATs mintadas, a segunda SOBRESCREVE o registro da primeira (`index.ts:1669-1673`) → duas caixas ligam → o GitHub escolhe uma → a outra dorme 15 minutos queimando nosso dinheiro OU rouba o próximo job (duplicando caixas no burst). A contagem de slots continua dizendo "1" — os números parecem certos enquanto o mundo está errado.
**Correção:** mover o claim pra dentro do DO (que é atômico de verdade e já centraliza tudo). Um round-trip fecha a raça.

---

## Parte 5 — Os MEDIUMs (tabela mastigada)

| # | Problema | Tradução humana | Onde |
|---|----------|-----------------|------|
| M1 | Suspensão não derruba job em andamento | Tenant banido continua computando até 2h | `index.ts:1625-1731` |
| M2 | Endpoint de diagnóstico sem senha nem limite | Qualquer um na internet enche nossos logs de lixo (e afoga incidentes reais) | `index.ts:3107-3122` |
| M3 | Senha do cache multi-uso + revogação engole erros | PAT vazada fica válida 2h DEPOIS do job acabar | `lib.ts:448-458` |
| M4 | Cadeira expira em 45min mas caixa dura mais ⚠️ | Contagem sub-reporta → admitimos além do fleet físico | `lib.ts:594,647` |
| M5 | Não existe fila de espera | Fleet cheio >30min = job DESCARTADO pra sempre (sem fila, sem prioridade, sem posição) | `lib.ts:1102,3541` |
| M6 | Mint de PAT antes de checar se tem cadeira | Cada retry de job stuck queima uma senha de verdade | `index.ts:1653→1678` |
| M7 | Segredo de registro passado na LINHA DE COMANDO | Visível em `/proc` pra qualquer processo da caixa | `entrypoint.sh:199` |
| M8 | Limpeza solta a cadeira ANTES de destruir a caixa | Destruição falhou? Caixa vazou pra sempre (sem retry) | `reaper.rs:946-952` |
| M9 | Recibo da caixa escrito com "se falhar, paciência" | É literalmente a origem das caixas fantasmas (RC2) | `index.ts:1274-1296` |
| M10 | Desinstalar o App não limpa nada | Mapas com mortos pra sempre (hoje safe: mint 403 barra; amanhã?) | webhooks :2789-2854 |
| M11 | Dois jeitos de saber quem é o tenant, sem dono da verdade | Incidente REAL: deploy zerou o mapa e cobramos o tenant ERRADO, zero erros | `wrangler.jsonc:67-74` |
| M12 | Recusa no modo frio = stranded sem bilhete | Job some sem estado terminal (irmão do RC3) | `index.ts:1671-1780` |
| M13 | Kill de emergência emite evento sem carimbo | Cobranças somem da reconciliação após restart | `app.rs:1465` |
| M14 | Conta começa no pickup, caixa liga antes | Minutos de fila = nosso dinheiro virando calor | `lib.ts:1626` |
| M15 | Teto de vCPU em prod é só um aviso | Proteção de receita == "será que o Stripe fatura?" | `index.ts:1447` |
| M16 | Canary observa placar, não joga o jogo | Leak sistemático de slot seria invisível | `cloudflare-canary/src/index.ts` |
| M17 | Erros fora do formato oficial de contrato | Clientes que dependem do vocabulário quebram | `cas_cred.rs:43` |
| M18 | GitHub Action interpola input direto no shell | Injeção via reusable workflows (Buildkite faz certo — copiar) | `action.yml:133-140` |
| M19 | Respostas diferentes revelam quais leases existem | Oráculo menor (ticket é de 256 bits — força bruta inviável) | `index.ts:3078-3104` |
| M20 | Inventário de secrets incompleto | 4+ secrets invisíveis pro checklist de rotação | `secret-inventory.md` |
| M21 | Fabricd só lê secrets no boot | Rotação exige rebuild de imagem + repin manual | `wrangler.jsonc:182` |
| M22 | Docs de preço se contradizem dentro do mesmo arquivo ($16/$40 vs $8/$20) + pricing.md:171 promete hard-stop que o código não faz | Quem confia na doc toma prejuído conceitual | `plans.rs:13-17,77-85` |

---

## Parte 6 — Performance: onde vão os segundos

**A jornada do job (nó quente): ~15–30 segundos entre "queued" e "rodando".**

| Etapa | Tempo | Comentário |
|-------|-------|------------|
| Webhook → verifica → ack | ~0,1s | ✅ rápido como deve |
| Mint PAT + stash + slot | ~0,1–0,5s | serial, ok |
| JIT config (registro no GitHub) | 0,2–0,8s | por tentativa |
| `container.start()` vs timeout 8s ×3 | **5–60s** | agenda + download de imagem multi-GB; pull lento = retry cega (ghost churn BY DESIGN) |
| Registro no runner + assign do GitHub | ~5–15s | upstream |
| **Bônus ridículo:** `await bumpMetrics` INLINE | ~ms | estatística travando o caminho crítico — virar fire-and-forget |

No caminho fabricd (alternativo), são **6+ idas de rede SERIAIS antes do ack** — incluindo a mesma introspecção feita DUAS vezes (a segunda ignora o cache que existe pra isso).

**Os 4 consertos de perf:**
1. Medir a distribuição do start() — se pulls estouram os 8s, estamos queimando retries em nó frio (win: dezenas de segundos no p95).
2. Fabricd: reaproveitar a introspecção + paralelizar os dois mints + provisionar async (−0,5–1,5s + mata o risco dos 10s do GitHub).
3. Sonda direta ~20–30s pós-spawn: recuperação de falha cai de MINUTOS pra segundos.
4. **Warm-pool** — o gap estratégico: manter 1–2 caixas pré-ligadas nas horas movimentadas. Em baixa concorrência, caixa dorminhoca custa quase nada; é o que torna o pitch universal (hoje ele vale pra builds pesados, não pros rápidos).

**Veredicto honesto do pitch:** spawn tax de 15–30s **perde pra pools pré-aquentados dos hosted runners (~5–15s)** em jobs pequenos — o cache não paga isso num build de 2 minutos. Segura firme onde importa (compile-heavy Bazel/Rust, 8min→3min). Com warm-pool, vira universal.

---

## Parte 7 — Economia: de onde vem cada centavo, onde vaza

**O que chargeamos** (ratificado): planos de $16 a $400/mês com franquia de vCPU-h; efetivo **~$0,67 por slot-hora**; excedente a $0,30/vCPU-h (**$1,20/slot-h** — 3× o custo).

**O que custamos:** base documentada Northflank $0,10/vCPU-h = **$0,40/slot-h** (worst case). Margem incluso: ~40%. Excedente: 67%.
⚠️ **A taxa REAL do Cloudflare Containers não existe em lugar nenhum do repo** — toda margem herda o proxy do Northflank.

**Custo fixo freeload:** dominado pelo fabricd singleton sempre-ligado (standard-2 ≈ 1.460 vCPU-h/mês × $0,10 = **~$145/mês proxy**) + check-host std-4 se algum dia aquecer (**≤~$290/mês** — hoje em 0 instâncias; um audit anterior pegou 4 caixas ociosas cobradas por 1 mês). ⚠️Correção aritmética vs rascunho: os valores iniciais (~$15/$29) estavam 10× abaixo — o proxy de custo torna esses itens MATERIAL, não cosmético.

**Por onde o dinheiro vaza (ranqueado):**
1. **RC1** — jobs >2h: cada hora além da 2ª numa caixa de 4 vCPU ≈ **$1,20 nunca cobrada**; um job de 6h deixa ~$4,80 na mesa — e nada no nosso código limita a duração (keepalive mantém viva), então o teto do prejuízo é a paciência do cliente
2. **RH7/M15** — ceiling corrupto/fracionário/unset = sem teto; overage depende do Stripe faturar (nunca verificado end-to-end!)
3. **RC2** — fantasmas: ~$12/incidente medido, recorrente até existir kill-path
4. **RH10** — corrida de claim: ~2× custo por job corrido durante bursts
5. **M14** — fila não-faturável (nosso COGS)
6. **Cold spawns** — compute dado de graça dentro do cap de 40 repos

**O ÚNICO gesto que melhora tudo: flipar `max_vcpu_h` no server + verificar que a PRIMEIRA invoice do overage sai.** Toda a maquinaria de proteção já existe e está esperando esse ato de configuração. Runner-up: fila com posição visível — transforma o cap de 250 de "churn machine no momento de sucesso" em upsell.

---

## Parte 8 — O que está EXCELENTE (não quebrar)

- **Isolamento por microVM**: cada lease é uma VM própria — blast radius de memória/CPU/disk do job malicioso = a própria caixa. Cross-tenant read/write fechado.
- **Stack anti-zumbi profunda**: PID1 trap + escalonamento SIGKILL + idle-backstop durável (stop→destroy ×2) + reap só com veredito do GitHub + ghost-confirm sweep. A disciplina pós-incidente-2026-08-02 (nunca matar por lookup de job; exigir match de runner_id) é textbook.
- **Cripto correta em todo lugar**: HMAC constante-time, ticket de 256 bits com comparação constante-time, PAT nunca logada (tipos Rust com Debug redigido).
- **Deploy higiênico**: TODAS as imagens pinadas por digest (@sha256, tarballs com SHA256 no Dockerfile); deploys manuais com gate de testes + gate de frota-ociosa (*ignorância ≠ idle* — direção certa).
- **Cultura de conformance rara**: vetores byte-exact triplicados (Rust+Python+TS), lens estrita separada do parser tolerante de propósito, golden files hash-pinned.
- **Higiene de código**: zero `todo!`/`unimplemented!` em código de verdade; 33 unwrap/expect em paths de produção — todos invariant-documentados ou boot-only; deps workspace exatas (`=`); ~1.300 testes; Buildkite hook exemplar.

---

## Parte 9 — As 5 lições sistêmicas (pra não reciclar esses bugs)

1. **Timer curto guardando fato permanente** (RC1): TTL de 2h numa identidade que precisa sobreviver ao job. Regra: TTL deve cobrir o PESSIMO caso medido, não o caso feliz imaginado.
2. **Resposta "feito!" sem ter feito** (RC2): 204 de teardown que destrói nada é pior que erro — ensina o operador a confiar no mentiroso. Regra: ação destrutiva sem confirmação de efeito = retornar erro.
3. **Promessa pós-ack sem bilhete prévio** (RC3): "processo em background" precisa de write-ahead ANTES de começar, não catch DEPOIS que deu ruim. Regra de ouro: se o aviso externo não repete, o bilhete vem primeiro.
4. **Sentinel colidindo com significado** (RH7): `0` significando "sem limite" E "falhei ao ler" é o mesmo DNA do bug #432. Regra: estados legítimos precisam de representações distintas.
5. **Docs contando história diferente do código** (RH9, M22, RH5, index.ts:856): quatro vezes nesta auditoria o comentário/doc estava errado CONTRA o código. Regra: claim de doc sobre comportamento crítico merece teste que o prenda.

---

## Parte 10 — Plano de execução (com o porquê da ordem)

**Fase 0 — HOJE (dinheiro + trancar a porta):**
1. RC1: TTL do `jtenant:` → 60 dias *(uma linha; para o leak que escala)*
2. RH1: armar INSTALLATION_ALLOWLIST + deploy fail-closed sem ela
3. RH6: fatiar billing flush ≤512 *(remove a bomba-relógio)*
4. RH7: parse 3-estado + suporte fracionário
5. **Flipar `max_vcpu_h` no server + verificar 1ª invoice Stripe**

**Fase 1 — esta semana (perda de job + integridade do fleet):**
6. RC3: bilhete write-ahead antes do spawn
7. RH9: 429→202 + counters + pager externo
8. RH3: DO fail-CLOSED pra clientes pagos (+ circuit-breaker)
9. RH10: claim dentro do DO
10. Assinar ADR-0010 (nomes endereçáveis = cura estrutural dos fantasmas)

**Fase 2 — sandbox e egresso (antes de tráfego externo):**
11. RH4: sudoers escopado (ou parar de alegar non-root) · RH5: allowlist de rede
12. M2 diag autenticado · M3 revoke fail-loud · M7 segredo fora do argv · M8 tombstone de leak
13. RH8: self-check de boot da mint key + inventário completo

**Fase 3 — estrutura:**
14. Warm-pool · fabricd chain otimizada · medição do image-start
15. Fila durável com posição (portar o pg_queue que já existe e está na gaveta) · heartbeat de slot (M4)
16. Handler de uninstall (M10) · self-check do mapa deployado (M11) · synthetic job no canary (M16)

**Fase 4 — contínuo:** docs truth-pass · taxa real do CF medida · memoize hit-rate instrumentado (fundamentar o "85–95%" que hoje ninguém mediu)

---

## Apêndice — Contagem e confiabilidade

| Severidade | N | Status pós-triple-check |
|---|---|--------------------------|
| Críticos | 3 | confirmados (RC1 e RC3 com argumento afiado) |
| High | 10 | 7 integrais · 3 com afinação (RH5: IMDS provado fechado · RH7: framing refiado · RH9: premissa App-vs-repo-hook não-verificada) |
| Medium | 22 | confirmados · M4 com premissa empírica pendente · M5/M11 com path/linha corrigidos |
| Low | ~10 | spot-checked 4/4 ✅ |
| Perf/Economia | waterfall + margens | refs verificadas; tensão nova pricing.md:171 incorporada; freeload re-calculado (10× acima do rascunho) |

**~60 claims verificados: 52 integrais · 6 afinados · 2 refs corrigidas · 0 refutados.**
Auditoria: 10 agents read-only @ `e228249` + 6 verificadores + confirmação manual. Exclusões: bugs já corrigidos documentados no CHANGELOG (#432, #433). Relatório técnico denso: `comprehensive-audit.md` (mesmo diretório).