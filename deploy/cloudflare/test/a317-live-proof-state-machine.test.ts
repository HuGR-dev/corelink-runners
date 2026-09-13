import { describe, expect, it } from "vitest";
import { NormalIntakeInbox, type A317ProofPhase, type A317ProofRecord } from "../src/lib/normal_intake_inbox";

/** A serialized Durable Object storage double with transaction rollback. */
class Store {
  data = new Map<string, unknown>();
  private tail = Promise.resolve();

  async get<T>(key: string) { return this.data.get(key) as T | undefined; }
  async put(key: string, value: unknown) { this.data.set(key, value); }
  async delete(key: string) { this.data.delete(key); }
  async list<T>(options: { prefix?: string; limit?: number }) {
    return new Map([...this.data.entries()]
      .filter(([key]) => key.startsWith(options.prefix ?? ""))
      .sort(([a], [b]) => a.localeCompare(b))
      .slice(0, options.limit ?? Infinity) as [string, T][]);
  }
  async transaction<T>(fn: (tx: Store) => Promise<T>) {
    const run = this.tail.then(async () => {
      const copy = new Map(this.data);
      const tx = Object.create(this) as Store;
      tx.data = copy;
      const result = await fn(tx);
      this.data = copy;
      return result;
    });
    this.tail = run.then(() => undefined, () => undefined);
    return run;
  }
}

const RUN = "11111111-1111-4111-8111-111111111111";
const BUILD = "abcdef1";
const T0 = 1_000_000;
const sha = (char: string) => char.repeat(64);
const input = (event_id: string, body_sha256 = sha("a")) => ({
  schema_version: 1 as const, event_id, body_sha256, job_id: event_id,
  repo: "Owner/Repo", installation_id: "42", labels: ["self-hosted"], received_at_ms: T0,
});
const proof = (phase: A317ProofPhase, index: number, nonce = `nonce-${phase}-${index}`): A317ProofRecord => ({
  schema_version: 1, run_id: RUN, phase, index, nonce, expires_at_ms: T0 + 120_000,
  build_sha: BUILD, event_id: "ignored", body_sha256: sha("b"), authorization_attempts: 0, authorization_refusals: 0,
});
const eventId = (phase: A317ProofPhase, index: number) => `a317:v1:${RUN}:${phase}:${index}`;

// These calls are kept in one adapter so the test documents the source seam
// precisely while allowing the writer to choose the result's literal shape.
type A317Inbox = NormalIntakeInbox & {
  a317Pending(runId: string, phase: A317ProofPhase | undefined, now: number, limit: number): Promise<unknown[]>;
  beginA317Authorization(eventId: string): Promise<boolean>;
  finishA317Authorization(eventId: string, status: "refused401" | "refused403" | "accepted2xx" | "unknown"): Promise<void>;
};
const a317 = (inbox: NormalIntakeInbox) => inbox as A317Inbox;

describe("A3.17 live proof state machine", () => {
  it("admits 100 unique proofs per phase and never exposes ordinary intake as proof work", async () => {
    const storage = new Store();
    const inbox = a317(new NormalIntakeInbox(storage as never));

    for (const phase of ["missing_key", "wrong_key", "store_unavailable"] as const) {
      for (let index = 0; index < 100; index++) {
        await expect(inbox.enqueueA317Proof(input(eventId(phase, index)), proof(phase, index), T0))
          .resolves.toMatchObject({ status: "accepted" });
      }
    }
    await inbox.enqueue(input("ordinary-event"), T0);

    expect(await inbox.a317Pending(RUN, undefined, T0, 400)).toHaveLength(300);
    expect(await inbox.a317Pending(RUN, "missing_key", T0, 200)).toHaveLength(100);
    expect((await inbox.a317Pending(RUN, undefined, T0, 400)).some((record: any) => record.event_id === "ordinary-event")).toBe(false);
  });

  it("binds slot, nonce, event and body immutably, including late exact redelivery", async () => {
    const storage = new Store();
    const inbox = a317(new NormalIntakeInbox(storage as never));
    const id = eventId("missing_key", 0);
    const marker = proof("missing_key", 0);

    await expect(inbox.enqueueA317Proof(input(id), marker, T0)).resolves.toMatchObject({ status: "accepted" });
    await expect(inbox.enqueueA317Proof(input(id), marker, T0 + 61_000)).resolves.toMatchObject({ status: "duplicate" });
    await expect(inbox.enqueueA317Proof(input(id, sha("c")), marker, T0 + 61_001)).resolves.toMatchObject({ status: "conflict" });
    await expect(inbox.enqueueA317Proof(input(eventId("missing_key", 0)), proof("missing_key", 0, "different"), T0 + 61_002)).resolves.toMatchObject({ status: "conflict" });
    await expect(inbox.enqueueA317Proof(input(eventId("missing_key", 1)), proof("missing_key", 0, "nonce-new"), T0 + 61_003)).resolves.toMatchObject({ status: "conflict" });
  });

  it("allows exactly one authorization attempt and latches 401, 403, 2xx and unknown outcomes", async () => {
    const storage = new Store();
    const inbox = a317(new NormalIntakeInbox(storage as never));
    const statuses = ["refused401", "refused403", "accepted2xx", "unknown"] as const;

    for (let i = 0; i < statuses.length; i++) {
      const id = eventId("wrong_key", i);
      await inbox.enqueueA317Proof(input(id), proof("wrong_key", i), T0);
      expect(await inbox.beginA317Authorization(id)).toBe(true);
      expect(await inbox.beginA317Authorization(id)).toBe(false);
      await inbox.finishA317Authorization(id, statuses[i]);
      await inbox.finishA317Authorization(id, statuses[i]);
    }
    const snapshot = await inbox.a317Snapshot(RUN, T0 + 1);
    expect(snapshot.authorization_attempts).toBe(4);
    expect(snapshot.authorization_refusals).toBe(2);
  });

  it("removes expired proof-owned event, pending, nonce and slot state atomically", async () => {
    const storage = new Store();
    const inbox = a317(new NormalIntakeInbox(storage as never));
    const id = eventId("store_unavailable", 0);
    await inbox.enqueueA317Proof(input(id), proof("store_unavailable", 0), T0);
    expect(await inbox.a317Pending(RUN, undefined, T0, 10)).toHaveLength(1);

    await inbox.cleanupExpiredA317Proofs(T0 + 120_000);
    expect(await inbox.a317Pending(RUN, undefined, T0 + 120_000, 10)).toHaveLength(0);
    expect(await inbox.a317Snapshot(RUN, T0 + 120_000)).toMatchObject({ accepted: 0, pending: 0 });
    expect([...storage.data.keys()].filter((key) => key.includes("a317") || key.includes(id))).toEqual([]);
  });

  it("does not leave durable state when the injected store-unavailable phase fails", async () => {
    const storage = new Store();
    const inbox = a317(new NormalIntakeInbox(storage as never));
    const id = eventId("store_unavailable", 1);
    await expect(inbox.enqueueA317Proof(input(id), proof("store_unavailable", 1), T0, true)).rejects.toThrow(/unavailable/);
    expect([...storage.data.keys()].filter((key) => key.includes("a317") || key.includes(id))).toEqual([]);
    expect(await inbox.pending(T0, 25)).toEqual([]);
  });
});
