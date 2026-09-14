#!/usr/bin/env node
import crypto from 'node:crypto';
const [rawKeys] = process.argv.slice(2);
let input=''; process.stdin.on('data', x => { input += x; }); process.stdin.on('end', () => {
  const close=JSON.parse(input), keys=JSON.parse(rawKeys);
  if (!Array.isArray(keys.keys)||keys.keys.length!==1||keys.keys[0].expires_ms!==null) throw Error('exact one selected key required');
  const key=keys.keys[0], raw=Buffer.from(key.pubkey_b64,'base64');
  if(raw.length!==32||crypto.createHash('sha256').update(raw).digest('hex').slice(0,16)!==key.key_id||close.fabric_key_id!==key.key_id) throw Error('key routing mismatch');
  const r=close.check_result??{memo_key:'',stdout_ref:'',stderr_ref:'',exit:0,artifacts:[]};
  const lp=s=>{const b=Buffer.from(s);const n=Buffer.alloc(4);n.writeUInt32BE(b.length);return Buffer.concat([n,b]);}; const i=n=>{const b=Buffer.alloc(4);b.writeInt32BE(n);return b;}; const u=n=>{const b=Buffer.alloc(4);b.writeUInt32BE(n);return b;};
  const v1=Buffer.concat([lp(r.memo_key),lp(r.stdout_ref),lp(r.stderr_ref)]); const v2=Buffer.concat([v1,i(r.exit),u(r.artifacts.length),...r.artifacts.flatMap(a=>[lp(a.path),lp(a.digest)])]);
  const der=Buffer.concat([Buffer.from('302a300506032b6570032100','hex'),raw]), pub=crypto.createPublicKey({key:der,format:'der',type:'spki'});
  if(!crypto.verify(null,v1,pub,Buffer.from(close.result_binding_sig,'base64'))||!crypto.verify(null,v2,pub,Buffer.from(close.result_binding_sig_v2,'base64'))) throw Error('Corelink v1/v2 rejected');
  console.log('corelink_v1_v2=accepted');
});
