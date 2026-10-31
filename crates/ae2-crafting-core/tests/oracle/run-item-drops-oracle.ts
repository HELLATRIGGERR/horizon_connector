import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
const reference = process.env.AE2_REFERENCE; assert(reference);
const version = Bun.spawnSync(['git', '-C', reference, 'rev-parse', 'HEAD']);
assert.equal(version.exitCode, 0, 'Could not read AE2 source revision');
assert.equal(version.stdout.toString().trim(), 'db17504a86128fdf3dae31f5fb7a112a646e0b93');
const source = readFileSync(join(reference,'src/main/java/appeng/api/stacks/AEItemKey.java'),'utf8');
const start = source.indexOf('    public void addDrops('); assert(start >= 0);
const open = source.indexOf('{', start); let depth = 1, end = open + 1;
while (depth && end < source.length) {if (source[end] === '{') depth++; if (source[end] === '}') depth--; end++;}
assert.equal(depth, 0);
let template=readFileSync(join(import.meta.dir,'SourceItemDropsOracle.java'),'utf8');
assert(template.includes('// ACTUAL_ADD_DROPS'));template=template.replace('// ACTUAL_ADD_DROPS',source.slice(start,end));
const dir=mkdtempSync(join(tmpdir(),'ae2-item-drops-oracle-')),java=join(dir,'SourceItemDropsOracle.java');writeFileSync(java,template);
for(const command of [['javac','-d',dir,java],['java','-cp',dir,'SourceItemDropsOracle']]) {const result=Bun.spawnSync(command,{stdout:'pipe',stderr:'pipe'});process.stdout.write(result.stdout);process.stderr.write(result.stderr);assert.equal(result.exitCode,0);}
