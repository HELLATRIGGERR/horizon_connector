import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
const reference = process.env.AE2_REFERENCE;
assert(reference);
const version = Bun.spawnSync(['git', '-C', reference, 'rev-parse', 'HEAD']);
assert.equal(version.exitCode, 0, 'Could not read AE2 source revision');
assert.equal(version.stdout.toString().trim(), 'db17504a86128fdf3dae31f5fb7a112a646e0b93');
const extract = (path: string, signature: string) => {
  const source = readFileSync(join(reference, path), 'utf8');
  const start = source.indexOf(signature); assert(start >= 0, signature);
  const open = source.indexOf('{', start); let depth = 1, end = open + 1;
  while (depth && end < source.length) {if (source[end] === '{') depth++; if (source[end] === '}') depth--; end++;}
  assert.equal(depth, 0); return source.slice(start, end);
};
let template = readFileSync(join(import.meta.dir,'SourceAssemblerOracle.java'), 'utf8');
for (const [marker,path,signature] of [
 ['// ACTUAL_GENERIC_CAPABILITY','src/main/java/appeng/api/crafting/IPatternDetails.java','    default boolean supportsPushInputsToExternalInventory('],
 ['// ACTUAL_ASSEMBLER_CAPABILITY','src/main/java/appeng/blockentity/crafting/IMolecularAssemblerSupportedPattern.java','    default boolean supportsPushInputsToExternalInventory('],
 ['// ACTUAL_FILL_CRAFTING_GRID','src/main/java/appeng/crafting/pattern/AECraftingPattern.java','    public void fillCraftingGrid('],
 ['// ACTUAL_FILL_GRID','src/main/java/appeng/blockentity/crafting/MolecularAssemblerBlockEntity.java','    private void fillGrid('],
 ['// ACTUAL_PUSH_PATTERN','src/main/java/appeng/blockentity/crafting/MolecularAssemblerBlockEntity.java','    public boolean pushPattern('],
] as const) {assert(template.includes(marker)); template=template.replace(marker,extract(path,signature));}
const dir=mkdtempSync(join(tmpdir(),'ae2-assembler-source-oracle-')); const java=join(dir,'SourceAssemblerOracle.java');writeFileSync(java,template);
for(const command of [['javac','-d',dir,java],['java','-cp',dir,'SourceAssemblerOracle']]) {const result=Bun.spawnSync(command,{stdout:'pipe',stderr:'pipe'});process.stdout.write(result.stdout);process.stderr.write(result.stderr);assert.equal(result.exitCode,0);}
