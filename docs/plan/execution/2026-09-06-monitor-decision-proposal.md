# Proposta de decisão — contrato verificável do monitor externo

**Estado: proposta, não autorizada nem aplicada.** Não libera produção, PG ou
qualquer WP. As contas AWS já provisionadas permanecem sem aplicação de monitor.

## O bloqueio concreto

O plano atual exige, antes de escrever o monitor, um transporte de alertas com
consulta por operação idempotente e cursor monotônico de recibos do provedor.
Também exige prova criptográfica exaustiva de que o provedor não omitiu nenhuma
operação ou apresentou visões divergentes. Ver
`2026-09-01-round3-remediation-delta.md:168–202` e
`2026-08-30-golive-remediation-plan.md:958–977`.

As APIs avaliadas ainda não qualificam esse contrato. SNS/SQS não permite
reconciliar historicamente uma resposta de exclusão perdida; sua deduplicação é
limitada a cinco minutos. O Google Chat permite IDs definidos pelo cliente e
consulta posterior, mas não documenta o cursor monotônico de recibos nem a prova
de completude exigida. Gravar nossos próprios hashes não cria essa capacidade no
transporte. A revisão está em
`docs/plan/evidence/O-MONITORHOST-capability-review-20260906.md`.

## Alteração proposta para aprovação do proprietário

Adotar AWS Lambda, EventBridge Scheduler, DynamoDB, SNS e S3 Object Lock em
modo Compliance nas três contas independentes já provisionadas. O contrato de
entrega seria **ao menos uma vez**, com identidade estável de incidente/operação.
Páginas duplicadas são possíveis após resultados ambíguos; o sistema não as
descreverá como entrega exatamente uma vez.

Substituir somente estas duas exigências do transporte:

1. O cursor de recibos e a identidade das operações serão mantidos no registro
   transacional do monitor e reconciliados pelo verificador independente com os
   recibos disponíveis do transporte. Esse cursor será identificado como nosso,
   nunca como um cursor exaustivo emitido pelo provedor.
2. A cadeia assinada, o armazenamento WORM e a testemunha independente provarão a
   integridade do registro observado. Não afirmarão provar a ausência de uma
   operação interna do SNS que sua API não permite consultar.

Qualquer resultado ambíguo permanecerá `UNKNOWN`, com obrigação durável de retry
e incidente operacional; não poderá produzir atestado verde de saúde ou liberar
PG. Uma aceitação da API SNS provará apenas aceitação pelo transporte, sem
afirmar leitura humana. A confirmação humana continuará autenticada e vinculada
ao incidente pelo protocolo de ACK. O canal e os testes usarão o endereço do
proprietário fornecido nesta sessão; nenhum envio foi realizado nesta proposta.

## O que continua obrigatório

- Três contas e credenciais separadas para monitor, sensibilidade e verificação;
  monitor fora de Cloudflare e verificador com acesso somente de leitura.
- Registro transacional de incidente e outbox antes dos efeitos, recuperação de
  crashes, repetição da mesma operação, limites de fila e preservação de evidência.
- Retenção WORM de pelo menos oito dias, cadeia assinada e testemunha independente.
- Qualificação da autoridade de tempo/checkpoint confiável; relógio local ou
  cabeçalho HTTP não vira prova por esta proposta.
- Testes de interrupção e recuperação, rotação de credenciais, isolamento,
  latência e entrega conforme os limites do plano; falhas continuam vermelhas.
- As duas verificações completas do monitor na versão final, todos os gates de
  PG e os sete dias contínuos de observação. Não há dispensa de espera real.

## Implementação após a decisão

Registrar a resposta literal do proprietário e o escopo aprovado, alterar de
forma consistente o contrato e os testes que dependem dessas duas garantias,
concluir a qualificação restante e só então implementar o monitor. A proposta
não autoriza inventar recibos, ocultar incidentes ou marcar o backlog como entregue.

Se o contrato original for mantido, a busca por uma combinação documentada que
o cumpra continua sendo um predecessor obrigatório; a aplicação do monitor e
seus sucessores permanecem bloqueados. As correções independentes continuam.
