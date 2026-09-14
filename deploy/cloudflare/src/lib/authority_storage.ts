/** Frozen shared storage boundary; domain modules own no RPC or bindings. */
export type AuthorityStorage = Pick<DurableObjectStorage, "get" | "put" | "delete" | "list" | "transaction">;
export type AuthorityTransaction = Pick<DurableObjectTransaction, "get" | "put" | "delete" | "list">;
