import { createHash, generateKeyPairSync, sign, verify } from "node:crypto";
import { createAck } from "../../cost-monitor/src/acks.ts";
import { createHistoricalTerminal } from "../../cost-monitor/src/terminal_ack.ts";

export function makeRsaSigner() {
  const pair = generateKeyPairSync("rsa", { modulusLength: 3072 });
  const identity = {
    keyId: "rsa-ingest", epoch: "9", keyArn: "arn:aws:kms:test:key/rsa",
    publicKeySpkiPem: pair.publicKey.export({ type: "spki", format: "pem" }).toString(), role: "ingest-ack",
  };
  const signer = { identity, async sign(bytes) {
    return sign(null, createHash("sha256").update(bytes).digest(), { key: pair.privateKey, padding: 6, saltLength: 32 }).toString("base64url");
  } };
  const verifier = {
    async verify(payload, signature, keyId, epoch) {
      if (keyId !== identity.keyId || epoch !== identity.epoch) return "invalid";
      return verify(null, createHash("sha256").update(payload).digest(), { key: pair.publicKey, padding: 6, saltLength: 32 }, Buffer.from(signature, "base64url")) ? "valid" : "invalid";
    },
  };
  return { signer, verifier };
}

export async function createTerminal(ackFields, signer) {
  const ack = await createAck(ackFields, signer);
  return createHistoricalTerminal(ack, signer);
}
