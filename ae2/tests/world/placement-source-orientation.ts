/** Normal player placement follows AE2's horizontal/full orientation strategies. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('placement-source-orientation', mode);
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 6, y = Math.ceil(py) + 4, z = Math.floor(pz) + 6;
  const at = (dx = 0, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  await fixture.command(`fill ${at(-4, -1, -4)} ${at(8, -1, 4)} minecraft:stone`);
  await fixture.command(`fill ${at(-4, 0, -4)} ${at(8, 4, 4)} minecraft:air`);
  await fixture.command(`setblock ${at()} minecraft:stone`);
  await fixture.command(`setblock ${at(4)} minecraft:stone`);
  await fixture.command('clear @s');
  await fixture.command('gamemode survival');
  const state = async (dx: number) => String((await fixture.raw(`getblock ${at(dx, 1)}`)).block);

  // Looking south yields north-facing Sky Chest in source.
  await fixture.command(`tp @s ${x + .5} ${y + 1} ${z - 2.5}`);
  await fixture.replace('hotbar.0', 'ae2:sky_stone_chest');
  await fixture.select(0, 'ae2:sky_stone_chest');
  await fixture.raw(`interact ${at()} up`);
  const chest = await until(() => state(0), s => s.startsWith('ae2:sky_stone_chest['), 'placed physical Sky Chest');
  assert.match(chest, /facing=north/, 'horizontal strategy points opposite the player view');

  // Looking west yields east-facing Crafting Monitor; its default north state
  // must not mask a missing full orientation strategy.
  await fixture.command(`tp @s ${x + 6.5} ${y + 1} ${z + .5}`);
  await fixture.replace('hotbar.0', 'ae2:crafting_monitor');
  await fixture.select(0, 'ae2:crafting_monitor');
  await fixture.raw(`interact ${at(4)} up`);
  const monitor = await until(() => state(4), s => s.startsWith('ae2:crafting_monitor['), 'placed physical Crafting Monitor');
  assert.match(monitor, /facing=east/);
  assert.match(monitor, /spin=0/);
  fixture.mark('real player placement applies opposite-view chest and full monitor orientation');

  await fixture.restart();
  assert.match(await state(0), /facing=north/);
  assert.match(await state(4), /facing=east/);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('both oriented block states survive restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
