/** Compile the pinned AECraftingPattern.getItemOrFluidInput against test-only stubs. */
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
const reference = process.env.AE2_REFERENCE;
assert(reference, 'Set AE2_REFERENCE to the pinned AE2 source checkout');
const version = Bun.spawnSync(['git', '-C', reference, 'rev-parse', 'HEAD']);
assert.equal(version.exitCode, 0, 'Could not read AE2 source revision');
assert.equal(version.stdout.toString().trim(), 'db17504a86128fdf3dae31f5fb7a112a646e0b93', 'Unexpected AE2 source revision');
const source = readFileSync(join(reference, 'src/main/java/appeng/crafting/pattern/AECraftingPattern.java'), 'utf8');
const start = source.indexOf('    private GenericStack getItemOrFluidInput(');
assert(start >= 0, 'The reference fluid-template method was not found');
const open = source.indexOf('{', start); let depth = 1, end = open + 1;
while (depth && end < source.length) { if (source[end] === '{') depth++; if (source[end] === '}') depth--; end++; }
assert.equal(depth, 0, 'Could not extract the complete reference method');
const method = source.slice(start, end);
const template = readFileSync(join(import.meta.dir, 'FluidTemplateOracle.java'), 'utf8');
assert(template.includes('// ACTUAL_AE2_METHOD'), 'Missing source insertion marker');
const dir = mkdtempSync(join(tmpdir(), 'ae2-fluid-template-oracle-'));
const java = join(dir, 'FluidTemplateOracle.java');
writeFileSync(java, template.replace('// ACTUAL_AE2_METHOD', method));
for (const command of [['javac', '-d', dir, java], ['java', '-cp', dir, 'FluidTemplateOracle']]) {
  const result = Bun.spawnSync(command, { stdout: 'pipe', stderr: 'pipe' });
  process.stdout.write(result.stdout); process.stderr.write(result.stderr);
  assert.equal(result.exitCode, 0, `${command[0]} failed`);
}
