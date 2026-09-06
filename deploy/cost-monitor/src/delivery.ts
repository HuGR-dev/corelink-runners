import { createHash } from "node:crypto";
import { PublishCommand, type SNSClient } from "@aws-sdk/client-sns";

export type DeliveryOperation = {
  operationId: string;
  incidentId: string;
  kind: "initial" | "escalation" | "recovery" | "update";
  destination: string;
  payload: string;
  payloadDigest: string;
};

export type DeliveryResult =
  | { status: "accepted"; operationId: string; provider: "aws-sns"; providerMessageId: string }
  | { status: "unknown"; operationId: string; reason: string };

export interface AlertTransport {
  publish(operation: DeliveryOperation): Promise<DeliveryResult>;
}

export class DeliveryValidationError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "DeliveryValidationError";
  }
}

type SnsClientLike = Pick<SNSClient, "send">;

const MAX_SNS_BYTES = 256 * 1024;
const HEX_SHA256 = /^[0-9a-f]{64}$/;

function exactText(value: unknown, field: string): asserts value is string {
  if (typeof value !== "string" || value.length === 0 || value.trim() !== value) {
    throw new DeliveryValidationError(`${field} must be a non-empty exact string`);
  }
}

function validateOperation(operation: DeliveryOperation, destination: string, maxBytes: number): void {
  if (operation === null || typeof operation !== "object") throw new DeliveryValidationError("operation is required");
  exactText(operation.operationId, "operationId");
  exactText(operation.incidentId, "incidentId");
  exactText(operation.destination, "destination");
  exactText(operation.payload, "payload");
  if (!["initial", "escalation", "recovery", "update"].includes(operation.kind)) {
    throw new DeliveryValidationError("kind is invalid");
  }
  if (operation.destination !== destination) throw new DeliveryValidationError("destination is not approved");
  if (!HEX_SHA256.test(operation.payloadDigest)) throw new DeliveryValidationError("payloadDigest is invalid");
  const digest = createHash("sha256").update(operation.payload, "utf8").digest("hex");
  if (digest !== operation.payloadDigest) throw new DeliveryValidationError("payloadDigest does not match payload");
  const attributeBytes = [
    ["operation", operation.operationId],
    ["incident", operation.incidentId],
    ["payloadDigest", operation.payloadDigest],
  ].reduce((total, [name, value]) => total + Buffer.byteLength(name, "utf8") + Buffer.byteLength("String", "utf8") + Buffer.byteLength(value, "utf8"), 0);
  if (Buffer.byteLength(operation.payload, "utf8") + attributeBytes > maxBytes) {
    throw new DeliveryValidationError("SNS message and attributes exceed configured limit");
  }
}

export class SnsAlertTransport implements AlertTransport {
  private readonly client: SnsClientLike;
  private readonly topicArn: string;
  private readonly destination: string;
  private readonly maxPayloadBytes: number;

  constructor(options: { client: SNSClient; topicArn: string; destination: string; maxPayloadBytes: number }) {
    exactText(options.topicArn, "topicArn");
    exactText(options.destination, "destination");
    if (options.topicArn.endsWith(".fifo")) throw new DeliveryValidationError("FIFO SNS topics are not supported");
    if (!Number.isSafeInteger(options.maxPayloadBytes) || options.maxPayloadBytes <= 0 || options.maxPayloadBytes > MAX_SNS_BYTES) {
      throw new DeliveryValidationError("maxPayloadBytes is invalid");
    }
    this.client = options.client;
    this.topicArn = options.topicArn;
    this.destination = options.destination;
    this.maxPayloadBytes = options.maxPayloadBytes;
  }

  async publish(operation: DeliveryOperation): Promise<DeliveryResult> {
    validateOperation(operation, this.destination, this.maxPayloadBytes);
    const command = new PublishCommand({
      TopicArn: this.topicArn,
      Message: operation.payload,
      MessageAttributes: {
        operation: { DataType: "String", StringValue: operation.operationId },
        incident: { DataType: "String", StringValue: operation.incidentId },
        payloadDigest: { DataType: "String", StringValue: operation.payloadDigest },
      },
    });
    try {
      const response = await this.client.send(command);
      if (typeof response.MessageId !== "string" || response.MessageId.length === 0 || response.MessageId.trim() !== response.MessageId) {
        return { status: "unknown", operationId: operation.operationId, reason: "malformed_provider_result" };
      }
      return { status: "accepted", operationId: operation.operationId, provider: "aws-sns", providerMessageId: response.MessageId };
    } catch {
      return { status: "unknown", operationId: operation.operationId, reason: "provider_outcome_unknown" };
    }
  }
}
