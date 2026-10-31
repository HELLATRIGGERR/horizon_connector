import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const reference=process.env.AE2_REFERENCE;assert(reference,'Set AE2_REFERENCE to the pinned AE2 source checkout');
const revision=Bun.spawnSync(['git','-C',reference,'rev-parse','HEAD']);
assert.equal(revision.exitCode,0);assert.equal(revision.stdout.toString().trim(),'db17504a86128fdf3dae31f5fb7a112a646e0b93');
const source=readFileSync(join(reference,'src/main/java/appeng/crafting/pattern/AECraftingPattern.java'),'utf8');
const method=(signature:string)=>{
  const start=source.indexOf(signature);assert(start>=0,signature);
  const open=source.indexOf('{',start);let depth=1,end=open+1;
  while(depth&&end<source.length){if(source[end]==='{')depth++;if(source[end]==='}')depth--;end++;}
  assert.equal(depth,0);return source.slice(start,end);
};
let template=readFileSync(join(import.meta.dir,'SourcePatternAssemblyOracle.java'),'utf8');
for(const [marker,signature] of [
  ['// ACTUAL_IS_ITEM_VALID','    public boolean isItemValid('],
  ['// ACTUAL_ASSEMBLE','    public ItemStack assemble('],
] as const){assert(template.includes(marker));template=template.replace(marker,method(signature));}
const dir=mkdtempSync(join(tmpdir(),'ae2-pattern-assembly-oracle-'));
const java=join(dir,'SourcePatternAssemblyOracle.java');writeFileSync(java,template);
for(const command of [['javac','-d',dir,java],['java','-cp',dir,'SourcePatternAssemblyOracle']]){
  const result=Bun.spawnSync(command,{stdout:'pipe',stderr:'pipe'});process.stdout.write(result.stdout);process.stderr.write(result.stderr);assert.equal(result.exitCode,0);
}
