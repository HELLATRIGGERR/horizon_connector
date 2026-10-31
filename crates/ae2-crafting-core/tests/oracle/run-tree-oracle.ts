/** Compile the pinned, actual AE2 tree classes against test-only stubs. */
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
const reference = process.env.AE2_REFERENCE;
assert(reference, 'Set AE2_REFERENCE to the pinned AE2 source checkout');
const version = Bun.spawnSync(['git', '-C', reference, 'rev-parse', 'HEAD']);
assert.equal(version.exitCode, 0, 'Could not read AE2 source revision');
assert.equal(version.stdout.toString().trim(), 'db17504a86128fdf3dae31f5fb7a112a646e0b93', 'Unexpected AE2 source revision');
const dir = mkdtempSync(join(tmpdir(), 'ae2-crafting-tree-oracle-'));
for (const name of ['CraftingTreeNode', 'CraftingTreeProcess']) {
  const source = readFileSync(join(reference, 'src/main/java/appeng/crafting', `${name}.java`), 'utf8')
    .replace(/^package .*;\s*$/gm, '')
    .replace(/^import .*;\s*$/gm, '')
    .replaceAll('@Nullable', '');
  writeFileSync(join(dir, `${name}.java`), 'import java.util.*;\nimport java.util.stream.Collectors;\n' + source);
}
const stubs = resolve(import.meta.dir, 'CraftingTreeOracle.java');
for (const command of [['javac', '-d', dir, stubs, join(dir, 'CraftingTreeNode.java'), join(dir, 'CraftingTreeProcess.java')], ['java', '-cp', dir, 'CraftingTreeOracle']]) {
  const result = Bun.spawnSync(command, { stdout: 'pipe', stderr: 'pipe' });
  process.stdout.write(result.stdout); process.stderr.write(result.stderr);
  assert.equal(result.exitCode, 0, `${command[0]} failed`);
}
