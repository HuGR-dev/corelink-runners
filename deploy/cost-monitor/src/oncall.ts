import { createHash } from "node:crypto";
import type { MonitorStateStore } from "./state.js";
import type { TrustedClock } from "./trusted_time.js";
import { canonicalJSON } from "./journal.js";
import type {
  HumanAuthenticator,
  HumanPrincipal,
  OnCallSchedule,
} from "./page_ack.js";
const HEX = /^[0-9a-f]{64}$/;
const text = (v: unknown, max = 512): v is string =>
  typeof v === "string" && v.length > 0 && v.length <= max && v.trim() === v;
const sha = (v: string) => createHash("sha256").update(v).digest("hex");
export interface ApiGatewayIamContext {
  accountId: string;
  userArn: string;
  userId: string;
  accessKey: string;
  callerId: string;
}
export class HumanAuthError extends Error {
  override readonly name = "HumanAuthError";
}
export class ApiGatewayIamHumanAuthenticator implements HumanAuthenticator {
  constructor(
    private readonly o: {
      apiId: string;
      stage: string;
      accountIds: readonly string[];
      maxSkewMs: number;
      ttlMs: number;
      scheduleDigest: string;
    },
  ) {}
  async authenticate(input: any): Promise<HumanPrincipal> {
    if (
      input.method !== "POST" ||
      input.apiId !== this.o.apiId ||
      input.stage !== this.o.stage ||
      !text(input.path) ||
      !/^AWS4-HMAC-SHA256 Credential=/.test(input.authorization) ||
      !/^\d{8}T\d{6}Z$/.test(input.amzDate) ||
      !HEX.test(input.idempotencyKey) ||
      !HEX.test(input.bodyDigest)
    )
      throw new HumanAuthError("invalid IAM request");
    const c = input.requestContextIam as ApiGatewayIamContext;
    if (
      !c ||
      !this.o.accountIds.includes(c.accountId) ||
      !/^\d{12}$/.test(c.accountId) ||
      !text(c.userArn) ||
      !new RegExp(
        `^arn:(aws|aws-us-gov|aws-cn):iam::${c.accountId}:(user|role)/[A-Za-z0-9+=,.@_-]{1,256}$`,
      ).test(c.userArn) ||
      ![c.userId, c.accessKey, c.callerId].every((x) => text(x))
    )
      throw new HumanAuthError("invalid IAM context");
    const stamp = Date.UTC(
      Number(input.amzDate.slice(0, 4)),
      Number(input.amzDate.slice(4, 6)) - 1,
      Number(input.amzDate.slice(6, 8)),
      Number(input.amzDate.slice(9, 11)),
      Number(input.amzDate.slice(11, 13)),
      Number(input.amzDate.slice(13, 15)),
    );
    if (
      !Number.isFinite(stamp) ||
      Math.abs(input.at - stamp) > this.o.maxSkewMs
    )
      throw new HumanAuthError("stale IAM request");
    return {
      identity: c.userArn,
      scheduleDigest: this.o.scheduleDigest,
      expiresAt: input.at + this.o.ttlMs,
    };
  }
}
export class DynamoOnCallSchedule implements OnCallSchedule {
  constructor(
    private readonly o: {
      store: MonitorStateStore;
      namespace: string;
      destination: string;
      scheduleDigest: string;
      clock: TrustedClock;
      ttlMs: number;
    },
  ) {}
  async authorize(input: {
    identity: string;
    destination: string;
    action: "ACK";
    at: number;
    scheduleDigest: string;
  }): Promise<{ expiresAt: number }> {
    if (
      input.destination !== this.o.destination ||
      input.action !== "ACK" ||
      input.scheduleDigest !== this.o.scheduleDigest
    )
      throw new HumanAuthError("schedule mismatch");
    const c = await this.o.store.get<any>(
      `${this.o.namespace}:oncall:schedule:current`,
    );
    if (
      !c ||
      c.value?.version !== "1" ||
      c.value.scheduleDigest !== this.o.scheduleDigest ||
      c.value.destination !== this.o.destination ||
      input.at < c.value.effectiveAt ||
      input.at >= c.value.expiresAt
    )
      throw new HumanAuthError("schedule unavailable");
    const member = await this.o.store.get<any>(
      `${this.o.namespace}:oncall:schedule:${this.o.scheduleDigest}:member:${sha(input.identity)}`,
    );
    if (
      !member ||
      member.value?.version !== "1" ||
      member.value.principalArn !== input.identity ||
      member.value.destination !== input.destination ||
      JSON.stringify(member.value.actions) !== JSON.stringify(["ACK"]) ||
      input.at < member.value.notBefore ||
      input.at >= member.value.expiresAt
    )
      throw new HumanAuthError("on-call member unavailable");
    return {
      expiresAt: Math.min(
        c.value.expiresAt,
        member.value.expiresAt,
        input.at + this.o.ttlMs,
      ),
    };
  }
}
export function scheduleDigest(value: unknown): string {
  return sha(canonicalJSON(value));
}
