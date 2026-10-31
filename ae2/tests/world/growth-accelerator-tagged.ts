/** A powered accelerator directly ticks tagged blocks, once per interval. */
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('growth-accelerator-tagged', mode);
try {
  const growthTag = JSON.parse(readFileSync(join(fixture.content, 'data/ae2/tags/block/growth_acceleratable.json'), 'utf8'));
  const buddingTag = JSON.parse(readFileSync(join(fixture.content, 'data/c/tags/block/budding_blocks.json'), 'utf8'));
  assert(growthTag.values.includes('#c:budding_blocks'));
  for (const id of ['minecraft:budding_amethyst', 'ae2:flawless_budding_quartz', 'ae2:flawed_budding_quartz', 'ae2:chipped_budding_quartz', 'ae2:damaged_budding_quartz']) {
    assert(buddingTag.values.includes(id), `prepared growth tag is missing ${id}`);
  }
  fixture.mark('prepared conventional budding tag includes both vanilla and AE2 blocks');
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 6, y = Math.ceil(py) + 3, z = Math.floor(pz) + 6;
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const accelerator = at(0), power = at(1), cane = at(0, 0, -1), upper = at(0, 1, -1), ore = at(0, 0, 1);
  const block = async (pos: string) => {
    const value = await fixture.raw(`getblock ${pos}`);
    return { block: value.block.split('[')[0], state: value.block };
  };
  await fixture.command(`fill ${at(-3, -1, -3)} ${at(3, -1, 3)} minecraft:stone`);
  await fixture.command(`fill ${at(-3, 0, -3)} ${at(3, 4, 3)} minecraft:air`);
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z + 2.5}`);
  await until(() => fixture.raw('status'), s => Math.abs(s.player.position[0] - (x + 2.5)) < 0.2, 'fixture teleport');
  await fixture.command('gamerule randomTickSpeed 0');
  await fixture.command(`setblock ${at(0, -2, -1)} minecraft:stone`);
  await fixture.command(`setblock ${at(0, -1, -1)} minecraft:sand`);
  await fixture.command(`setblock ${at(0, -1, -2)} minecraft:water`);
  await fixture.command(`setblock ${ore} minecraft:redstone_ore[lit=true]`);
  await fixture.command(`setblock ${accelerator} ae2:growth_accelerator[facing=east,powered=false]`);
  await fixture.command(`setblock ${power} ae2:creative_energy_cell`);
  await until(() => fixture.ae(`grid status ${accelerator}`), s => s.powered, 'accelerator powered', 30000);
  await fixture.hold();
  await fixture.command(`setblock ${cane} minecraft:sugar_cane[age=15]`);
  assert.equal((await block(upper)).block, 'minecraft:air');
  assert.match((await block(ore)).state ?? '', /lit=true/);
  let grown = false;
  for (let i = 0; i < 100; i++) {
    await fixture.stepServer(1);
    if ((await block(upper)).block === 'minecraft:sugar_cane') { grown = true; break; }
  }
  assert(grown, 'age-15 sugar cane grows on the first accelerator random tick');
  assert.match((await block(cane)).state ?? '', /age=0/);
  assert.match((await block(ore)).state ?? '', /lit=true/, 'untagged ore is not random-ticked');
  fixture.mark('a tagged sugar cane grows deterministically while an untagged neighbor is untouched');

  await fixture.hold(false); await fixture.restart();
  await until(() => block(upper), s => s.block === 'minecraft:sugar_cane', 'grown cane survives restart', 30000);
  await fixture.command(`setblock ${power} minecraft:air`);
  await until(() => fixture.ae(`grid status ${accelerator}`), s => !s.powered, 'accelerator loses network power', 30000);
  await fixture.hold();
  await fixture.command(`setblock ${upper} minecraft:air`);
  await fixture.command(`setblock ${cane} minecraft:sugar_cane[age=15]`);
  await fixture.stepServer(40);
  assert.equal((await block(upper)).block, 'minecraft:air', 'unpowered accelerator does not grow cane');
  assert.match((await block(cane)).state ?? '', /age=15/);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('after restart an unpowered accelerator leaves the same tagged cane unchanged');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
