/** Failed/simulated placement must not publish neighbor-triggered TNT events. */
import assert from 'node:assert/strict';
import { WorldFixture, until } from './harness';

const fixture = new WorldFixture('formation-preview-events', 'server-thread');
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 6, y = Math.ceil(py) + 5, z = Math.floor(pz) + 6;
  const at = (dx = 0, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const host = at(), target = at(1), tnt = at(2), power = at(-1);
  const primed = async () => ((await fixture.raw('simulation')).projectiles as any[]).filter(e => e.kind === 'minecraft:tnt');
  const block = async (p: string) => String((await fixture.raw(`getblock ${p}`)).block);
  await fixture.command(`fill ${at(-3, -2, -3)} ${at(5, -1, 3)} minecraft:stone`);
  await fixture.command(`fill ${at(-3, 0, -3)} ${at(5, 4, 3)} minecraft:air`);
  await fixture.command(`tp @s ${x + 1.5} ${y + 1} ${z - 2.5}`);
  await fixture.command('clear @s');
  await fixture.replace('hotbar.0', 'ae2:formation_plane');
  await fixture.select(0, 'ae2:formation_plane');
  await fixture.hold();
  await fixture.command(`setblock ${power} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${host} ae2:cable_bus`);
  assert.equal((await fixture.ae(`grid part ${host} east`)).attached, true);
  await fixture.stepServer(100);
  await until(() => fixture.ae(`grid part-status ${host} east`), v => v.online && v.powered, 'powered plane', 30000);
  // The TNT is diagonally adjacent to the torch, so it starts unpowered. The
  // torch strongly powers a conducting block directly above it; placing the
  // Drive there queues exactly one TNT prime from the real neighbor callback.
  await fixture.command(`setblock ${at(1, -1)} minecraft:redstone_torch`);
  await fixture.command(`setblock ${tnt} minecraft:tnt`);
  assert.match(await block(tnt), /^minecraft:tnt(?:\[|$)/);
  assert.equal((await primed()).length, 0);
  await fixture.hold(false);
  try {
    await fixture.replace('hotbar.1', 'ae2:drive');
    await fixture.select(1, 'ae2:drive');
  } finally { await fixture.hold(); }
  await fixture.stepServer(100);
  assert.equal((await primed()).length, 0, 'setup has no primed TNT');
  assert.equal((await fixture.ae(`grid deposit ${host}`)).inserted, 1);
  assert.match(await block(target), /^ae2:drive\[/);
  assert.match(await block(tnt), /^minecraft:air(?:\[|$)/);
  assert.equal((await primed()).length, 1, 'the committed neighbor event spawns TNT exactly once across preview and commit');
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('the preview does not publish or duplicate a real neighbor-triggered TNT event');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
