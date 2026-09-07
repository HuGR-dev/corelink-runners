# T6-W15 — binding definitivo de `HumanAuthenticator` e `OnCallSchedule`

Status: **DECIDIDO PARA IMPLEMENTAÇÃO**. Base: `26c54a35ae249f52693a15e8724e2fd72fff1cb4`.
Este suplemento completa o contrato de `PageAckService` e do runtime. Não
introduz serviço novo, biblioteca OIDC/JWT ou produto de interface humana.

Fontes verificadas: `deploy/cost-monitor/src/{index,page_ack,state,types}.ts`,
`deploy/cost-monitor/infra/{monitor-runtime,monitor-foundation}.yaml`,
`deploy/cloudflare-canary/src/{index,tick_adapter,tick_outbox,notify}.ts` e os
commits históricos `58e8a09`/`1932a81` (`deploy/cloudflare-canary/src/page_ack.ts`).

## Decisão de autenticação

O único binding humano é `AWS_IAM` na rota exata:

```text
POST /v1/incidents/{incidentId}/pages/{pageId}/ack
```

`AWS::ApiGatewayV2::Route.AuthorizationType` deve ser `AWS_IAM`, apontando para
a integração Lambda já promovida. A rota `ANY /{proxy+}` continua servindo as
rotas existentes; o match exato da rota de ACK vence o catch-all. A Scheduler
continua chamando a Lambda diretamente com seu evento de máquina e não pode
atingir o serviço de ACK humano.

O delta de infraestrutura é exatamente este recurso no template de runtime:

```yaml
  PageAckRoute:
    Type: AWS::ApiGatewayV2::Route
    Properties:
      ApiId: !Ref HttpApi
      RouteKey: 'POST /v1/incidents/{incidentId}/pages/{pageId}/ack'
      AuthorizationType: AWS_IAM
      Target: !Sub integrations/${HttpIntegration}
```

`AWS_IAM` é um tipo suportado em rotas de HTTP API e exige SigV4 mais
`execute-api:Invoke` para a rota. [Documentação AWS de IAM em HTTP APIs](https://docs.aws.amazon.com/apigateway/latest/developerguide/http-api-access-control-iam.html)


O API Gateway verifica a assinatura AWS Signature Version 4 com HMAC-SHA256
antes de invocar a Lambda. A credencial deve usar o escopo
`YYYYMMDD/<region>/execute-api/aws4_request`; método, host, caminho, headers
assinados, corpo e `X-Amz-Date` pertencem à assinatura. O runtime confia
somente no contexto IAM produzido pelo API Gateway e não reimplementa SigV4.

Não há JWT neste contrato: portanto não existe `iss`/`aud` livre para o cliente
fornecer. A raiz de issuer é a conta/partição AWS configurada em
`MONITOR_ONCALL_ACCOUNT_IDS` e a audiência é o ARN do API Gateway
`execute-api` para `MONITOR_API_ID`, `MONITOR_API_STAGE`, método e rota exatos.
`Bearer`, OIDC, Cloudflare Access e qualquer `Authorization` aceito fora de uma
rota `AWS_IAM` são rejeitados. Cloudflare Access permanece somente no seu uso
atual de autenticação de serviço para chamadas internas de máquina.

O contexto v2.0 exigido pela `HumanAuthenticator` é exatamente:

```ts
type ApiGatewayIamContext = {
  accountId: string;       // exatamente 12 dígitos
  userArn: string;         // arn:<partition>:iam::<account>:(user|role)/...
  userId: string;
  accessKey: string;
  callerId: string;
};
```

Todos os cinco campos são obrigatórios, strings limitadas a 512 bytes, e
`accountId` deve estar na allowlist. `userArn` deve pertencer à mesma conta e
ser um IAM user ou role; root, service principal e ARN de STS literal são
recusados. O `userArn` completo, com comparação byte a byte, é a identidade que
será consultada na escala de plantão. A ausência, duplicação ou forma inválida
do contexto falha fechado.

O `HumanAuthenticator` deixa de receber o objeto incompleto atual e passa a
expor esta interface exata:

```ts
interface HumanAuthenticator {
  authenticate(input: {
    method: "POST";
    path: string;
    apiId: string;
    stage: string;
    requestContextIam: ApiGatewayIamContext;
    authorization: string;
    amzDate: string;
    securityToken?: string;
    idempotencyKey: string;
    bodyDigest: string;
    at: number;
  }): Promise<{ identity: string; scheduleDigest: string; expiresAt: number }>;
}
```

O adapter valida `method`, `path`, `apiId`, `stage`, conta, `Authorization`
com prefixo `AWS4-HMAC-SHA256`, `X-Amz-Date` ISO básico de 16 dígitos,
`idempotencyKey` em lowercase hex de 64 caracteres e `bodyDigest` SHA-256 do
payload UTF-8 recebido. A assinatura SigV4 já foi validada pelo gateway; o
adapter exige que o evento seja `version:"2.0"` e que o contexto IAM exista.
`X-Amz-Security-Token` só é repassado quando presente para uma sessão STS e
nunca é persistido ou logado. `abs(at - parse(amzDate))` deve ser no máximo
`MONITOR_PAGE_ACK_MAX_SKEW_MS` (300000 por padrão). O resultado expira em
`min(scheduleExpiry, at + MONITOR_PAGE_ACK_TTL_MS)` (300000 por padrão).

O `idempotencyKey` é obrigatório e deve ser:

```text
sha256(JSON.stringify(["page-ack", incident_id, page_id, delivery_id,
                       destination, bodyDigest]))
```

O serviço compara o valor com o corpo e grava-o na reserva. Isso impede que uma
repetição com o mesmo caminho e payload altere o resultado, e que um payload
divergente se disfarce de retry.

## Escala de plantão no StateStore

O `DynamoDbStateStore` já faz `ConsistentRead: true` e CAS por `version`; não
há nova tabela. Como ele usa `PK=<namespace>` e `SK=<key>`, os SK exatos são:

```text
<ns>:oncall:schedule:current
<ns>:oncall:schedule:<scheduleDigest>:member:<sha256(principalArn)>
```

O valor da linha `current` é exatamente:

```ts
{
  version: "1";
  generation: number;
  scheduleDigest: string;
  destination: string;
  effectiveAt: number; // epoch milliseconds, inclusive
  expiresAt: number;   // epoch milliseconds, exclusive
}
```

O valor de cada linha `member` é exatamente:

```ts
{
  version: "1";
  scheduleDigest: string;
  principalArn: string;
  destination: string;
  actions: ["ACK"];
  notBefore: number;
  expiresAt: number;
}
```

`scheduleDigest` é SHA-256 do `canonicalJSON` de `{version,generation,
destination,effectiveAt,expiresAt,members}`, com `members` ordenados por
`principalArn` e contendo somente os campos acima, sem o próprio digest. O
`DynamoOnCallSchedule.authorize` lê `current` consistentemente, exige digest
igual a `config.onCallScheduleDigest`, destino igual a
`config.destination`, `effectiveAt <= at < expiresAt`, e então lê
consistentemente a linha do `principalArn`. Exige digest, ARN, destino, ação e
janela idênticos. Retorna:

```text
expiresAt = min(current.expiresAt, member.expiresAt,
                at + MONITOR_PAGE_ACK_TTL_MS)
```

Erro de leitura, linha ausente, digest divergente, janela vencida ou membro
revogado é recusa. O relógio é sempre `TrustedClock`; `Date.now()` não decide
autorização.

Bootstrap e rotação são uma operação administrativa offline usando o mesmo
DynamoDB e `TransactWriteItems`, sem endpoint novo. O bootstrap só aceita
`current` ausente e grava todos os membros e o ponteiro em uma transação; a
rotação exige `generation = current.generation + 1` e CAS na versão corrente,
grava os novos membros e troca o ponteiro atomicamente. O limite operacional é
90 membros por geração para caber na transação de 100 itens, deixando espaço
para condicionais. Uma geração parcialmente gravada nunca se torna corrente.
Nenhum request HTTP, tick ou canary pode instalar/alterar a escala.

## Corpo, headers e sequência do ACK

O evento Lambda precisa preservar `version:"2.0"`, `rawPath`,
`requestContext.http.method`, `requestContext.apiId`, `requestContext.stage`,
`requestContext.authorizer.iam` e headers sem normalização destrutiva. O corpo
JSON deve ter exatamente estes campos e o path deve coincidir byte a byte:

```json
{
  "incident_id": "...",
  "page_id": "sha256(JSON.stringify([\"page\",incident_id,delivery_id,destination]))",
  "delivery_id": "...",
  "destination": "...",
  "action": "ACK",
  "payload": "payload imutável da entrega"
}
```

Headers obrigatórios após lowercase único:

```text
authorization: AWS4-HMAC-SHA256 Credential=.../YYYYMMDD/region/execute-api/aws4_request, SignedHeaders=..., Signature=...
x-amz-date: YYYYMMDDTHHMMSSZ
x-idempotency-key: 64 lowercase hex
```

`x-amz-security-token` é opcional apenas para credencial STS aceita pelo
gateway. O runtime rejeita `Bearer`, service token Cloudflare, header repetido,
path/query inesperado, API/stage incorreto, body extra ou `action` diferente de
`ACK`.

Depois de autenticar e autorizar a escala, `PageAckService` verifica a entrega
pendente por `delivery_id`, incidente, destino, payload e digest do payload.
Lê o incidente e reserva por CAS na chave já congelada:

```text
<ns>:page-ack:<sha256(JSON.stringify([incident_id,page_id,delivery_id,destination]))>
```

O valor da reserva deve conter `status`, `fingerprint` do JSON completo,
`idempotencyKey`, `identity`, `intentReceipt`, e, quando concluído, `token`,
`auditReceipt` e `resultReceipt`. Uma reserva concluída só retorna o mesmo
resultado se fingerprint, idempotency key e identidade forem iguais; divergência
é `409 CONFLICT`. Concorrentes fazem um único efeito por CAS. Crash depois de
intent, assinatura, journal ou antes do CAS retorna erro/indeterminado e é
reconciliado pela evidência; nunca assume ACK por causa de HTTP 200.

A sequência obrigatória é: leitura consistente de schedule/entrega/incidente;
reserva `PENDING` por CAS; append e verificação da intenção; assinatura com a
identidade `page-ack` atual e manifest/revogação atuais; append e verificação do
resultado; CAS atômico da reserva concluída e de `incident.humanAcknowledgedAt`.
`humanAcknowledgedAt` só pode ser escrito nesta última transação. O token
assinado contém identidade, digest do schedule, digest do payload, manifest,
key/epoch, `acknowledged_at` e `expires_at`; o audit receipt é retornado junto.

## Composição singleton, AWS e least privilege

No `getRuntime()` singleton existente, substituir os dois placeholders por:

```ts
const authenticator = new ApiGatewayIamHumanAuthenticator({
  apiId: process.env.MONITOR_API_ID!,
  stage: process.env.MONITOR_API_STAGE!,
  accountIds: process.env.MONITOR_ONCALL_ACCOUNT_IDS!.split(","),
  maxSkewMs: Number(process.env.MONITOR_PAGE_ACK_MAX_SKEW_MS ?? 300000),
  ttlMs: Number(process.env.MONITOR_PAGE_ACK_TTL_MS ?? 300000),
});
const schedule = new DynamoOnCallSchedule({
  store, namespace: config.stateNamespace,
  destination: config.destination,
  scheduleDigest: config.onCallScheduleDigest,
  clock,
  ttlMs: Number(process.env.MONITOR_PAGE_ACK_TTL_MS ?? 300000),
});
```

Os objetos são passados ao `PageAckService` junto com o `registry`, `audit`,
`clock` e `pageAckSigner` já compostos. A factory continua singleton; ingest,
recovery, scheduler e seus signers não são recriados por request.

Adicionar ao ambiente da Lambda, sem segredo novo:

```text
MONITOR_CONFIG_JSON
MONITOR_AUDIT_LOG_ID
MONITOR_API_ID
MONITOR_API_STAGE=$default
MONITOR_ONCALL_ACCOUNT_IDS=975306274105
MONITOR_PAGE_ACK_MAX_SKEW_MS=300000
MONITOR_PAGE_ACK_TTL_MS=300000
STATE_TABLE (já existente)
```

O runtime usa `dynamodb:GetItem` e `dynamodb:TransactWriteItems` no
`FoundationTableName`, além das permissões de journal/KMS já requeridas pelos
serviços. API Gateway conserva a permissão de invocar o alias. Roles humanas
recebem somente `execute-api:Invoke` no ARN
`arn:<partition>:execute-api:<region>:<account>:<apiId>/<stage>/POST/v1/incidents/*/pages/*/ack`;
não recebem DDB, KMS, Lambda ou Secrets Manager. Scheduler conserva apenas
`lambda:InvokeFunction` e DLQ. Nenhuma chave `CORELINK_CF_ACCESS_*` ou secret
de bearer entra no caminho humano.

## Correção obrigatória do T6-W13

Os commits históricos `58e8a09` e `1932a81` adicionaram
`deploy/cloudflare-canary/src/page_ack.ts`, que fazia um POST com
`Authorization: PAGE_ACK_AUTHORIZATION` e `action:"ACK"`. Isso é auto-ACK de
máquina e não pode ser promovido. O HEAD `26c54a3` não deve reintroduzi-lo.

O canary deve apenas emitir a notificação de página contendo `page_url` HTTPS,
`incident_id`, `page_id`, `delivery_id`, destino e digest/payload imutável. O
`delivery_id` é correlação opaca, não credencial. Abrir o link não grava estado;
uma ação explícita de um principal IAM humano deve assinar a requisição SigV4
na rota acima. O canary não possui credencial IAM de on-call, não envia
`Authorization` para essa rota e não grava `humanAcknowledgedAt`. O POST
automático existente em `tick_outbox` continua sendo somente o transporte de
envelope de máquina para `/v1/ingest`; seu ACK de transporte nunca é ACK humano.

## Fixtures e aceite mínimo

Fixtures locais usam `MemoryStateStore`, `TrustedClock` fixo, signer RSA/KMS
fake e evento API Gateway v2 sintético com `authorizer.iam` completo. O fixture
não simula confiança em um bearer: a fronteira AWS_IAM é representada por um
contexto já verificado e há teste separado de rota para ausência/má-formação.

Implementação só está aceita quando estes testes passarem:

1. `page-ack-iam-auth.test.ts`: IAM user/role válido; conta, API, stage,
   método, path, contexto ausente, `Bearer`, Cloudflare token, ARN root/service,
   timestamp velho/futuro e idempotency key incorreta.
2. `on-call-schedule.test.ts`: bootstrap, leitura consistente, destino atual,
   membro válido, membro ausente/revogado, janela, digest errado, rotação CAS e
   ponteiro que nunca aponta para conjunto parcial.
3. `page-ack-replay-expiry.test.ts`: duplicate idempotente estável, fork 409,
   concorrência com um efeito, token expirado, schedule expirado e mismatch de
   payload/entrega.
4. `page-ack-crash.test.ts`: crash após reserva, intent, assinatura, journal e
   antes do CAS; nenhum caminho define `humanAcknowledgedAt` sem commit final.
5. `runtime-routes.test.ts`: rota exata exige AWS_IAM, `/v1/ingest` continua
   máquina, Scheduler aceita somente `{kind:"monitor_tick",source:"aws-scheduler"}`
   e nenhuma rota canary produz ACK humano.
6. `deploy/cloudflare-canary/test/page-ack-human-boundary.test.ts`: canary
   entrega link/correlação e digest; não chama POST `action:"ACK"`, não envia
   bearer e não altera estado de incidente.

Riscos ficam fechados por construção: SigV4 é validado no gateway; identidade e
escala são leituras consistentes do mesmo StateStore; replay/fork é CAS;
expiração usa relógio confiável; custody/rotação/revogação continuam sob o
registry já promovido; máquina não tem caminho de escrita para ACK humano.
