import { createHash } from "node:crypto";
import { describe, expect, it, vi } from "vitest";
import { PublishCommand } from "@aws-sdk/client-sns";
import { DeliveryValidationError, SnsAlertTransport, type DeliveryOperation } from "../src/delivery.js";

const payload = '{"incident_id":"inc-1","message":"degraded"}';
const operation: DeliveryOperation = {
  operationId: "op-1",
  incidentId: "inc-1",
  kind: "initial",
  destination: "ops-email",
  payload,
  payloadDigest: createHash("sha256").update(payload).digest("hex"),
};

function transport(send: ReturnType<typeof vi.fn>) {
  return new SnsAlertTransport({ client: { send } as never, topicArn: "arn:aws:sns:us-east-1:123456789012:alerts", destination: "ops-email", maxPayloadBytes: 256 * 1024 });
}

describe("SnsAlertTransport", () => {
  it("accepts a real SDK PublishCommand result and preserves the operation wire", async () => {
    const send = vi.fn().mockResolvedValue({ MessageId: "sns-message-1" });
    const result = await transport(send).publish(operation);
    expect(result).toEqual({ status: "accepted", operationId: "op-1", provider: "aws-sns", providerMessageId: "sns-message-1" });
    expect(send).toHaveBeenCalledOnce();
    const command = send.mock.calls[0][0] as PublishCommand;
    expect(command.input).toEqual({
      TopicArn: "arn:aws:sns:us-east-1:123456789012:alerts",
      Message: payload,
      MessageAttributes: {
        operation: { DataType: "String", StringValue: "op-1" },
        incident: { DataType: "String", StringValue: "inc-1" },
        payloadDigest: { DataType: "String", StringValue: operation.payloadDigest },
      },
    });
  });

  it.each([
    [new Error("timeout"), "provider_outcome_unknown"],
    [{}, "malformed_provider_result"],
    [{ MessageId: " " }, "malformed_provider_result"],
  ])("returns unknown for ambiguous or malformed provider outcome", async (response, reason) => {
    const send = vi.fn().mockImplementation(() => response instanceof Error ? Promise.reject(response) : Promise.resolve(response));
    await expect(transport(send).publish(operation)).resolves.toEqual({ status: "unknown", operationId: "op-1", reason });
  });

  it("retries with byte-identical publish parameters and no in-memory success cache", async () => {
    const send = vi.fn().mockResolvedValue({ MessageId: "sns-message-1" });
    const instance = transport(send);
    await instance.publish(operation);
    await instance.publish(operation);
    expect(send).toHaveBeenCalledTimes(2);
    expect((send.mock.calls[0][0] as PublishCommand).input).toEqual((send.mock.calls[1][0] as PublishCommand).input);
  });

  it("refuses invalid destination, digest, and payload before SNS I/O", async () => {
    const send = vi.fn();
    await expect(transport(send).publish({ ...operation, destination: "other" })).rejects.toBeInstanceOf(DeliveryValidationError);
    await expect(transport(send).publish({ ...operation, payloadDigest: "0".repeat(64) })).rejects.toBeInstanceOf(DeliveryValidationError);
    await expect(transport(send).publish({ ...operation, payload: "not-the-digest" })).rejects.toBeInstanceOf(DeliveryValidationError);
    expect(send).not.toHaveBeenCalled();
  });

  it("rejects FIFO topics and oversized configuration", () => {
    expect(() => new SnsAlertTransport({ client: {} as never, topicArn: "alerts.fifo", destination: "ops-email", maxPayloadBytes: 1 })).toThrow(DeliveryValidationError);
    expect(() => new SnsAlertTransport({ client: {} as never, topicArn: "alerts", destination: "ops-email", maxPayloadBytes: 256 * 1024 + 1 })).toThrow(DeliveryValidationError);
  });
});
