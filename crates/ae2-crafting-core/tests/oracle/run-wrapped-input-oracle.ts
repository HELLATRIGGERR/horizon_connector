/** Execute the pinned AE2 crafting-wrapper conversion methods against small stubs. */
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
const reference = process.env.AE2_REFERENCE;
assert(reference, 'Set AE2_REFERENCE to the pinned AE2 source checkout');
const version = Bun.spawnSync(['git', '-C', reference, 'rev-parse', 'HEAD']);
assert.equal(version.exitCode, 0, 'Could not read AE2 source revision');
assert.equal(version.stdout.toString().trim(), 'db17504a86128fdf3dae31f5fb7a112a646e0b93', 'Unexpected AE2 source revision');
const extract = (path: string, signature: string) => {
  const source = readFileSync(join(reference, path), 'utf8');
  const start = source.indexOf(signature);
  assert(start >= 0, `Missing source method ${signature}`);
  const open = source.indexOf('{', start); let depth = 1, end = open + 1;
  while (depth && end < source.length) { if (source[end] === '{') depth++; if (source[end] === '}') depth--; end++; }
  assert.equal(depth, 0, `Could not extract ${signature}`);
  return source.slice(start, end);
};
const root = 'src/main/java/appeng/';
let template = readFileSync(join(import.meta.dir, 'SourceWrappedOracle.java'), 'utf8');
for (const [marker, path, signature] of [
  ['// ACTUAL_UNWRAP_WHAT', `${root}items/misc/WrappedGenericStack.java`, '    public AEKey unwrapWhat('],
  ['// ACTUAL_UNWRAP_AMOUNT', `${root}items/misc/WrappedGenericStack.java`, '    public long unwrapAmount('],
  ['// ACTUAL_FROM_ITEM', `${root}api/stacks/GenericStack.java`, '    public static GenericStack fromItemStack('],
  ['// ACTUAL_UNWRAP_ITEM', `${root}api/stacks/GenericStack.java`, '    public static GenericStack unwrapItemStack('],
  ['// ACTUAL_CRAFTING_INPUTS', `${root}crafting/pattern/AECraftingPattern.java`, '    public static List<GenericStack> getCraftingInputs('],
  ['// ACTUAL_CONDENSE', `${root}crafting/pattern/AEPatternHelper.java`, '    public static List<GenericStack> condenseStacks('],
  ['// ACTUAL_INPUT_CONSTRUCTOR', `${root}crafting/pattern/AECraftingPattern.java`, '        private Input('],
] as const) {
  assert(template.includes(marker), `Missing insertion marker ${marker}`);
  template = template.replace(marker, extract(path, signature));
}
const dir = mkdtempSync(join(tmpdir(), 'ae2-wrapped-input-oracle-'));
const java = join(dir, 'SourceWrappedOracle.java');
writeFileSync(java, template);
for (const command of [['javac', '-d', dir, java], ['java', '-cp', dir, 'SourceWrappedOracle']]) {
  const result = Bun.spawnSync(command, { stdout: 'pipe', stderr: 'pipe' });
  process.stdout.write(result.stdout); process.stderr.write(result.stderr);
  assert.equal(result.exitCode, 0, `${command[0]} failed`);
}
