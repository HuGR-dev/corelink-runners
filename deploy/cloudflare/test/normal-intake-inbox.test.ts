import { describe, expect, it } from "vitest";
import { NormalIntakeInbox } from "../src/lib/normal_intake_inbox";

class Store {
  data = new Map<string, unknown>(); fail = false; private tail = Promise.resolve();
  async get<T>(key: string) { return this.data.get(key) as T | undefined; }
  async put(key: string, value: unknown) { if (this.fail) throw new Error("put failed"); this.data.set(key, value); }
  async delete(key: string) { this.data.delete(key); }
  async list<T>(options: { prefix?: string; limit?: number }) {
    return new Map([...this.data.entries()].filter(([k]) => k.startsWith(options.prefix ?? "")).sort(([a], [b]) => a.localeCompare(b)).slice(0, options.limit ?? Infinity) as [string, T][]);
  }
  async transaction<T>(fn: (tx: Store) => Promise<T>) {
    const run = this.tail.then(async () => { const copy = new Map(this.data); const tx = Object.create(this) as Store; tx.data = copy; const result = await fn(tx); this.data = copy; return result; });
    this.tail = run.then(() => undefined, () => undefined); return run;
  }
}
const input = (event_id: string, received_at_ms = 1000, body_sha256 = "a".repeat(64)) => ({ schema_version: 1 as const, event_id, body_sha256, job_id: "1", repo: "Owner/Repo", installation_id: "42", labels: ["self-hosted"], received_at_ms });

describe("NormalIntakeInbox", () => {
  it("is idempotent, survives restart, and orders bounded pages", async () => {
    const storage = new Store(); const inbox = new NormalIntakeInbox(storage as never);
    expect((await inbox.enqueue(input("b", 2), 2)).status).toBe("accepted");
    expect((await inbox.enqueue(input("a", 1), 1)).status).toBe("accepted");
    expect((await inbox.enqueue(input("a", 1), 1)).status).toBe("duplicate");
    expect((await new NormalIntakeInbox(storage as never).pending(2, 2)).map((x) => x.event_id)).toEqual(["a", "b"]);
    expect((await inbox.pending(3, 2)).map((x) => x.event_id)).toEqual(["a", "b"]);
    await inbox.settle("a", "a".repeat(64), "complete", 3);
    expect((await inbox.pending(3)).map((x) => x.event_id)).toEqual(["b"]);
  });
  it("rejects conflicting identity and never reopens terminal records", async () => {
    const storage = new Store(); const inbox = new NormalIntakeInbox(storage as never); await inbox.enqueue(input("e"));
    expect((await inbox.enqueue(input("e", 1000, "b".repeat(64)))).status).toBe("conflict");
    await inbox.settle("e", "a".repeat(64), "uncertain", 2); await inbox.settle("e", "a".repeat(64), "retry", 3);
    expect((await inbox.pending(100)).map((x) => x.event_id)).toEqual([]);
  });
  it("enforces the active bound and rolls back failed writes", async () => {
    const storage = new Store(); const inbox = new NormalIntakeInbox(storage as never);
    for (let i = 0; i < 500; i++) await inbox.enqueue(input(`e${i}`, i + 1));
    expect((await inbox.enqueue(input("overflow", 999))).status).toBe("full");
    await inbox.settle("e0", "a".repeat(64), "complete", 2000);
    storage.fail = true; await expect(inbox.enqueue(input("rollback", 1001), 1001)).rejects.toThrow(); storage.fail = false;
    expect((await inbox.enqueue(input("overflow", 999))).status).toBe("accepted");
  });
  it("uses durable retry timing and removes completed capacity", async () => {
    const storage = new Store(); const inbox = new NormalIntakeInbox(storage as never);
    await inbox.enqueue(input("retry"), 100, 500); expect((await inbox.pending(100)).length).toBe(0); expect((await inbox.pending(601))[0].event_id).toBe("retry");
    await inbox.settle("retry", "a".repeat(64), "complete", 601); expect((await inbox.pending(601)).length).toBe(0); expect((await inbox.enqueue(input("new"))).status).toBe("accepted");
  });
  it("serializes concurrent admissions and rejects malformed or key-crossing identities", async () => {
    const storage = new Store(); const inbox = new NormalIntakeInbox(storage as never);
    const results = await Promise.all(Array.from({ length: 100 }, (_, i) => inbox.enqueue(input(`same/${i}`, i), i)));
    expect(results.filter((x) => x.status === "accepted")).toHaveLength(100);
    await expect(inbox.enqueue({ ...input("bad"), repo: "evil/../repo" })).rejects.toThrow();
    await expect(inbox.enqueue({ ...input("x%2Fy"), repo: "acme/repo" })).resolves.toMatchObject({ status: "accepted" });
    await expect(inbox.enqueue({ ...input("x%2Fy"), repo: "other/repo" })).resolves.toMatchObject({ status: "conflict" });
    storage.data.set("normal-inbox:v1:event:broken", { schema_version: 1 });
    await expect(inbox.settle("broken", "a".repeat(64), "complete", 4)).rejects.toThrow(/corruption/);
  });
  it("looks past delayed successors without unbounded reads", async () => {
    const storage = new Store(); const inbox = new NormalIntakeInbox(storage as never);
    for (let i = 0; i < 499; i++) await inbox.enqueue(input(`delayed-${i}`, i), 0, 10000);
    await inbox.enqueue(input("ready", 499), 0);
    expect((await inbox.pending(1, 1)).map((x) => x.event_id)).toEqual(["ready"]);
  });
  it("refuses a missing counter and divergent index, and strips non-contract fields", async () => {
    const storage = new Store(); const inbox = new NormalIntakeInbox(storage as never);
    await inbox.enqueue({ ...input("safe"), raw_payload: "must-not-persist", token: "must-not-persist" } as never, 0);
    expect(storage.data.get("normal-inbox:v1:event:safe")).not.toHaveProperty("token");
    expect(storage.data.get("normal-inbox:v1:event:safe")).not.toHaveProperty("raw_payload");
    storage.data.delete("normal-inbox:v1:count");
    await expect(inbox.enqueue(input("other"), 0)).rejects.toThrow("missing active count");
    storage.data.set("normal-inbox:v1:pending:0000000000000000:wrong", "safe");
    await expect(inbox.pending(1001)).rejects.toThrow("pending index/state mismatch");
    await expect(inbox.enqueue({ ...input("bad"), schema_version: 2 } as never, 0)).rejects.toThrow();
  });
});
