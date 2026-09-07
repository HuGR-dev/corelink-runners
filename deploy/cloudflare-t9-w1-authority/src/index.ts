export interface Env {
  T9_AUTHORITY_DB: D1Database; T9_TENANT_ADMISSION: DurableObjectNamespace; T9_AUTHORITY_ID: string; T9_GRANT_KEY_ID: string;
  T9_GRANT_PUBLIC_KEY: string; T9_TERMINAL_PUBLIC_KEY: string; T9_TERMINAL_SIGNING_PRIVATE_KEY: string;
  T9_MAX_VCPU_MS: string; T9_MAX_WALL_MS: string; T9_EXPIRES_AT_MS: string; T9_EXECUTOR_PROTOCOL: string;
  T9_SOURCE_SHA256: string;
}
type Grant = { v: 1; key_id: string; tenant_id: string; workload_kind: "devenv"; workload_id: string; reservation_id: string; period_key: number; ceiling_vcpu_ms: string; vcpu_count: number; maximum_wall_ms: number; issued_at_ms: number; expires_at_ms: number };
type Row = { grant_digest: string; state: "prepared" | "cancelled"; receipt_json: string | null; receipt_envelope_json: string | null; generation: string };
type StrictReceipt = { reservation_id: string; state: "cancelled"; materialized: false; actual_vcpu_ms: "0"; evidence_digest: string; future_materialization_fence: string; terminal_authority: string; authority_signature: string };
type Envelope = { receipt_version: "t9-w1-terminal-v2"; reservation_id: string; tenant_id: string; grant_digest: string; generation: string; state: "cancelled"; materialized: false; actual_vcpu_ms: "0"; evidence_digest: string; future_materialization_fence: string; authority: string; key_id: string; alg: "Ed25519"; signed_at_ms: number; expires_at_ms: number; signature: string };
const UUID=/^(?!00000000-0000-0000-0000-000000000000$)[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i, WORKLOAD=/^[A-Za-z0-9:_./-]{1,256}$/, DECIMAL=/^(0|[1-9][0-9]{0,18})$/, MAX_TOKEN_BYTES=8192;

export default { async fetch(request: Request, env: Env): Promise<Response> {
  const expiry=number(env.T9_EXPIRES_AT_MS); if (Date.now() >= expiry) return json({error:"acceptance_target_expired"},410);
  const url=new URL(request.url);
  if (request.method === "GET" && url.pathname === "/internal/v1/compute/acceptance-binding") return json({
    authority_origin:url.origin, authority_id:env.T9_AUTHORITY_ID, grant_key_id:env.T9_GRANT_KEY_ID, grant_public_key:env.T9_GRANT_PUBLIC_KEY,
    terminal_public_key:env.T9_TERMINAL_PUBLIC_KEY, max_vcpu_ms:env.T9_MAX_VCPU_MS, max_wall_ms:env.T9_MAX_WALL_MS, expires_at_ms:env.T9_EXPIRES_AT_MS,
    executor_protocol:env.T9_EXECUTOR_PROTOCOL, compute_materialization:"disabled", deployment_source_sha256:env.T9_SOURCE_SHA256
  });
  const match=request.method === "POST" && /^\/internal\/v1\/compute\/(reserve|activate|cancel|settle)$/.exec(url.pathname);
  if (!match) return json({error:"not_found"},404);
  if ((request.headers.get("content-type")||"").split(";",1)[0] !== "application/json") return json({error:"invalid_request"},400);
  const token=request.headers.get("authorization")?.replace(/^ComputeGrant /,"");
  if (!token || token===request.headers.get("authorization")) return json({error:"invalid_compute_grant"},401);
  let verified: {grant:Grant;digest:string}; try { verified=await verify(token,env,expiry,match[1]==="cancel"); } catch { return json({error:"invalid_compute_grant"},401); }
  let body:unknown; try { body=await request.json(); } catch { return json({error:"invalid_request"},400); }
  if (!empty(body)) return json({error:"invalid_request"},400);
  if (match[1]==="reserve") return reserve(env,verified.grant,verified.digest);
  if (match[1]==="activate") return activate(env,verified.grant,verified.digest);
  if (match[1]==="settle") return json({error:"executor_protocol_unpromoted"},503);
  return cancel(env,verified.grant,verified.digest,expiry);
}} satisfies ExportedHandler<Env>;

async function reserve(env:Env,g:Grant,digest:string):Promise<Response> {
  const required=BigInt(g.vcpu_count)*BigInt(g.maximum_wall_ms), ceiling=BigInt(g.ceiling_vcpu_ms), maximum=BigInt(number(env.T9_MAX_VCPU_MS));
  if(required>ceiling || required>maximum) return json({error:"monthly_compute_refused"},429);
  return env.T9_TENANT_ADMISSION.get(env.T9_TENANT_ADMISSION.idFromName(g.tenant_id)).fetch("https://tenant/reserve",{method:"POST",headers:{"content-type":"application/json"},body:JSON.stringify({grant:g,digest,required:required.toString()})});
}

export class TenantAdmission implements DurableObject {
  private tail: Promise<void> = Promise.resolve();
  constructor(private readonly state: DurableObjectState, private readonly env: Env) {}
  async fetch(request: Request): Promise<Response> {
    let release!: () => void; const previous=this.tail; this.tail=new Promise<void>(resolve => { release=resolve; });
    await previous; try { return await this.reserve(request); } finally { release(); }
  }
  private async reserve(request: Request): Promise<Response> {
    if (request.method !== "POST" || new URL(request.url).pathname !== "/reserve") return json({error:"not_found"},404);
    let input: { grant: Grant; digest: string; required: string }; try { input=await request.json(); } catch { return json({error:"invalid_request"},400); }
    const {grant:g,digest,required}=input; if (!g || typeof digest!=="string" || !DECIMAL.test(required) || required==="0" || g.tenant_id !== this.state.id.name) return json({error:"invalid_request"},400);
    const prior=await row(this.env,g.reservation_id);
    if(prior) return prior.grant_digest===digest && prior.state==="prepared" ? json({reservation_id:g.reservation_id,state:"prepared"}) : json({error:"reservation_conflict"},409);
    const used=await this.env.T9_AUTHORITY_DB.prepare("SELECT COALESCE(SUM(CAST(required_vcpu_ms AS INTEGER)),0) AS used FROM reservations WHERE tenant_id=?1 AND state='prepared'").bind(g.tenant_id).first<{used:number}>();
    if(BigInt(used?.used??0)+BigInt(required)>BigInt(g.ceiling_vcpu_ms)) return json({error:"monthly_compute_refused"},429);
    try { await this.env.T9_AUTHORITY_DB.prepare("INSERT INTO reservations(reservation_id,grant_digest,tenant_id,workload_id,required_vcpu_ms,ceiling_vcpu_ms,state,created_at_ms,generation) VALUES(?1,?2,?3,?4,?5,?6,'prepared',?7,?8)").bind(g.reservation_id,digest,g.tenant_id,g.workload_id,required,g.ceiling_vcpu_ms,Date.now(),generation(digest)).run(); }
    catch { const replay=await row(this.env,g.reservation_id); return replay?.grant_digest===digest && replay.state==="prepared" ? json({reservation_id:g.reservation_id,state:"prepared"}) : json({error:"reservation_conflict"},409); }
    return json({reservation_id:g.reservation_id,state:"prepared"});
  }
}
async function activate(env:Env,g:Grant,digest:string):Promise<Response> { const prior=await row(env,g.reservation_id); if(!prior||prior.grant_digest!==digest) return json({error:"reservation_conflict"},409); if(prior.state==="cancelled") return json({error:"reservation_cancelled"},409); return json({error:"executor_protocol_unpromoted"},503); }
async function cancel(env:Env,g:Grant,digest:string,expiry:number):Promise<Response> {
  const prior=await row(env,g.reservation_id); if(prior?.grant_digest && prior.grant_digest!==digest) return json({error:"reservation_conflict"},409);
  if(prior?.state==="cancelled" && prior.receipt_json) return new Response(prior.receipt_json,{headers:{"content-type":"application/json","cache-control":"no-store"}});
  const gen=prior?.generation || generation(digest); const envelope=await envelopeFor(env,g,digest,gen,expiry); const strict:StrictReceipt={reservation_id:g.reservation_id,state:"cancelled",materialized:false,actual_vcpu_ms:"0",evidence_digest:envelope.evidence_digest,future_materialization_fence:envelope.future_materialization_fence,terminal_authority:envelope.authority,authority_signature:envelope.signature};
  if(prior) await env.T9_AUTHORITY_DB.prepare("UPDATE reservations SET state='cancelled',receipt_json=?2,receipt_envelope_json=?3 WHERE reservation_id=?1").bind(g.reservation_id,JSON.stringify(strict),JSON.stringify(envelope)).run();
  else await env.T9_AUTHORITY_DB.prepare("INSERT INTO reservations(reservation_id,grant_digest,tenant_id,workload_id,required_vcpu_ms,ceiling_vcpu_ms,state,created_at_ms,generation,receipt_json,receipt_envelope_json) VALUES(?1,?2,?3,?4,'0',?5,'cancelled',?6,?7,?8,?9)").bind(g.reservation_id,digest,g.tenant_id,g.workload_id,g.ceiling_vcpu_ms,Date.now(),gen,JSON.stringify(strict),JSON.stringify(envelope)).run();
  return json(strict);
}
async function envelopeFor(env:Env,g:Grant,digest:string,gen:string,expiry:number):Promise<Envelope> {
  const evidence_digest=await sha(`t9-w1-cancel:${g.reservation_id}:${digest}`), future_materialization_fence=await sha(`t9-w1-fence:${g.reservation_id}:${digest}`), signed_at_ms=Date.now();
  const unsigned= {receipt_version:"t9-w1-terminal-v2" as const,reservation_id:g.reservation_id,tenant_id:g.tenant_id,grant_digest:digest,generation:gen,state:"cancelled" as const,materialized:false as const,actual_vcpu_ms:"0" as const,evidence_digest,future_materialization_fence,authority:env.T9_AUTHORITY_ID,key_id:"t9w1-terminal-20260907",alg:"Ed25519" as const,signed_at_ms,expires_at_ms:expiry};
  const key=await crypto.subtle.importKey("pkcs8",b64(env.T9_TERMINAL_SIGNING_PRIVATE_KEY),{name:"Ed25519"},false,["sign"]); const signature=base64url(new Uint8Array(await crypto.subtle.sign("Ed25519",key,new TextEncoder().encode(JSON.stringify(unsigned)))));
  return {...unsigned,signature};
}
async function row(env:Env,id:string):Promise<Row|undefined>{return (await env.T9_AUTHORITY_DB.prepare("SELECT grant_digest,state,receipt_json,receipt_envelope_json,generation FROM reservations WHERE reservation_id=?1").bind(id).first<Row>()) ?? undefined;}
async function verify(token:string,env:Env,targetExpiry:number,allowExpired:boolean):Promise<{grant:Grant;digest:string}>{if(new TextEncoder().encode(token).byteLength>MAX_TOKEN_BYTES)throw 0;const[p,s,x]=token.split(".");if(!p||!s||x!==undefined)throw 0;const bytes=b64(p),sig=b64(s),raw:unknown=JSON.parse(new TextDecoder().decode(bytes));if(sig.byteLength!==64||!grant(raw,env,targetExpiry,allowExpired))throw 0;const key=await crypto.subtle.importKey("raw",b64(env.T9_GRANT_PUBLIC_KEY),{name:"Ed25519"},false,["verify"]);if(!await crypto.subtle.verify("Ed25519",key,sig,bytes))throw 0;return{grant:raw,digest:await sha(token)}}
function grant(v:unknown,env:Env,target:number,allowExpired:boolean):v is Grant {if(!v||typeof v!=="object"||Array.isArray(v))return false;const g=v as Record<string,unknown>, ks=["v","key_id","tenant_id","workload_kind","workload_id","reservation_id","period_key","ceiling_vcpu_ms","vcpu_count","maximum_wall_ms","issued_at_ms","expires_at_ms"],period=g.period_key as number;return Object.keys(g).length===ks.length&&ks.every(k=>k in g)&&g.v===1&&g.key_id===env.T9_GRANT_KEY_ID&&typeof g.tenant_id==="string"&&UUID.test(g.tenant_id)&&g.workload_kind==="devenv"&&typeof g.workload_id==="string"&&WORKLOAD.test(g.workload_id)&&typeof g.reservation_id==="string"&&UUID.test(g.reservation_id)&&Number.isSafeInteger(period)&&period>=197001&&period<=999912&&period%100>=1&&period%100<=12&&typeof g.ceiling_vcpu_ms==="string"&&DECIMAL.test(g.ceiling_vcpu_ms)&&BigInt(g.ceiling_vcpu_ms)>0n&&typeof g.vcpu_count==="number"&&g.vcpu_count===1&&typeof g.maximum_wall_ms==="number"&&Number.isSafeInteger(g.maximum_wall_ms)&&g.maximum_wall_ms>0&&g.maximum_wall_ms<=number(env.T9_MAX_WALL_MS)&&typeof g.issued_at_ms==="number"&&typeof g.expires_at_ms==="number"&&Number.isSafeInteger(g.issued_at_ms)&&Number.isSafeInteger(g.expires_at_ms)&&g.issued_at_ms<g.expires_at_ms&&g.expires_at_ms<=target&&g.expires_at_ms-g.issued_at_ms<=90000&&(allowExpired||Date.now()<g.expires_at_ms)}
function generation(d:string){return `g-${d.slice(0,16)}`}; function empty(v:unknown):v is Record<string,never>{return!!v&&typeof v==="object"&&!Array.isArray(v)&&Object.keys(v).length===0};function number(v:string){if(!/^[1-9][0-9]*$/.test(v))throw new Error("invalid authority configuration");return Number(v)};function json(v:unknown,status=200){return new Response(JSON.stringify(v),{status,headers:{"content-type":"application/json","cache-control":"no-store"}})};function b64(v:string){const p=v.replace(/-/g,"+").replace(/_/g,"/")+"=".repeat((4-v.length%4)%4);return Uint8Array.from(atob(p),c=>c.charCodeAt(0))};function base64url(v:Uint8Array){let s="";for(const b of v)s+=String.fromCharCode(b);return btoa(s).replace(/\+/g,"-").replace(/\//g,"_").replace(/=+$/,"")};async function sha(v:string){const d=new Uint8Array(await crypto.subtle.digest("SHA-256",new TextEncoder().encode(v)));return Array.from(d,b=>b.toString(16).padStart(2,"0")).join("")}
