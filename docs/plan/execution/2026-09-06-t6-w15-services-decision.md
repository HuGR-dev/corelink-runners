# T6-W15 — contrato fechado dos quatro serviços restantes

Status: **DECIDIDO PARA IMPLEMENTAÇÃO**.  Base: `ca0d9f5db2d2e8407e12179a57e5d6d2ca55eb59`.
Escopo: `registry/manifest authority` durável, `ack_recovery`, `page_ack` e runtime
principal em `deploy/cost-monitor`. Este registro decide a integração entre os
codecs já existentes e os serviços; não altera os wire formats congelados.

Fontes normativas usadas: `docs/plan/2026-09-01-round3-remediation-delta.md`
(T6-W15, ACK/RECOVERY, manifest e page ACK),
`docs/plan/execution/2026-09-06-monitor-token-contract.md`,
`deploy/cost-monitor/src/{state,evidence_log,journal,witness,acks,signer_manifest,
recovery_token,page_ack_token,ingest,outbox,scheduler,types}.ts` e
`deploy/cost-monitor/config.schema.json`.

## Decisão comum

O único estado mutável de produção é `MonitorStateStore`; o armazenamento é
`DynamoDbStateStore` com `ConsistentRead` e `transact` condicional. A
`MemoryStateStore` só pode aparecer em testes. Toda leitura de estado inválida,
rollback, fork, resultado ambíguo ou relógio não confiável é erro fail-closed;
nenhum serviço restaura estado a partir de defaults ou cache em memória.

O namespace recebe quatro famílias de chaves:

```text
<ns>:signer-registry:head
<ns>:ack-recovery:<sha256(event_id,producer_seq,original_ack_digest,manifest_digest)>
<ns>:page-ack:<sha256(incident_id,page_id,delivery_id,destination)>
<ns>:runtime:singleton          # somente diagnóstico; nunca autoridade
```

O digest de uma tupla é SHA-256 de `JSON.stringify` do array na ordem do
contrato. Payload de auditoria usa `canonicalJSON` de `journal.ts`. A prova de
payload não é o `AuditReceipt` sozinho: o serviço chama `audit.verify(receipt)`,
lê `journal.read(receipt.journalReceipt)` e compara
`canonicalJSON(record.payload)` com o objeto esperado byte a byte.

Cada operação que emite um token faz `WRITE_AHEAD_INTENT`, assina somente após
o intent estar durável, faz `WRITE_AHEAD_RESULT` com o token completo e só então
faz o CAS do resultado. Um erro/timeout depois de um efeito durável retorna
`UNKNOWN`/5xx; nunca retorna sucesso presumido.

## 1. Autoridade durável de registry e manifest

### Interfaces exatas

O módulo novo é `src/signer_registry.ts` e exporta os tipos e a classe abaixo.

```ts
type TokenRole = "ingest-ack" | "recovery" | "page-ack" | "manifest";

interface FreshNonceSource { next(): string } // 64 hex; nunca reutiliza nonce

interface IdentityDirectory {
  resolve(keyId: string, epoch: string, role: TokenRole): Promise<PublicSigningIdentity>;
}

interface RevocationRecord {
  version: "1";
  digest: string;
  entries: Array<{ keyId: string; epoch: string; role: TokenRole; revokedAt: number; reason: string }>;
  receipt: AuditReceipt;
}

interface RevocationAuthority {
  read(digest: string): Promise<RevocationRecord>;
}

interface CustodyRecord {
  version: "1";
  digest: string;
  keyId: string;
  epoch: string;
  role: "manifest" | "recovery" | "page-ack";
  receipt: AuditReceipt;
}

interface SigningCustody {
  read(digest: string): Promise<CustodyRecord>;
  resolve(record: CustodyRecord): Promise<AsyncSigner>;
}

interface ManifestProposal {
  active_signer_key_id: string; active_signer_epoch: string;
  next_signer_key_id: string; next_signer_epoch: string;
  revoked_signer_set_digest: string;
  overlap_started_at: number; overlap_expires_at: number;
  recovery_custody_digest: string; monitor_rearm_tuple_digest: string;
  previous_manifest_digest: string;
  manifest_issuer_key_id: string; manifest_issuer_epoch: string;
  issued_at: number;
}

interface RegistrySnapshot {
  manifest: SignerRotationManifest;
  manifestDigest: string;
  storeVersion: number;
  activationReceipt: AuditReceipt;
  highWater: { generation: number; auditSequence: number; witnessRoot: string };
}

interface SignerAuthorization {
  manifest: SignerRotationManifest;
  manifestDigest: string;
  identity: PublicSigningIdentity;
}

export interface SignerRegistryAuthority {
  current(): Promise<RegistrySnapshot>;
  authorize(input: { role: TokenRole; keyId: string; epoch: string; tupleDigest: string; at: number }): Promise<SignerAuthorization>;
  rotate(proposal: ManifestProposal): Promise<RegistrySnapshot>;
}
```

`SignerRegistryAuthority` recebe no construtor exatamente
`{store, audit: DurableAuditLog, witness: CurrentCheckpointWitness, clock:
TrustedClock, nonce: FreshNonceSource, custody: SigningCustody,
revocations: RevocationAuthority, identityDirectory: IdentityDirectory, namespace,
monitorRearmTupleDigest}`. `identityDirectory.resolve(keyId, epoch, role)` é
somente leitura e deve devolver a identidade pública correspondente; identidade
desconhecida ou papel divergente recusa a operação.

### Estado e invariantes

O valor de `<ns>:signer-registry:head` é exatamente:

```ts
{
  version: "1", generation: number, manifestDigest: string,
  manifest: SignerRotationManifest, activationReceipt: AuditReceipt,
  auditSequence: number, auditRoot: string, witnessRoot: string
}
```

`generation`, `auditSequence`, `auditRoot`, `witnessRoot` são high-water
duráveis. `current()` verifica assinatura do manifest pelo issuer `manifest`,
digest, `activationReceipt`, payload do activation record, identidade/epoch,
revocation record, custody record, tuple digest e todos os roots; discrepância
é `RegistryIntegrityError`. `manifest_generation` deve ser exatamente o
high-water anterior + 1; `previous_manifest_digest` deve ser o digest persistido
(na gênese, 64 zeros); a transação nunca aceita geração, sequência ou root menor.

`authorize` só autoriza identidade cujo `(keyId,epoch,role)` esteja no manifest
atual, não esteja revogado no `revoked_signer_set_digest`, esteja dentro da
janela de overlap e use o `tupleDigest` atual. O `next` só pode assinar a partir
de `overlap_started_at`; o `active` deixa de assinar em `overlap_expires_at` ou
na revogação. A identidade, role e epoch são comparados contra o diretório;
campos do token não são autoridade.

### Rotação sem recursão e anti-rollback

`rotate` executa esta sequência única, com no máximo oito re-leituras CAS:

1. Lê e valida o head atual. Gera `nonce = nonce.next()` de fonte independente
   (`crypto.randomBytes(32).toString("hex")` em produção) e chama
   `witness.readHead(nonce)`. O nonce é novo para cada chamada, a resposta deve
   ecoá-lo exatamente, e `sequence/root` não podem regredir ao high-water. O
   witness é construído sem referência ao registry; o registry nunca chama o
   próprio runtime para obter essa prova. Assim não existe recursão de
   autorização.
2. Confere proposta: geração seguinte, previous digest, issuer atual,
   identidades distintas, overlap estritamente crescente, tuple atual,
   revocation/custody WORM válidos e issuer autorizado. `RevocationAuthority`
   e `SigningCustody` verificam o receipt no audit log e exigem que o payload
   lido seja exatamente o conjunto/record cujo digest foi proposto. A gênese só aceita o
   issuer de bootstrap explicitamente configurado e somente se o store e o
   journal estiverem vazios; não existe chave padrão.
3. Faz `audit.append("manifest:<generation>:preseal:<proposalDigest>", {
   type:"MANIFEST_PRESEAL", proposal, priorManifestDigest, witnessHead })` e
   verifica o receipt. Preseal é a quebra explícita do ciclo: o manifest recebe
   os `witness_checkpoint_sequence`, `witness_previous_root_digest` e
   `witness_root_digest` desse receipt; nenhum manifest é usado para obter o
   witness que o autoriza.
4. Resolve a custody do issuer, cria o token com
   `createSignerManifest`, e faz `audit.append("manifest:<generation>:activation:<manifestDigest>",
   {type:"MANIFEST_ACTIVATION", manifest, manifestDigest, presealReceipt})`.
   Verifica ambos os receipts, lê os payloads esperados dos receipts e exige
   que activation estenda o preseal na cadeia do mesmo `worm_log_id`. O estado
   persiste o activation receipt; não basta guardar o manifest assinado.
5. Gera um segundo nonce independente, lê o head do witness novamente e exige
   que a prova seja atual e não inferior ao activation receipt. Faz uma única
   transação CAS: escreve o novo `<ns>:signer-registry:head` esperando a versão
   lida e a nova versão da operação de rotação. Em conflito, recarrega; se o
   mesmo digest já estiver persistido devolve-o idempotentemente, caso contrário
   reinicia a partir do novo head.

Perda da chave ativa só pode usar `SigningCustody` cujo record esteja no WORM,
tenha digest igual a `recovery_custody_digest`, identidade não revogada e
manifest issuer autorizado. Não há restauração de segredo no DynamoDB, fallback
para chave anterior ou troca de epoch fora de um manifest assinado.

## 2. `ack_recovery` durável

O módulo novo é `src/ack_recovery.ts`.

```ts
interface RecoveryRequest { envelope: unknown; original_ack_digest: string }
interface RecoveryResponse { token: RecoveryToken; auditReceipt: AuditReceipt }

export interface AckRecoveryService {
  recover(request: RecoveryRequest): Promise<RecoveryResponse>;
}
```

O construtor recebe `{store, audit, ingest: IngestService, registry,
clock, recoverySigner: (auth: SignerAuthorization) => Promise<AsyncSigner>,
identityDirectory, namespace}`. `envelope` é parseado por `parseEnvelope`; o
commit original é obtido por `ingest.getCommitted(envelope)`. A recuperação
recusa envelope inválido, registro ausente, source/service/application/key/epoch
divergente, `original_ack_digest` diferente do SHA-256 do ACK completo, ou
qualquer payload fora do commit.

O serviço verifica `intentReceipt` e `resultReceipt` do commit e lê os dois
payloads no journal. O primeiro deve ser exatamente
`{type:"WRITE_AHEAD_INTENT", commitId, envelope, envelopeDigest}`; o segundo,
`{type:"WRITE_AHEAD_RESULT", commitId, envelopeDigest, ackDigest, outcome}`.
Isso prova o payload auditado e impede recuperar um ACK a partir de apenas
campos copiados da requisição.

Depois carrega `registry.current()`, verifica que o signer do ACK original
está no `revoked_signer_set_digest`, resolve a identidade histórica pelo
diretório e verifica a assinatura antiga. Autoriza o signer de role `recovery`
contra o manifest atual e tuple atual. O token emitido usa exatamente os 21
campos de `RecoveryToken`; `original_monitor_rearm_tuple_digest` vem do
envelope/ACK persistido, `current_monitor_rearm_tuple_digest` do registry,
`original_ack_digest` é do token original completo e os três digests de
revogação/manifest/witness vêm dos records verificados. `issued_at` vem do
`TrustedClock`; `Date.now()` não participa.

Antes da assinatura grava `ACK_RECOVERY_INTENT` no audit log com todos os campos
esperados sem assinatura e os digests dos receipts. Depois da assinatura grava
`ACK_RECOVERY_RESULT` com o token completo e verifica o payload pelo journal.
Por fim faz CAS no key idempotente. Repetição byte-idêntica, com o mesmo digest
de manifest atual, devolve o mesmo token/receipt; request divergente no mesmo
key é `REJECTED`. Um novo manifest pode produzir uma nova chave de recuperação,
mas nunca uma segunda ingestão.

Recuperação não altera cursor, incident, outbox, producer sequence, deadline,
payload, ACK original ou estado de autorização. Não chama SNS, scheduler,
Cloudflare, witness head diretamente, nem reprocessa `IngestService.ingest`.

## 3. `page_ack` autenticado e idempotente

O módulo novo é `src/page_ack.ts`.

```ts
interface HumanPrincipal {
  identity: string; scheduleDigest: string; expiresAt: number;
}
interface HumanAuthenticator {
  authenticate(input: { authorization: string; method: string; path: string; bodyDigest: string; at: number }): Promise<HumanPrincipal>;
}
interface OnCallSchedule {
  authorize(input: { identity: string; destination: string; action: "ACK"; at: number; scheduleDigest: string }): Promise<{ expiresAt: number }>;
}
interface PageAckRequest {
  incident_id: string; page_id: string; delivery_id: string;
  destination: string; action: "ACK"; payload: string;
}
interface PageAckResult { token: PageAckToken; auditReceipt: AuditReceipt }

export interface PageAckService {
  acknowledge(request: PageAckRequest, auth: { authorization: string; method: string; path: string }): Promise<PageAckResult>;
}
```

O construtor recebe `{store, audit, clock, registry, pageAckSigner,
authenticator, schedule, namespace, destination, onCallScheduleDigest}`.
`page_id` é obrigatório e deve ser
`sha256(JSON.stringify(["page", incident_id, delivery_id, destination]))`.
`delivery_id` deve existir em `deliveryKey(namespace, delivery_id)`, pertencer
ao `incident_id`, ter exatamente `destination` e
`payloadDigest === sha256(payload)`. O único `action` aceito nesta versão é
`"ACK"`; o body do payload é imutável e não é aceito como texto equivalente.

O runtime autentica o principal antes do serviço; `on_call_identity` não vem
do JSON do cliente. O serviço exige principal não vazio, schedule digest igual
ao digest de configuração e `schedule.authorize` positivo para identidade,
destino, ação e horário confiável. Exige `at <= expiresAt` e assina somente
com identidade role `page-ack` autorizada pelo manifest atual. O token contém
os 16 campos do codec; `payload_digest` é do payload do delivery, e os digests
de tuple/manifest são os atuais verificados.

Fluxo: reserva `<ns>:page-ack:<id>` como `pending` por CAS; grava e verifica
`PAGE_ACK_INTENT`; carrega incident e delivery novamente; assina; grava e
verifica `PAGE_ACK_RESULT`; em uma transação CAS transforma a reserva em
`completed` e define `incident.humanAcknowledgedAt` somente se ainda `null`.
Se já houver completed idêntico, devolve o mesmo token. Qualquer token/request
divergente, replay após expiry, incidente/delivery/payload/destino/schedule
divergente, principal fora da escala ou signer revogado é recusado e não altera
incidente.

O ACK humano nunca fecha incidente, cancela outbox, suprime sinal novo,
reenfileira página, publica SNS ou altera cursor. `humanAcknowledgedAt` só
suprime a escalada já criada; novos sinais continuam atualizando o mesmo
incident CAS.

## 4. Runtime principal e bindings

O módulo novo é `src/index.ts`, compilado pelo `Containerfile` já fixado como
`dist/index.handler`. A inicialização é lazy singleton por processo, mas toda
autoridade permanece nos stores. Ela exige `MONITOR_CONFIG_JSON`, valida-o com
`validateConfig`, constrói os clientes AWS e injeta as implementações concretas:

```text
DynamoDbStateStore
S3ImmutableJournal (>= 8 dias, compliance/WORM)
DurableAuditLog -> journal signer + independent CurrentCheckpointWitness
IngestService
DurableDeliveryOutbox -> SnsAlertTransport
MonitorScheduler
SignerRegistryAuthority
AckRecoveryService
PageAckService
```

Não há config parcial, chave embutida, signer falso, callback `() => true`,
relógio local, endpoint alternativo ou fallback em memória. A identidade
witness é do account/verifier configurado; journal, manifest, ingest-ACK,
recovery e page-ACK mantêm roles, keys, epochs e trust/revocation digests
separados.

### HTTP

API Gateway HTTP API payload v2.0, body JSON UTF-8, sem logging de
`Authorization`, secrets, payload bruto ou tokens completos.

| Rota | Entrada | Sucesso | Efeito permitido |
|---|---|---|---|
| `POST /v1/ingest` | envelope completo | `200` com `AckToken` ou `HistoricalTerminal` | ingest CAS, incident/outbox CAS; nunca delivery externo |
| `POST /v1/ack/recovery` | `{envelope,original_ack_digest}` | `200` com `RecoveryToken` | somente prova/registro de recovery |
| `POST /v1/incidents/{incidentId}/pages/{pageId}/ack` | `PageAckRequest` + `Authorization` | `200` com `PageAckToken` | somente ACK humano CAS |

Mapeamento obrigatório: `REJECTED=400`, `QUARANTINED=409`,
`RECOVERY_REQUIRED=409`, `RETRY/UNKNOWN=503` com `Retry-After: 1`, erro de
autorização humana `401/403`, conflito de page ACK `409`. Nenhuma resposta 2xx
é emitida antes do CAS e dos receipts exigidos.

### Scheduler

EventBridge Scheduler chama a mesma Lambda com o objeto exato já configurado
em `monitor-runtime.yaml`: `{kind:"monitor_tick", source:"aws-scheduler"}`.
O runtime aceita somente esses dois campos, obtém `TrustedClock.now()` uma vez
e chama `MonitorScheduler.run(scheduledFor)` com esse horário confiável como o
slot da invocação. A lateness dos produtores continua sendo decidida pelos
`expectedAt`/high-waters persistidos; o runtime não inventa um horário local.
O monitor faz o CAS de incident/outbox e depois drena a outbox com idempotência
do `operationId`. Evento desconhecido ou duplicata em execução é erro tipado;
retry do Scheduler é seguro por CAS/operation id.

O handler não cria timer de processo, não altera schedule/flags de ativação,
não chama API Cloudflare, não publica recovery/page ACK por SNS e não transforma
receipt de transporte em ACK de monitor. Cada exceção é sanitizada para fora;
detalhes ficam apenas em métricas sem dados sensíveis.

## Critérios de aceite fechados

1. **Registry:** genesis, concorrência de duas rotações, restart, rollback de
   storage, fork, digest/receipt/payload corrompido, nonce repetido, witness
   stale, issuer/role/epoch errado, revogação durante e após overlap, perda de
   primary e custody divergente. Só uma geração vence; nenhuma regressão vence.
2. **Recovery:** ACK original válido/revogado, token atual assinado, prova dos
   dois payloads via `journal.read`, duplicate estável, request divergente,
   original ausente, payload alterado, manifest/tuple/revocation errados,
   signer recovery stale/revogado, crash entre intent/signature/result/CAS e
   zero alteração em cursor/incident/outbox/sequence.
3. **Page ACK:** principal válido/inválido, schedule atual/fora de escala,
   expiry, action/payload/destination/page/delivery substitution, replay e
   concorrência. Um ACK muda `humanAcknowledgedAt` uma vez e não produz efeito
   de entrega.
4. **Runtime:** wiring real de todos os módulos, três rotas com status acima,
   scheduler exact binding, ausência de `Date.now`/defaults/callbacks de
   autorização, fail-closed em config/estado/clock/receipt e nenhuma chamada
   externa proibida nos três serviços.

O executor deve entregar os testes focados correspondentes e `tsc --noEmit`;
CI pesada e probes live ficam para o gate do bundle. Este contrato não exige
mudança de schema wire nem reabertura dos codecs já aprovados.
