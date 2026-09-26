/** Build a complete accepted receipt for the exact event batch a test submits. */
export function acceptedBillingResponse(init: RequestInit | undefined): Response {
  const events = JSON.parse(String(init?.body)) as { idem_key: string }[];
  return new Response(JSON.stringify({
    outcomes: events.map((event, index) => ({ index, idem_key: event.idem_key, outcome: "accepted" })),
    accepted: events.length,
    deduped: 0,
    rejected: 0,
    total: events.length,
  }), { status: 202, headers: { "content-type": "application/json" } });
}
