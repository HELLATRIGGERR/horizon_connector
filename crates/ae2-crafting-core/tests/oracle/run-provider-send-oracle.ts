/** Execute pinned AE2 processing order and provider retry methods. */
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
const reference=process.env.AE2_REFERENCE;
assert(reference, 'Set AE2_REFERENCE to the pinned AE2 source checkout');
const version=Bun.spawnSync(['git','-C',reference,'rev-parse','HEAD']);
assert.equal(version.exitCode,0);assert.equal(version.stdout.toString().trim(),'db17504a86128fdf3dae31f5fb7a112a646e0b93');
const extract=(path:string,signature:string)=>{
  const source=readFileSync(join(reference,path),'utf8');const start=source.indexOf(signature);
  assert(start>=0,`Missing source method ${signature}`);const open=source.indexOf('{',start);let depth=1,end=open+1;
  while(depth&&end<source.length){if(source[end]==='{')depth++;if(source[end]==='}')depth--;end++;}
  assert.equal(depth,0);return source.slice(start,end);
};
const root='src/main/java/appeng/';
let template=readFileSync(join(import.meta.dir,'SourceProviderOracle.java'),'utf8');
for(const [marker,path,signature] of [
  ['// ACTUAL_DEFAULT_PUSH',`${root}api/crafting/IPatternDetails.java`,'    default void pushInputsToExternalInventory('],
  ['// ACTUAL_PROCESSING_PUSH',`${root}crafting/pattern/AEProcessingPattern.java`,'    public void pushInputsToExternalInventory('],
  ['// ACTUAL_ADD_SEND',`${root}helpers/patternprovider/PatternProviderLogic.java`,'    private void addToSendList('],
  ['// ACTUAL_SEND_STACKS',`${root}helpers/patternprovider/PatternProviderLogic.java`,'    private boolean sendStacksOut('],
  ['// ACTUAL_CONDENSER_INSERT',`${root}blockentity/misc/CondenserMEStorage.java`,'    public long insert('],
] as const){assert(template.includes(marker));template=template.replace(marker,extract(path,signature));}
// Only the test harness may call these methods. Their bodies are unchanged.
template=template.replace('private void addToSendList(', 'void addToSendList(').replace('private boolean sendStacksOut(', 'boolean sendStacksOut(');
const dir=mkdtempSync(join(tmpdir(),'ae2-provider-send-oracle-'));const java=join(dir,'SourceProviderOracle.java');writeFileSync(java,template);
for(const command of [['javac','-d',dir,java],['java','-cp',dir,'SourceProviderOracle']]){
  const result=Bun.spawnSync(command,{stdout:'pipe',stderr:'pipe'});process.stdout.write(result.stdout);process.stderr.write(result.stderr);assert.equal(result.exitCode,0,`${command[0]} failed`);
}
