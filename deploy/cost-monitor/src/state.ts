import {
  DynamoDBClient,
  GetItemCommand,
  QueryCommand,
  TransactWriteItemsCommand,
  type AttributeValue,
  type DynamoDBClientConfig,
} from "@aws-sdk/client-dynamodb";

export interface Stored<T = unknown> { key: string; version: number; value: T }
export interface Write { key: string; expectedVersion: number | null; value: unknown }
export interface MonitorStateStore {
  get<T>(key: string): Promise<Stored<T> | null>;
  transact(writes: readonly Write[]): Promise<"committed" | "conflict">;
  scan(prefix: string, cursor?: string): Promise<{ items: Stored[]; nextCursor: string | null }>;
}

export class StateBackendError extends Error {
  readonly code = "ambiguous" as const;
  constructor(message: string, options?: { cause?: unknown }) {
    super(message, options);
    this.name = "StateBackendError";
  }
}

type Item = Record<string, AttributeValue>;
const MAX_ITEM_BYTES = 400 * 1024;
const MAX_WRITES = 100;
const CURSOR_VERSION = 1;

function safeVersion(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 1;
}
function validKey(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= 1024 && !/[\u0000-\u001f\u007f]/.test(value);
}
function clone<T>(value: T): T {
  return structuredClone(value);
}
function jsonBytes(value: unknown): number {
  let encoded: string;
  try { encoded = JSON.stringify(value); } catch { throw new TypeError("state value must be JSON serializable"); }
  if (encoded === undefined) throw new TypeError("state value must be JSON serializable");
  return new TextEncoder().encode(encoded).byteLength;
}
function validateWrite(write: Write): void {
  if (!write || !validKey(write.key)) throw new TypeError("invalid state key");
  if (write.expectedVersion !== null && !safeVersion(write.expectedVersion)) throw new TypeError("invalid expected version");
  const bytes = jsonBytes(write.value);
  if (bytes > MAX_ITEM_BYTES) throw new RangeError("state item exceeds DynamoDB limit");
}
function encodeCursor(namespace: string, prefix: string, key: string): string {
  return Buffer.from(JSON.stringify({ v: CURSOR_VERSION, namespace, prefix, key }), "utf8").toString("base64url");
}
function decodeCursor(cursor: string, namespace: string, prefix: string): string {
  if (typeof cursor !== "string" || cursor.length === 0 || cursor.length > 4096) throw new TypeError("invalid scan cursor");
  let parsed: unknown;
  try { parsed = JSON.parse(Buffer.from(cursor, "base64url").toString("utf8")); } catch { throw new TypeError("invalid scan cursor"); }
  if (!parsed || typeof parsed !== "object") throw new TypeError("invalid scan cursor");
  const p = parsed as Record<string, unknown>;
  if (p.v !== CURSOR_VERSION || p.namespace !== namespace || p.prefix !== prefix || !validKey(p.key)) throw new TypeError("invalid scan cursor");
  return p.key;
}
function parseStored(item: Item | undefined, expectedKey?: string): Stored {
  if (!item || typeof item.SK?.S !== "string" || typeof item.version?.N !== "string" || typeof item.value?.S !== "string") {
    throw new StateBackendError("malformed state item");
  }
  const version = Number(item.version.N);
  if (!safeVersion(version) || (expectedKey !== undefined && item.SK.S !== expectedKey)) throw new StateBackendError("malformed state item");
  try { return { key: item.SK.S, version, value: JSON.parse(item.value.S) }; }
  catch (cause) { throw new StateBackendError("malformed state item", { cause }); }
}
function isConditionalFailure(error: unknown): boolean {
  if (!error || typeof error !== "object") return false;
  const e = error as { name?: string; CancellationReasons?: Array<{ Code?: string }> };
  return e.name === "ConditionalCheckFailedException"
    || (e.name === "TransactionCanceledException" && (e.CancellationReasons?.some((r) => r?.Code === "ConditionalCheckFailed") ?? false));
}

export class DynamoDbStateStore implements MonitorStateStore {
  constructor(private readonly options: { client: DynamoDBClient; tableName: string; namespace: string }) {
    if (!options.client || !validKey(options.tableName) || !validKey(options.namespace)) throw new TypeError("invalid state store configuration");
  }
  async get<T>(key: string): Promise<Stored<T> | null> {
    if (!validKey(key)) throw new TypeError("invalid state key");
    let result;
    try {
      result = await this.options.client.send(new GetItemCommand({ TableName: this.options.tableName, ConsistentRead: true, Key: { PK: { S: this.options.namespace }, SK: { S: key } } }));
    } catch (cause) { throw new StateBackendError("state read is ambiguous", { cause }); }
    return result.Item ? parseStored(result.Item, key) as Stored<T> : null;
  }
  async transact(writes: readonly Write[]): Promise<"committed" | "conflict"> {
    if (!Array.isArray(writes) || writes.length < 1 || writes.length > MAX_WRITES) throw new RangeError("transaction must contain 1..100 writes");
    const seen = new Set<string>(); writes.forEach((w) => { validateWrite(w); if (seen.has(w.key)) throw new TypeError("duplicate state key"); seen.add(w.key); });
    const items = writes.map((w) => ({
      Put: {
        TableName: this.options.tableName,
        Item: { PK: { S: this.options.namespace }, SK: { S: w.key }, version: { N: String(w.expectedVersion === null ? 1 : w.expectedVersion + 1) }, value: { S: JSON.stringify(w.value) } },
        ConditionExpression: w.expectedVersion === null ? "attribute_not_exists(#version)" : "#version = :expected",
        ExpressionAttributeNames: { "#version": "version" },
        ...(w.expectedVersion === null ? {} : { ExpressionAttributeValues: { ":expected": { N: String(w.expectedVersion) } } }),
      },
    }));
    try { await this.options.client.send(new TransactWriteItemsCommand({ TransactItems: items })); return "committed"; }
    catch (error) { if (isConditionalFailure(error)) return "conflict"; throw new StateBackendError("state transaction outcome is ambiguous", { cause: error }); }
  }
  async scan(prefix: string, cursor?: string): Promise<{ items: Stored[]; nextCursor: string | null }> {
    if (typeof prefix !== "string" || prefix.length > 1024) throw new TypeError("invalid scan prefix");
    const exclusive = cursor === undefined ? undefined : decodeCursor(cursor, this.options.namespace, prefix);
    let result;
    try {
      result = await this.options.client.send(new QueryCommand({ TableName: this.options.tableName, ConsistentRead: true, KeyConditionExpression: "#pk = :pk AND begins_with(#sk, :prefix)", ExpressionAttributeNames: { "#pk": "PK", "#sk": "SK" }, ExpressionAttributeValues: { ":pk": { S: this.options.namespace }, ":prefix": { S: prefix } }, ...(exclusive ? { ExclusiveStartKey: { PK: { S: this.options.namespace }, SK: { S: exclusive } } } : {}) }));
    } catch (cause) { throw new StateBackendError("state scan is ambiguous", { cause }); }
    const items = (result.Items ?? []).map((item) => parseStored(item));
    const last = result.LastEvaluatedKey?.SK?.S;
    return { items, nextCursor: last ? encodeCursor(this.options.namespace, prefix, last) : null };
  }
}

export class MemoryStateStore implements MonitorStateStore {
  private readonly values = new Map<string, Stored>();
  private lock: Promise<void> = Promise.resolve();
  private async mutex<T>(fn: () => Promise<T>): Promise<T> {
    const prior = this.lock; let release!: () => void; this.lock = new Promise<void>((resolve) => { release = resolve; });
    await prior; try { return await fn(); } finally { release(); }
  }
  async get<T>(key: string): Promise<Stored<T> | null> { if (!validKey(key)) throw new TypeError("invalid state key"); return this.mutex(async () => { const x = this.values.get(key); return x ? clone(x) as Stored<T> : null; }); }
  async transact(writes: readonly Write[]): Promise<"committed" | "conflict"> {
    if (!Array.isArray(writes) || writes.length < 1 || writes.length > MAX_WRITES) throw new RangeError("transaction must contain 1..100 writes");
    writes.forEach(validateWrite); const keys = new Set(writes.map((w) => w.key)); if (keys.size !== writes.length) throw new TypeError("duplicate state key");
    return this.mutex(async () => { if (writes.some((w) => (this.values.get(w.key)?.version ?? null) !== w.expectedVersion)) return "conflict"; writes.forEach((w) => this.values.set(w.key, { key: w.key, version: w.expectedVersion === null ? 1 : w.expectedVersion + 1, value: clone(w.value) })); return "committed"; });
  }
  async scan(prefix: string, cursor?: string): Promise<{ items: Stored[]; nextCursor: string | null }> {
    if (typeof prefix !== "string" || prefix.length > 1024) throw new TypeError("invalid scan prefix");
    const start = cursor === undefined ? undefined : decodeCursor(cursor, "memory", prefix);
    return this.mutex(async () => { const keys = [...this.values.keys()].filter((k) => k.startsWith(prefix)).sort(); const from = start ? keys.findIndex((k) => k > start) : 0; const selected = keys.slice(from < 0 ? 0 : from, (from < 0 ? 0 : from) + 100); const last = selected.length === 100 ? selected[selected.length - 1] : undefined; return { items: selected.map((k) => clone(this.values.get(k)!)), nextCursor: last ? encodeCursor("memory", prefix, last) : null }; });
  }
}
