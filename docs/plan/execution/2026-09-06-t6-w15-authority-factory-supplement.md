# T6-W15 — suplemento vinculante: authorities, factory e aceite mínimo

Base de implementação: `c2533b0cd36f020de8cd06f903b582bbf1b8a209`.
Este suplemento complementa `2026-09-06-t6-w15-services-decision.md` e encerra
a segunda submissão: as autoridades abaixo são implementações concretas usando
os recursos já aprovados, sem novo serviço durável.

## Correções obrigatórias de interface

1. `SignerRegistryAuthority` deve persistir dois roots distintos:

```ts
type RegistryHead = {
  version: "1"; generation: number; manifestDigest: string;
  manifest: SignerRotationManifest; activationReceipt: AuditReceipt;
  auditSequence: number; auditRoot: string;
  manifestWitnessSequence: number; manifestWitnessPreviousRoot: string;
  manifestWitnessRoot: string;
  observedWitnessSequence: number; observedWitnessCheckpointRoot: string;
  observedWitnessRoot: string;
}
```

`manifestWitnessRoot === manifest.witness_root_digest` e é o witness do
`presealReceipt`; `observedWitnessRoot` é o resultado do segundo
`readHead(nonce)` e nunca deve ser comparado ao root embutido no manifest.
`current().highWater` expõe ambos. O código atual que grava
`witnessRoot: witness2.witnessRoot` e depois exige igualdade com o root do
manifest está incorreto e deve ser substituído.

2. Os campos do manifest vêm exclusivamente do preseal receipt:

```ts
witness_checkpoint_sequence: preseal.checkpoint.sequence
witness_previous_root_digest: preseal.witnessReceipt.previousWitnessRoot
witness_root_digest: preseal.witnessRoot
```

Não usar `readHead` para preencher esses três campos e não usar o record de
custody fabricado. `custody.read()` deve retornar um record cujo `keyId`,
`epoch`, `role`, digest e receipt são verificados; `custody.resolve(record)`
recebe exatamente esse objeto. Para assinar o manifest, o record deve ter
`role:"manifest"` e identidade igual ao issuer. `recovery_custody_digest`
nomeia a custody independente que permite recuperar essa identidade; não é uma
chave privada nem autorização implícita.

3. `SignerRegistryAuthority` exporta também:

```ts
isRevoked(input: {
  keyId: string; epoch: string; role: TokenRole; manifestDigest: string
}): Promise<boolean>;
```

Ela só consulta o conjunto revogado do manifest indicado, depois de verificar
o manifest atual e seu receipt; digest diferente do atual é erro. Recovery deve
usar `isRevoked` para provar que o signer original foi revogado. Falha de
identidade, record ou receipt é recusa, nunca `false` silencioso.

4. `RecoveryRequest` mantém `envelope` e `original_ack_digest`, mas a chave
idempotente é calculada **depois** de `registry.current()`:

```text
<ns>:ack-recovery:<sha256([event_id, producer_seq,
  original_ack_digest, currentManifestDigest])>
```

`current_monitor_rearm_tuple_digest` vem do manifest atual; o original vem do
commit persistido. O valor durável guarda `intentReceipt` e `resultReceipt`, e
replay verifica ambos antes de devolver o token. Não existe recovery estável
atravessando manifests diferentes.

5. `page_ack` reserva a chave como `{status:"pending",fingerprint}`. Se houver
pending após crash, o serviço verifica o intent existente e retoma; se houver
result receipt sem CAS final, verifica o result e somente repete o CAS final.
Não cria novo intent/token para o mesmo fingerprint. O valor completed guarda
`intentReceipt`, `resultReceipt`, token e fingerprint. O CAS final inclui o
incident com `humanAcknowledgedAt`; conflito recarrega e só devolve sucesso se
o registro completed for byte-idêntico.

## Persistência concreta das três authorities

Todas usam `MonitorStateStore` sobre a tabela DynamoDB existente. `get` é
`ConsistentRead:true`; toda instalação é `transact` com `expectedVersion:null`
e toda atualização exige a versão lida. Nunca há `PutItem`/overwrite direto.

| Authority | chave `SK` | valor exato | escrita |
|---|---|---|---|
| IdentityDirectory | `signer-identity:<role>:<sha256([keyId,epoch])>` | `{version:"1", identity, identityDigest, configDigest}` | `register` CAS nulo uma vez; conflito só é aceitável se bytes forem iguais |
| RevocationAuthority | `signer-revocation:<digest>` | `{version:"1", digest, entries:sorted, receipt, recordDigest}` | `install` grava receipt WORM antes; CAS nulo, nunca update |
| SigningCustody | `signer-custody:<digest>` | `{version:"1", digest, keyId, epoch, role:"manifest"|"recovery"|"page-ack", keyArn, receipt, custodyVersion:"1"}` | `install` grava receipt WORM antes; CAS nulo, nunca update |

O `PK` é sempre `config.stateNamespace`; a chave é delimitada pelo prefixo
correspondente. `identityDigest = sha256(canonicalJSON(identity))`.
`recordDigest` de revogação é `sha256(canonicalJSON({version:"1",digest,entries}))`.
Entries são ordenadas por `(role,keyId,epoch,revokedAt,reason)`, sem duplicatas.
Custody digest é o digest do record sem `receipt`. Cada `read` faz
`store.get` consistente, valida todos os atributos, chama `audit.verify(receipt)`
e lê o journal receipt; o payload deve ser exatamente:

```text
REVOCATION_RECORD: {type:"REVOCATION_RECORD",digest,entries}
CUSTODY_RECORD:    {type:"CUSTODY_RECORD",digest,keyId,epoch,role,keyArn,custodyVersion:"1"}
```

O IdentityDirectory é público e version-bound: no genesis, `register` instala
todos os `config.signerIdentities`; rotação só pode instalar previamente a
identidade `next`. Ausência, alteração de PEM/ARN/role/epoch ou conflito
divergente falha fechado. Revogar não apaga a identidade histórica: ela é
necessária para verificar o ACK antigo.

Genesis exige, em uma única operação administrativa/test fixture, tabela e
journal sem `signer-registry:head`, `signer-revocation:*` ou
`signer-custody:*`, e os env vars version-bound:

```text
MONITOR_BOOTSTRAP_MANIFEST_ISSUER_KEY_ID
MONITOR_BOOTSTRAP_MANIFEST_ISSUER_EPOCH
MONITOR_BOOTSTRAP_REVOCATION_DIGEST
MONITOR_BOOTSTRAP_CUSTODY_DIGEST
```

O issuer precisa existir em `config.signerIdentities` com role `manifest`; os
records de revogação/custody e todas as identidades são instalados por CAS
antes de `rotate`. O primeiro manifest só é criado se o digest anterior for 64
zeros e a auditoria confirmar journal vazio. Depois do genesis, ausência desses
env vars é obrigatória; eles não podem rearmar ou substituir o head.

Rotação/revogação é append-only: provisionar identidade/custody e revocation
record, obter seus receipts WORM, então submeter uma única `ManifestProposal`.
`rotate` valida issuer atual, generation+1, previous digest, tuple, overlap,
revocation/custody e roots; concorrentes perdem por CAS. Nenhuma rota HTTP ou
tick executa `install`/genesis.

## Adapters AWS e least privilege

Não se cria adapter ou banco novo. A composição usa:

```text
DynamoDbStateStore(DynamoDBClient, config.stateTable, config.stateNamespace)
S3ImmutableJournal(S3Client, config.journalBucket, config.journalPrefix,
                   config.journalRetentionMs)
DurableAuditLog(store, journal, trustedClock, journal AwsKmsSigner,
                LambdaWitnessClient)
AwsSourceSecrets(SecretsManagerClient)
AwsKmsSigner(KMSClient, identity)       # manifest/recovery/page-ack/ingest-ack
LambdaWitnessClient(LambdaClient, config.witnessFunctionArn, journal+witness ids)
SnsAlertTransport(SNSClient, config.snsTopicArn, config.destination)
```

`SigningCustody.resolve(record)` só constrói `AwsKmsSigner` com o `keyArn` do
record, valida KMS `RSA_3072/SIGN_VERIFY` e compara o public key retornado ao
PEM da identidade. Nunca lê segredo privado. Recovery/page-ack resolvem o
signer KMS pela identidade devolvida por `registry.authorize`; não escolhem ARN
por request. `CryptoNonceSource` usa `randomBytes(32)` e cada `readHead` recebe
nonce novo; o witness Lambda continua independente do registry.

`MONITOR_CONFIG_JSON` é obrigatório e é a única configuração sem fallback;
contém os ARNs públicos, identities, source secret ARNs, tuple, namespace,
bucket, table, witness ARN e trusted-time fields já previstos no schema. Os
quatro bootstrap env vars acima só existem na execução one-shot de genesis.
`MONITOR_AUDIT_LOG_ID` é obrigatório e igual no journal/audit/witness.

O `RuntimeRole` recebe somente:

```text
dynamodb:GetItem, dynamodb:Query, dynamodb:TransactWriteItems
  -> StateTable
s3:GetObject, s3:PutObject, s3:GetObjectRetention, s3:PutObjectRetention
s3:ListBucketVersions -> JournalBucket e prefixo journal/*
secretsmanager:GetSecretValue -> somente os ARNs configurados de sources
lambda:InvokeFunction -> somente WitnessFunctionArn qualificado
kms:GetPublicKey,kms:Sign -> somente IngestKey/AckKey/RecoveryKey/
  PageAckKey/SigningKey correspondentes
sns:Publish -> somente ReceiptTopic
logs:CreateLogGroup,CreateLogStream,PutLogEvents -> logs
```

Não conceder `dynamodb:Scan`, `DeleteItem`, `UpdateItem`, KMS decrypt/data-key,
SecretsManager list, Lambda `:*`, ou SNS subscribe. O role de Scheduler só
invoca o alias live e envia ao DLQ; o role witness permanece no account verifier
e a permission cross-account permite apenas a versão Lambda qualificada.

## Factory singleton e rotas

`src/index.ts` deve substituir o `handler` que hoje lança
“runtime is not initialized” por `getRuntime()` lazy singleton. A factory faz
esta ordem, uma vez por processo: `loadConfig()` → clients AWS → state/journal
→ trusted time/floor → witness client → audit log → identity/revocation/custody
authorities → `AwsSourceSecrets` → promoted `IngestService` → outbox/SNS →
`MonitorScheduler` → `AckRecoveryService` → `PageAckService` → `createHandler`.
Falha de qualquer etapa impede toda resposta 2xx. Não usar `MemoryStateStore`,
signer fixture, `Date.now`, callback autorizador verdadeiro ou defaults em
produção.

Bindings permanecem exatos: `POST /v1/ingest`, `POST /v1/ack/recovery` e
`POST /v1/incidents/{incidentId}/pages/{pageId}/ack`; API Gateway passa
`rawPath`, método, body e Authorization sem logar segredo. Scheduler recebe
exatamente `{kind:"monitor_tick",source:"aws-scheduler"}`, chama `clock.now`
e `scheduler.run(now)`. Somente scheduler drena outbox/SNS; recovery e page ACK
não publicam alertas.

## Fixtures e testes mínimos obrigatórios

Fixtures usam `MemoryStateStore`, fake WORM/audit, RSA-3072 reais e identities,
ARNs, tuples, nonces e source IDs distintos de produção; não alteram flags,
timers, credentials ou high-waters live.

| Área | Testes mínimos |
|---|---|
| Registry | genesis CAS único; restart/current; duas rotações concorrentes (1 commit); generation/sequence/root rollback; manifest fork e receipt/payload/PEM/custody corruption; active/next overlap; revocation/current authorization; `CryptoNonceSource` non-replay; stale witness; manifest root separado de observed witness root |
| Recovery | original ACK revogado e assinatura histórica; current recovery signer; missing/divergent commit; payload/receipt substitution; duplicate byte-identical; manifest drift key separation; crash após intent, assinatura, result e antes do CAS; zero mudança de cursor/incident/outbox/sequence |
| Page ACK | auth/schedule positivo e negativo; expiry; page/delivery/destination/payload/action substitution; concurrent ACK; crash pending/result/final CAS; duplicate stable; `humanAcknowledgedAt` único e zero SNS |
| Runtime | factory singleton/wiring do ingest promovido; config/permission fail-closed; três rotas/status; scheduler exact two-field tick; unknown route/event; no local clock/defaults; concrete KMS/witness/secret command args |

Executar somente esses testes focados e `npm run typecheck` no owner. CI pesada,
deploy e probe production continuam no gate do bundle.
