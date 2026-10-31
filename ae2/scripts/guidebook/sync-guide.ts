/** Project verified Guide data into a content root without runtime JS. */
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {cpSync,existsSync,mkdirSync,readFileSync,readdirSync} from 'node:fs';
import {dirname,join,relative,resolve} from 'node:path';

const stage=import.meta.dir;
const repoRoot=resolve(stage,'../../..');
const stagedAssets=resolve(process.env.AE2_GUIDE_ROOT ?? join(repoRoot,'.local/guidebook/ae2guide'));
const validationPath=resolve(process.env.AE2_GUIDE_VALIDATION ?? join(repoRoot,'.local/guidebook/validation.json'));
function files(dir:string):string[] {
  return readdirSync(dir,{withFileTypes:true}).sort((a,b)=>a.name<b.name?-1:a.name>b.name?1:0).flatMap(entry=>{
    const p=join(dir,entry.name);return entry.isDirectory()?files(p):entry.isFile()?[p]:[];
  });
}
function digest(bytes:Uint8Array):string{return createHash('sha256').update(bytes).digest('hex');}

/** Validate all inputs before copying anything. Caller chooses the content root. */
export function syncGuideAssets(reference:string,content:string) {
  const source=join(reference,'guidebook');
  const report=JSON.parse(readFileSync(validationPath,'utf8'));
  const compiled=join(stagedAssets,'compiled-guide.json');
  assert.equal(digest(readFileSync(compiled)),report.generated_sha256,'stale compiled Guide data');
  assert.equal(digest(readFileSync(join(stage,'compile-guide.ts'))),report.compiler_sha256,'Guide compiler changed after verification');
  const inputs=files(source);assert.equal(inputs.length,report.source_files,'Guide source count changed');
  const sourceHash=createHash('sha256');
  for(const path of inputs){
    const rel=relative(source,path);const bytes=readFileSync(path);const staged=join(stagedAssets,rel);
    assert(existsSync(staged),`missing staged Guide asset: ${rel}`);
    assert.deepEqual(readFileSync(staged),bytes,`Guide source changed: ${rel}`);
    sourceHash.update(rel.split('\\').join('/')+'\0');sourceHash.update(createHash('sha256').update(bytes).digest());
  }
  assert.equal(sourceHash.digest('hex'),report.source_path_content_sha256,'Guide source fingerprint changed');
  const destination=join(content,'assets/ae2/ae2guide');
  for(const path of inputs){
    const out=join(destination,relative(source,path));mkdirSync(dirname(out),{recursive:true});cpSync(path,out);
  }
  mkdirSync(destination,{recursive:true});cpSync(compiled,join(destination,'compiled-guide.json'));
  return {source_files:inputs.length,compiled_sha256:report.generated_sha256,source_sha256:report.source_path_content_sha256};
}

if(import.meta.main){
  const destination=process.argv[2];if(!destination)throw Error('usage: bun run sync-guide.ts <content-root>');
  console.log(JSON.stringify(syncGuideAssets(resolve(process.env.AE2_REFERENCE_DIR ?? join(repoRoot,'ae2-reference')),resolve(destination))));
}
