import { validateConfig, type MonitorConfig, type MonitorEnvelope } from "./types.js";
import type { AckRecoveryService } from "./ack_recovery.js";
import type { PageAckService } from "./page_ack.js";
import type { MonitorScheduler } from "./scheduler.js";
export interface ApiEvent { version?:string; rawPath?:string; requestContext?:{http?:{method?:string}}; body?:string|null; headers?:Record<string,string|undefined> }
export interface ApiResponse { statusCode:number; headers?:Record<string,string>; body:string }
export interface RuntimeDependencies { config:MonitorConfig; ingest:{ingest(envelope:unknown):Promise<unknown>}; recovery:Pick<AckRecoveryService,"recover">; pageAck:Pick<PageAckService,"acknowledge">; scheduler:Pick<MonitorScheduler,"run">; clock:{now():Promise<{timeMs:number}>} }
function json(statusCode:number,value:unknown,headers:Record<string,string>={}):ApiResponse{return {statusCode,headers:{"content-type":"application/json",...headers},body:JSON.stringify(value)}}
function parseBody(event:ApiEvent):unknown {if(typeof event.body!=="string")throw new Error("body required");return JSON.parse(event.body)}
function authorization(headers:Record<string,string|undefined>):string {return headers.authorization??headers.Authorization??""}
function path(event:ApiEvent):string {return event.rawPath??""}
function mapError(error:unknown):ApiResponse {const name=error instanceof Error?error.name:"";if(name.includes("Validation")||name.includes("Integrity"))return json(400,{error:"REJECTED"});if(name.includes("Authorization")||name.includes("principal"))return json(401,{error:"UNAUTHORIZED"});return json(503,{error:"UNKNOWN"},{"retry-after":"1"})}
export function createHandler(deps:RuntimeDependencies){if(!deps||!deps.config)throw new TypeError("runtime dependencies are required");return async(event:ApiEvent):Promise<ApiResponse>=>{try{const method=event.requestContext?.http?.method??"";const p=path(event);if(method!=="POST")return json(404,{error:"NOT_FOUND"});if(p==="/v1/ingest")return json(200,await deps.ingest.ingest(parseBody(event)));if(p==="/v1/ack/recovery")return json(200,await deps.recovery.recover(parseBody(event) as any));const m=/^\/v1\/incidents\/([^/]+)\/pages\/([^/]+)\/ack$/.exec(p);if(m){const body=parseBody(event) as any;return json(200,await deps.pageAck.acknowledge({...body,incident_id:body.incident_id??m[1],page_id:body.page_id??m[2]},{authorization:authorization(event.headers??{}),method,path:p}))}return json(404,{error:"NOT_FOUND"})}catch(error){return mapError(error)}}}
export async function handler(event:ApiEvent):Promise<ApiResponse>{throw new Error("runtime is not initialized: MONITOR_CONFIG_JSON and concrete AWS bindings are required")}
export function loadConfig(raw=process.env.MONITOR_CONFIG_JSON):MonitorConfig {if(!raw)throw new Error("MONITOR_CONFIG_JSON is required");let parsed:unknown;try{parsed=JSON.parse(raw)}catch{throw new Error("MONITOR_CONFIG_JSON is invalid JSON")}return validateConfig(parsed)}
