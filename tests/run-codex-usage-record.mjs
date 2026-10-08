import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { parse } from '../src-tauri/resources/cli/src/parsers/codex.js';
const at=n=>new Date(Date.UTC(2026,8,20,0,0,n)).toISOString();
const u=(input=100,cached=20,output=10,reasoning=2)=>({input_tokens:input,cached_input_tokens:cached,output_tokens:output,reasoning_output_tokens:reasoning,total_tokens:input+output});
const meta=(id='session',second=0,extra={})=>({timestamp:at(second),type:'session_meta',payload:{id,timestamp:at(second),cwd:'C:/test',...extra}});
const context={timestamp:at(0),type:'turn_context',payload:{model:'test-model'}};
const record=(n=1,total=u(),usage=u())=>({timestamp:at(n),type:'token_usage_record',payload:{usage,thread_token_usage:total}});
const legacy=(n=2,total=u(),usage=u())=>({timestamp:at(n),type:'event_msg',payload:{type:'token_count',info:{last_token_usage:usage,total_token_usage:total}}});
async function fixture(files,fn) {
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'vbu-record-fixture-')); const dir=path.join(root,'sessions');fs.mkdirSync(dir);
 const saved={};for(const k of ['CODEX_HOME','VIBE_USAGE_CACHE_DIR','VIBE_USAGE_CINDY_DIRS'])saved[k]=process.env[k];
 process.env.CODEX_HOME=root;process.env.VIBE_USAGE_CACHE_DIR=path.join(root,'cache');process.env.VIBE_USAGE_CINDY_DIRS=path.join(root,'absent');
 for(const [name,rows]of Object.entries(files))fs.writeFileSync(path.join(dir,name),rows.map(JSON.stringify).join('\n')+'\n');
 try{return await fn(dir)}finally{for(const[k,v]of Object.entries(saved))if(v===undefined)delete process.env[k];else process.env[k]=v;
 // root is an absolute mkdtemp child of the OS temporary directory, never user data.
 if(!path.resolve(root).startsWith(path.resolve(os.tmpdir())+path.sep))throw Error('unsafe fixture cleanup');
 fs.rmSync(root,{recursive:true,force:true});}
}
const total=r=>r.buckets.reduce((n,b)=>n+b.inputTokens+b.outputTokens+b.cachedInputTokens+b.reasoningOutputTokens,0);
test('new record without UI event is counted',()=>fixture({'a.jsonl':[meta(),context,record()]},async()=>assert.equal(total(await parse()),110)));
test('mirrors with different cumulative counters are counted once',()=>fixture({'a.jsonl':[meta(),context,record(),legacy(2,u(90)),record(3,u(200,40,20,4)),legacy(4,u(180,40,20,4))]},async()=>assert.equal(total(await parse()),220)));
test('missing mirror does not suppress the next real request',()=>fixture({'a.jsonl':[meta(),context,record(),record(3,u(200,40,20,4)),legacy(4,u())]},async()=>assert.equal(total(await parse()),220)));
test('mirror arriving after cache checkpoint does not double-count',()=>fixture({'a.jsonl':[meta(),context,record()]},async dir=>{
 assert.equal(total(await parse()),110);
 fs.appendFileSync(path.join(dir,'a.jsonl'),JSON.stringify(legacy(2,u(90)))+'\n');
 const warm=await parse();assert.equal(total(warm),110);
 const old=process.env.VIBE_USAGE_CODEX_CACHE;process.env.VIBE_USAGE_CODEX_CACHE='0';
 try{assert.deepEqual((await parse()).buckets,warm.buckets)}finally{if(old===undefined)delete process.env.VIBE_USAGE_CODEX_CACHE;else process.env.VIBE_USAGE_CODEX_CACHE=old}
}));
test('cumulative-only legacy fallback uses its own last mirror baseline',()=>fixture({'a.jsonl':[meta(),context,record(),legacy(2,u(90)),legacy(3,u(190,40,20,4),undefined)].map((v,i)=>{if(i===4)delete v.payload.info.last_token_usage;return v})},async()=>assert.equal(total(await parse()),220)));
test('same-session segments preserve new records and model context',()=>fixture({'a.jsonl':[meta(),context,record()],'b.jsonl':[meta(),context,record(),legacy(2,u(90)),record(3,u(200,40,20,4))]},async()=>{const r=await parse();assert.equal(total(r),220);assert.equal(r.buckets[0].model,'test-model')}));
test('fork replay of new and legacy records is excluded',()=>fixture({'a.jsonl':[meta('parent'),context,record(),legacy()],'b.jsonl':[meta('child',3,{forked_from_id:'parent'}),context,record(),legacy(),record(4,u(200,40,20,4)),legacy(5,u(200,40,20,4))]},async()=>assert.equal(total(await parse()),220)));
test('invalid durable records do not hide valid legacy usage',()=>fixture({'a.jsonl':[meta(),context,record(1,u(),u(-1)),legacy()]},async()=>assert.equal(total(await parse()),110)));

test('equal-sized legacy call in a new turn remains a distinct request',()=>fixture({'a.jsonl':[meta(),context,record(),{...context,timestamp:at(2)},legacy(3,u(200,40,20,4))]},async()=>assert.equal(total(await parse()),220)));
test('repeated durable records are counted once despite lagging UI counters',()=>fixture({'a.jsonl':[meta(),context,record(),legacy(2,u(90)),record(3),legacy(4,u(90))]},async()=>assert.equal(total(await parse()),110)));

test('malformed record cannot turn the next legitimate legacy request into a mirror',()=>fixture({'a.jsonl':[meta(),context,record(),record(2,u(),u(-1)),legacy(3,u(200,40,20,4))]},async()=>assert.equal(total(await parse()),220)));
test('durable usage cache retains numeric accounting fields only',()=>fixture({'a.jsonl':[meta(),context,record(1,u(),{...u(),private_text:'SENSITIVE_TEST_CONTENT'})]},async dir=>{
  await parse();
  const cache=path.join(path.dirname(dir),'cache');
  for(const name of fs.readdirSync(cache,{recursive:true})) {
    const p=path.join(cache,name);
    if(fs.statSync(p).isFile())assert.ok(!fs.readFileSync(p,'utf8').includes('SENSITIVE_TEST_CONTENT'));
  }
}));
