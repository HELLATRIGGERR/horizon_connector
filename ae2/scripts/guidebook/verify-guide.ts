/** Check the pinned source copy, compiled component order and reproducibility. */
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {readFileSync,readdirSync,writeFileSync} from 'node:fs';
import {join,relative,resolve} from 'node:path';

const stage=import.meta.dir;
const repoRoot=resolve(stage,'../../..');
const original=process.env.AE2_GUIDE_SOURCE ?? join(repoRoot,'ae2-reference/guidebook');
const assets=resolve(process.env.AE2_GUIDE_ROOT ?? join(repoRoot,'.local/guidebook/ae2guide'));
const output=join(assets,'compiled-guide.json');
const samplePath=resolve(process.env.AE2_GUIDE_SAMPLE ?? join(repoRoot,'.local/guidebook/compiled-sample.json'));
const validationPath=resolve(process.env.AE2_GUIDE_VALIDATION ?? join(repoRoot,'.local/guidebook/validation.json'));
const sha=(data:Uint8Array)=>createHash('sha256').update(data).digest('hex');
const shaFile=(p:string)=>sha(readFileSync(p));
function files(dir:string):string[] {
  return readdirSync(dir,{withFileTypes:true}).sort((a,b)=>a.name<b.name?-1:a.name>b.name?1:0).flatMap(entry=>{
    const p=join(dir,entry.name);return entry.isDirectory()?files(p):entry.isFile()?[p]:[];
  });
}
function walk(value:unknown,visit:(o:Record<string,unknown>)=>void):void {
  if(Array.isArray(value)){for(const item of value)walk(item,visit);return}
  if(!value||typeof value!=='object')return;
  const obj=value as Record<string,unknown>;visit(obj);
  for(const child of Object.values(obj))walk(child,visit);
}
const before=shaFile(output);
execFileSync('bun',['run','compile-guide.ts'],{cwd:stage,stdio:'ignore',env:process.env});
assert.equal(shaFile(output),before,'the same sources produced a different document');
const sourceFiles=files(original);assert.equal(sourceFiles.length,288);
const sourcePaths=new Set<string>();const sourceDigest=createHash('sha256');
for(const source of sourceFiles){
  const path=relative(original,source).split('\\').join('/');sourcePaths.add(path);
  const bytes=readFileSync(source);assert.deepEqual(readFileSync(join(assets,path)),bytes,path);
  sourceDigest.update(path+'\0');sourceDigest.update(createHash('sha256').update(bytes).digest());
}
const extras=files(assets).map(p=>relative(assets,p).split('\\').join('/')).filter(p=>!sourcePaths.has(p));
assert.deepEqual(extras,['compiled-guide.json']);
const doc=JSON.parse(readFileSync(output,'utf8'));
assert.equal(Object.keys(doc.pages).length,125);assert.deepEqual(doc.corpus.diagnostics,[]);
const components=doc.corpus.components as Record<string,number>;
const allNodes:Record<string,unknown>[]=[];const scenes:Record<string,unknown>[]=[];
for(const [pageId,pageValue] of Object.entries(doc.pages)){
  const page=pageValue as {nodes:unknown[]};const found:string[]=[];
  walk(page.nodes,node=>{
    if(typeof node.type==='string')allNodes.push(node);
    if(node.type==='element'&&typeof node.name==='string'){
      found.push(node.name);if(node.name==='GameScene')scenes.push(node);
    }
  });
  const source=readFileSync(join(original,pageId),'utf8');
  const expected=[...source.matchAll(/<(?!\/|!)([\w:.-]+)\b/g)].map(m=>m[1]).filter(name=>name in components);
  assert.deepEqual(found,expected,`${pageId}: component order differs`);
}
assert.equal(scenes.length,139);
assert.equal(new Set(scenes.map(s=>s.scene_id)).size,139);
for(const scene of scenes){
  assert(Array.isArray(scene.children));
  assert.equal(scene.children.filter((x:unknown)=>x&&typeof x==='object'&&(x as Record<string,unknown>).name==='ImportStructure').length,1,String(scene.scene_id));
}
const sample=JSON.parse(readFileSync(samplePath,'utf8'));
assert.equal(shaFile(samplePath),'6abc978017c3509f41dcefdc136b49273540db0128dab0d93055590445961826','Guide sample does not match the pinned corpus');
for(const key of ['link','image','recipe','config','index_scene','nested_annotated_scene'])
  assert(allNodes.some(node=>JSON.stringify(node)===JSON.stringify(sample[key])),`stale example: ${key}`);
assert.deepEqual(sample.navigation_example,doc.pages[sample.page_id].navigation);
const report={source:'ae2-reference/guidebook',source_files:sourceFiles.length,
  source_path_content_sha256:sourceDigest.digest('hex'),staged_source_byte_identical:true,
  generated_file:'.local/guidebook/ae2guide/compiled-guide.json',generated_sha256:before,
  compiler_sha256:shaFile(join(stage,'compile-guide.ts')),dependencies:{marked:'17.0.5',yaml:'2.8.3'},
  pages:Object.keys(doc.pages).length,components,scene_ids_unique:scenes.length,
  all_scenes_have_one_direct_import:true,every_page_markup_order_matches_source:true,
  compiled_sample_verified:true,diagnostics:doc.corpus.diagnostics,deterministic_rebuild:true};
writeFileSync(validationPath,JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify({source_files:sourceFiles.length,pages:report.pages,scenes:scenes.length,sha256:before}));
