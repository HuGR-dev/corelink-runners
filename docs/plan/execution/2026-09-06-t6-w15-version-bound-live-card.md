# T6-W15 — card de execução version-bound e live

Status: **PRONTO PARA EXECUÇÃO, NÃO É EVIDÊNCIA LIVE**. Código requerido:
43b3f4db3586ca2af5dcee4c45d51890e103c0bd. Este card não autoriza deploy,
mudança de credencial, injeção ou ACK; ele fixa a ordem, inputs e registros
que a execução autorizada deve produzir. Nenhum ARN, id de stack, segredo,
imagem, versão Lambda, identidade humana, digest ou timestamp ausente é
inferido aqui.

O objetivo é o detector externo de A6.10. A base não recebe crédito por este
card: o crédito requer as três injeções reais e os artefatos abaixo. O período
de sete dias e o correlator/provider final pertencem a T6-W12/T6-W10, não a
esta execução.

## Pré-condições imutáveis

1. O checkout usado para construir a imagem resolve exatamente para o SHA
   acima; registrar SHA, árvore limpa, digest da imagem e resultado da
   verificação de assinatura do artefato.
2. A imagem é construída por deploy/cost-monitor/Containerfile, publicada em
   ECR com digest sha256:<digest-real> e passada a CloudFormation somente como
   ImageUri=<registry>/<repository>@sha256:<digest-real>. Tags são recusadas
   pela template.
3. O operador tem valores emitidos pelos stacks e credenciais independentes
   para monitor, verifier e scheduler/control. Não substituir WitnessFunctionArn,
   SourceSecretArns, identities, trust roots ou MONITOR_CONFIG_JSON por
   fixtures.
4. Validar previamente que MONITOR_CONFIG_JSON passa validateConfig, contém
   journalPrefix compatível com o prefixo journal/ autorizado no bucket, e
   identifica contas distintas em monitorAccountId, sensitivityAccountId e
   verifierAccountId.

## Ordem de CloudFormation e inputs

Aplicar cada mudança como change set revisável. Registrar para cada etapa
nome/ID do stack, change-set ID, template SHA, parâmetros redigidos, eventos de
stack e outputs efetivos.

1. O witness verifier já deve existir e expor Lambda qualificada por versão, no
   formato aceito por WitnessFunctionArn. Registrar ARN, versão e identidade
   pública de witness. Não apontar para $LATEST.
2. Criar ou atualizar monitor-foundation.yaml antes do runtime. Inputs:

   | Parâmetro | Valor de execução exigido |
   | --- | --- |
   | ResourcePrefix | prefixo real aprovado |
   | WitnessFunctionArn | ARN versionado do verifier independente |
   | SourceSecretArns | lista exata dos ARNs dos segredos de source |

   Capturar StateTableName, JournalBucketName, JournalKeyArn, SigningKeyArn,
   IngestKeyArn, AckKeyArn, RecoveryKeyArn, PageAckKeyArn, RuntimeRoleArn,
   SchedulerRoleArn, SchedulerDeadLetterQueueArn e ReceiptTopicArn. A policy
   do RuntimeRole deve continuar limitada a Dynamo GetItem/Query/
   TransactWriteItems, s3:ListBucketVersions no bucket com s3:prefix=journal/*,
   objetos journal/*, chaves KMS nomeadas, witness nomeado e segredos nomeados.
   Registrar a policy efetiva.
3. Criar ou atualizar monitor-runtime.yaml depois de confirmar outputs. Preencher
   sem omissão:

   | Parâmetro | Fonte |
   | --- | --- |
   | ResourcePrefix, ImageUri | prefixo aprovado; digest imutável da imagem |
   | FoundationTableName, FoundationBucketArn, FoundationTopicArn | foundation |
   | RuntimeRoleArn, SchedulerRoleArn, JournalKeyArn, SigningKeyArn, SchedulerDeadLetterQueueArn | foundation |
   | SensitivitySchedulerRoleArn | principal de scheduler/control separado |
   | MonitorConfigJson | JSON validado, sem segredo em logs ou evidência |
   | AuditLogId | identificador de log WORM aprovado |
   | OnCallAccountIds, PageAckMaxSkewMs, PageAckTtlMs | rotação e limites aprovados |

   PublishedVersion e LiveAlias devem referenciar a imagem digestada. Registrar
   PublishedVersion.Version, LiveAliasArn, FunctionArn, ApiEndpoint, ScheduleArn
   e o payload literal do schedule:
   {"kind":"monitor_tick","source":"aws-scheduler"}. O runtime recusa terceiro
   campo e HTTP sem payload API Gateway 2.0.
4. Antes de prova, consultar a configuração efetiva da Lambda. Ela deve conter
   os bindings MONITOR_CONFIG_JSON, MONITOR_AUDIT_LOG_ID, MONITOR_API_ID,
   MONITOR_API_STAGE, MONITOR_ONCALL_ACCOUNT_IDS,
   MONITOR_PAGE_ACK_MAX_SKEW_MS e MONITOR_PAGE_ACK_TTL_MS. Registrar nomes e
   hashes/redações seguras; nunca valores secretos.

## Bootstrap durável antes do primeiro tick

Não há default, chave embutida ou bootstrap HTTP. A ferramenta aprovada usa o
mesmo DynamoDbStateStore, journal WORM e witness independente do runtime e
registra cada receipt.

1. Registrar no diretório Dynamo identidades públicas separadas para journal,
   witness, ingest-ack, recovery, manifest e page-ack. Cada uma precisa de
   keyId, epoch, keyArn, PEM e role exatamente configurados.
2. Escrever e read-after-write verificar o registro de revogação canônico e
   receipt WORM. Instalar cada custody record
   {digest,keyId,epoch,role,keyArn,receipt} somente após verificar o payload
   CUSTODY_RECORD. Custody do issuer de manifest deve ter mesmo
   role/key/epoch/keyArn da identidade de manifest; mesma PEM com outro ARN é
   falha.
3. Criar genesis/rotação pelo fluxo preseal → manifest assinado → activation
   WORM → CAS. Guardar generation, manifest digest, activation receipt,
   roots/seqüências de checkpoint e witness. Falha de nonce, root, receipt,
   revogação ou CAS interrompe bootstrap.
4. Instalar em Dynamo schedule atual e membros on-call com digest, destino,
   action ACK, notBefore e expiresAt. Fazer leitura consistente e registrar
   somente hashes dos principals, nunca credenciais.
5. Executar um tick permitido e preservar request/response sanitizado, prova de
   tempo confiável, checkpoint, receipts e cursor. Falta de configuração,
   relógio, registry, receipt ou secret deve falhar fechado.

## Provas A6.10 — três injeções independentes

Cada injeção usa a mesma versão Lambda, image digest, monitor tuple, source
registry digest e schedule ARN registrados acima. A janela começa no checkpoint
confiável agendado e cada caso só aprova com página externa até 120 s desse
checkpoint. Para cada caso guardar timestamps confiáveis, ID de
incident/delivery/page, tuple digest, estado/receipts WORM, versão da função e
evidência independente de entrega.

| Caso | Injeção controlada | Critério de sucesso e recuperação |
| --- | --- | --- |
| 1 | parar o canary produtor | tick esperado ausente; detector externo abre incident e página até 120 s; restaurar sem resetar relógio do tick ausente |
| 2 | falhar entrega de CANARY_CONFIG_INVALID | detector encontra sinal/config esperado ausente e página até 120 s; restaurar rota e provar que duplicata não cria segundo efeito |
| 3 | falhar credencial ou caminho canary→monitor | detector externo encontra ausência até 120 s e página; restaurar somente credencial/caminho aprovado |

Nenhum autoestado do canary é evidência. Repetir até três execuções
independentes aprovadas por caso; execução tardia, ambígua, sem receipt ou sem
página é falha, não sucesso parcial.

## ACK humano, crash, rotação e isolamento

Para cada página da prova, on-call autorizado chama apenas
POST /v1/incidents/{incidentId}/pages/{pageId}/ack com API Gateway IAM, SigV4,
x-amz-date e x-idempotency-key canônico. Registrar status, digest do body e
page ACK token/receipt em cofre de evidência; nunca Authorization, token
completo ou payload bruto. Confirmar que identidade fora da schedule,
body/page/delivery alterado, token expirado ou replay é recusado e que ACK não
fecha incident nem publica delivery novo.

Executar e preservar uma vez para cada cenário:

- interrupção após intent, após assinatura/token e após result antes de CAS de
  ACK recovery e page ACK; reinício retoma mesmo estado/resultado, sem segundo
  efeito;
- rotação de signer e revogação do original; recovery só emite token depois de
  provar ACK original, intent/result original e revogação durável;
- custody com ARN diferente da identidade configurada, mesmo PEM; deve falhar;
- acesso cruzado entre três contas ou Lambda/secret/witness fora dos ARNs
  permitidos; deve ser negado e não alterar estado.

## Evidência WORM, tempo confiável e rollback

Para cada operação externa, guardar WRITE_AHEAD_INTENT, result, receipt de
journal imutável, versão de objeto/retenção, checkpoint assinado, witness
independente, roots/seqüências e prova RFC3161. Verificar ListObjectVersions no
prefixo journal/, read-after-write de cada receipt e ausência de pending intent
antes de declarar caso concluído. Relógio local não compõe SLO ou evidência.

Se falhar build, change set, bootstrap, trusted time, WORM, registry, ACK,
injeção ou isolamento, parar prova e preservar receipts. Não apagar Dynamo,
bucket versionado/Object Lock, manifests, custody, revogações ou evidência. O
rollback de código é novo change set para o digest de imagem imutável anterior,
que publica a versão correspondente e move LiveAlias somente após verificar
compatibilidade de registry e monitor tuple; manifest/epoch não sofrem rollback.
Se isso não puder ser provado, desabilitar a schedule por change set aprovado e
preservar estado para investigação.
