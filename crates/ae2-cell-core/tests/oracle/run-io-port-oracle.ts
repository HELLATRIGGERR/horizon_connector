import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
const reference=process.env.AE2_REFERENCE;assert(reference,'Set AE2_REFERENCE to the pinned AE2 source checkout');
const revision=Bun.spawnSync(['git','-C',reference,'rev-parse','HEAD']);
assert.equal(revision.exitCode,0);assert.equal(revision.stdout.toString().trim(),'db17504a86128fdf3dae31f5fb7a112a646e0b93');
const source=readFileSync(join(reference,'src/main/java/appeng/blockentity/storage/IOPortBlockEntity.java'),'utf8');
const signature='    private long transferContents(';const start=source.indexOf(signature);assert(start>=0);
const open=source.indexOf('{',start);let depth=1,end=open+1;
while(depth&&end<source.length){if(source[end]==='{')depth++;if(source[end]==='}')depth--;end++;}
assert.equal(depth,0);
let template=readFileSync(join(import.meta.dir,'SourceIoPortOracle.java'),'utf8');
assert(template.includes('// ACTUAL_TRANSFER_CONTENTS'));template=template.replace('// ACTUAL_TRANSFER_CONTENTS',source.slice(start,end));
const dir=mkdtempSync(join(tmpdir(),'ae2-io-port-oracle-'));
const java=join(dir,'SourceIoPortOracle.java');writeFileSync(java,template);
for(const command of [['javac','-d',dir,java],['java','-cp',dir,'SourceIoPortOracle']]) {
    const result=Bun.spawnSync(command,{stdout:'pipe',stderr:'pipe'});process.stdout.write(result.stdout);process.stderr.write(result.stderr);assert.equal(result.exitCode,0);
}
