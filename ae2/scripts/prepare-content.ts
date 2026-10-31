/** Prepare a local AE2 content pack from pinned upstream sources and owned game resources. */
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {copyFileSync,cpSync,existsSync,mkdirSync,readFileSync,readdirSync,renameSync,writeFileSync} from 'node:fs';
import {dirname,join,resolve} from 'node:path';
import {AE2_COMMIT,AE2_SOURCE,requirePinnedReference} from './reference.ts';

const root=resolve(import.meta.dir,'../..');
const local=join(root,'.local');
const reference=resolve(process.env.AE2_REFERENCE_DIR??join(root,'ae2-reference'));
const vanilla=resolve(process.env.AE2_VANILLA_RESOURCES??join(local,'resources/26.3'));
const destination=resolve(process.env.AE2_CONTENT_DIR??join(root,'content/ae2'));
const neo=resolve(process.env.AE2_NEOFORGE_JAR??join(local,'downloads/neoforge-21.1.259-universal.jar'));
const neoSha='50abce8f44a4f1a54e77eb5fc8db8512233f9d0725317662536a4280f4885dbb';
const neoUrl='https://maven.neoforged.net/releases/net/neoforged/neoforge/21.1.259/neoforge-21.1.259-universal.jar';
const sha=(bytes:Uint8Array)=>createHash('sha256').update(bytes).digest('hex');
if(process.argv.includes('--help')||process.argv.includes('-h')){
  console.log(`Usage: bun run ae2/scripts/prepare-content.ts\n\nRequires Bun, Git, unzip, your prepared Minecraft 26.3 resources and network access for pinned source/dependencies. Generated content and downloads remain local.\n\nOptional environment: AE2_REFERENCE_DIR, AE2_VANILLA_RESOURCES, AE2_NEOFORGE_JAR, AE2_CONTENT_DIR; set both AE2_MARKED_MODULE and AE2_YAML_MODULE to use installed Guide compiler dependencies.`);
  process.exit(0);
}
function run(command:string,args:string[],cwd:string,env:NodeJS.ProcessEnv=process.env):void {
  const child=spawnSync(command,args,{cwd,env,stdio:'inherit'});
  if(child.error)throw child.error;
  if(child.status!==0)throw new Error(`${command} failed (${child.status??child.signal})`);
}
if(!existsSync(join(vanilla,'assets/minecraft/models')))throw new Error('Prepare your own Minecraft 26.3 resources first (bun run ae2/scripts/prepare-resources.ts), or set AE2_VANILLA_RESOURCES.');
if(existsSync(destination)&&readdirSync(destination).length)throw new Error(`Content destination is not empty: ${destination}. Move or back it up before generating again.`);
if(!existsSync(reference)){
  if(process.env.AE2_REFERENCE_DIR)throw new Error('AE2_REFERENCE_DIR does not exist.');
  run('git',['clone','--no-checkout','--filter=blob:none',AE2_SOURCE,reference],root);
  run('git',['-C',reference,'checkout','--detach',AE2_COMMIT],root);
}
requirePinnedReference(reference);
if(!existsSync(neo)){
  if(process.env.AE2_NEOFORGE_JAR)throw new Error('AE2_NEOFORGE_JAR does not exist.');
  console.log('Downloading pinned NeoForge 21.1.259 from its official Maven repository.');
  const response=await fetch(neoUrl);
  if(!response.ok)throw new Error(`NeoForge download failed: HTTP ${response.status}`);
  const bytes=new Uint8Array(await response.arrayBuffer());
  if(sha(bytes)!==neoSha)throw new Error('NeoForge download SHA-256 does not match the pinned JAR.');
  mkdirSync(dirname(neo),{recursive:true});
  const temporary=neo+`.tmp-${process.pid}`;writeFileSync(temporary,bytes,{flag:'wx'});renameSync(temporary,neo);
}
if(sha(readFileSync(neo))!==neoSha)throw new Error('NeoForge JAR SHA-256 does not match the pinned 21.1.259 universal JAR.');
let marked=process.env.AE2_MARKED_MODULE,yaml=process.env.AE2_YAML_MODULE;
if(Boolean(marked)!==Boolean(yaml))throw new Error('Set both AE2_MARKED_MODULE and AE2_YAML_MODULE, or neither.');
if(!marked||!yaml){
  const deps=join(local,'guide-deps');mkdirSync(deps,{recursive:true});
  for(const name of ['package.json','bun.lock'])copyFileSync(join(import.meta.dir,'guidebook',name),join(deps,name));
  run('bun',['install','--frozen-lockfile','--ignore-scripts'],deps,{...process.env,BUN_INSTALL_CACHE_DIR:join(local,'bun-cache')});
  marked=join(deps,'node_modules/marked/lib/marked.esm.js');yaml=join(deps,'node_modules/yaml/dist/index.js');
}
// A fresh private staging directory avoids altering previous generated material.
const stage=join(local,'guidebook',`${Date.now()}-${process.pid}`), guide=join(stage,'ae2guide');
mkdirSync(stage,{recursive:true});cpSync(join(reference,'guidebook'),guide,{recursive:true});
const env={...process.env,AE2_REFERENCE_DIR:reference,AE2_VANILLA_RESOURCES:vanilla,AE2_CONTENT_DIR:destination,
  AE2_NEOFORGE_JAR:neo,AE2_GUIDE_SOURCE:join(reference,'guidebook'),AE2_GUIDE_ROOT:guide,
  AE2_GUIDE_SAMPLE:join(stage,'compiled-sample.json'),AE2_GUIDE_VALIDATION:join(stage,'validation.json'),
  AE2_MARKED_MODULE:marked,AE2_YAML_MODULE:yaml};
for(const script of ['compile-guide.ts','build-sample.ts','verify-guide.ts'])run('bun',['run',join(import.meta.dir,'guidebook',script)],root,env);
mkdirSync(destination,{recursive:true});
run('bun',['run',join(import.meta.dir,'sync-ae2-content.ts')],root,env);
console.log(`AE2 content prepared in ${destination}. Generated upstream assets and downloads stay local.`);
