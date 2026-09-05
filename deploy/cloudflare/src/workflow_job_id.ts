import { canonicalSafeJobId } from "./containment_authority_helpers";

function whitespaceAt(value: string, position: number): number {
  while (position < value.length && /[\u0009\u000a\u000c\u000d\u0020]/.test(value[position] ?? "")) position++;
  return position;
}

function stringEnd(value: string, start: number): number {
  if (value[start] !== '"') return -1;
  for (let i = start + 1; i < value.length; i++) {
    const ch = value[i];
    if (ch === "\\") { i++; if (value[i] === "u") i += 4; continue; }
    if (ch === '"') return i + 1;
    if (ch < " ") return -1;
  }
  return -1;
}

function valueEnd(value: string, start: number): number {
  const ch = value[start];
  if (ch === '"') return stringEnd(value, start);
  if (ch === "{" || ch === "[") {
    const stack: string[] = [ch === "{" ? "}" : "]"];
    for (let i = start + 1; i < value.length; i++) {
      if (value[i] === '"') { const end = stringEnd(value, i); if (end < 0) return -1; i = end - 1; continue; }
      if (value[i] === "{" || value[i] === "[") stack.push(value[i] === "{" ? "}" : "]");
      else if (value[i] === "}" || value[i] === "]") {
        if (stack.pop() !== value[i]) return -1;
        if (stack.length === 0) return i + 1;
      }
    }
    return -1;
  }
  let i = start;
  while (i < value.length && !/[\u0009-\u000d\u0020,}\]]/.test(value[i] ?? "")) i++;
  return i === start ? -1 : i;
}

function canonicalToken(token: string): string | null {
  if (/^[1-9][0-9]*$/.test(token)) return canonicalSafeJobId(token);
  // The contract accepts only the literal canonical digit spelling; escaped
  // digits would be a different wire token even if JSON decodes them equally.
  if (token.startsWith('"') && token.endsWith('"') && /^[1-9][0-9]*$/.test(token.slice(1, -1))) return canonicalSafeJobId(token.slice(1, -1));
  return null;
}

function workflowJobId(value: string, start: number): { end: number; id: string | null; invalid: boolean } {
  if (value[start] !== "{") return { end: valueEnd(value, start), id: null, invalid: true };
  let position = whitespaceAt(value, start + 1); let found: string | null = null; let sawId = false;
  while (position < value.length && value[position] !== "}") {
    const keyEnd = stringEnd(value, position);
    if (keyEnd < 0) return { end: -1, id: null, invalid: true };
    let key: unknown;
    try { key = JSON.parse(value.slice(position, keyEnd)); } catch { return { end: -1, id: null, invalid: true }; }
    position = whitespaceAt(value, keyEnd);
    if (value[position] !== ":") return { end: -1, id: null, invalid: true };
    position = whitespaceAt(value, position + 1);
    const end = valueEnd(value, position);
    if (end < 0) return { end: -1, id: null, invalid: true };
    if (key === "id") {
      if (sawId) return { end: -1, id: null, invalid: true };
      sawId = true; found = canonicalToken(value.slice(position, end));
      if (found === null) return { end, id: null, invalid: true };
    }
    position = whitespaceAt(value, end);
    if (value[position] === ",") position = whitespaceAt(value, position + 1);
    else if (value[position] !== "}") return { end: -1, id: null, invalid: true };
  }
  return { end: position + 1, id: found, invalid: !sawId };
}

/** Read and validate workflow_job.id before the generic JSON decoder runs. */
export function canonicalWorkflowJobIdFromRaw(raw: string): string | null {
  let position = whitespaceAt(raw, 0);
  if (raw[position] !== "{") return null;
  position = whitespaceAt(raw, position + 1); let found: string | null = null; let sawWorkflowJob = false;
  while (position < raw.length && raw[position] !== "}") {
    const keyEnd = stringEnd(raw, position);
    if (keyEnd < 0) return null;
    let key: unknown;
    try { key = JSON.parse(raw.slice(position, keyEnd)); } catch { return null; }
    position = whitespaceAt(raw, keyEnd);
    if (raw[position] !== ":") return null;
    position = whitespaceAt(raw, position + 1);
    const end = valueEnd(raw, position);
    if (end < 0) return null;
    if (key === "workflow_job") {
      if (sawWorkflowJob) return null;
      sawWorkflowJob = true;
      const result = workflowJobId(raw, position);
      if (result.invalid) return null;
      found = result.id;
    }
    position = whitespaceAt(raw, end);
    if (raw[position] === ",") position = whitespaceAt(raw, position + 1);
    else if (raw[position] !== "}") return null;
  }
  if (raw[position] !== "}") return null;
  position = whitespaceAt(raw, position + 1);
  return position === raw.length ? found : null;
}
