/** Physical Formation Plane fluid placement and source-custody regression. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('formation-fluid-source', mode);
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 6, y = Math.ceil(py) + 5, z = Math.floor(pz) + 6;
  const at = (dx = 0, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const host = at(), power = at(-1), target = at(1);
  const block = async () => (await fixture.raw(`getblock ${target}`)).block as string;
  const selected = async () => JSON.stringify(await fixture.command('data get entity @s SelectedItem'));
  const refuse = async () => {
    const reply = await fixture.command(`ae2 fluid grid insert ${host}`);
    assert(!(reply.chat ?? []).some((line: string) => line.startsWith('AE2 {')), 'grid refuses this full-bucket transfer');
  };
  const bucket = async (item: string) => {
    await fixture.replace('hotbar.1', item);
    await fixture.select(1, item);
  };
  await fixture.command(`fill ${at(-3, -1, -3)} ${at(4, -1, 3)} minecraft:stone`);
  await fixture.command(`fill ${at(-3, 0, -3)} ${at(4, 4, 3)} minecraft:air`);
  await fixture.command(`tp @s ${x + 1.5} ${y + 1} ${z - 2.5}`);
  await fixture.command('clear @s');
  await fixture.command('tick freeze');
  await fixture.command(`setblock ${power} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${host} ae2:cable_bus`);
  await fixture.replace('hotbar.0', 'ae2:formation_plane');
  await fixture.select(0, 'ae2:formation_plane');
  assert.equal((await fixture.ae(`grid part ${host} east`)).attached, true);
  await until(() => fixture.ae(`grid status ${host}`), s => s.online && s.powered, 'formation plane grid online');

  await bucket('minecraft:water_bucket');
  assert.equal((await fixture.ae(`fluid grid insert ${host}`)).inserted_mb, 1000);
  assert.match(await block(), /^minecraft:water(?:\[|$)/);
  assert.match(await selected(), /minecraft:bucket/, 'the physical source became an empty bucket');
  await bucket('minecraft:water_bucket');
  await refuse();
  assert.match(await selected(), /minecraft:water_bucket/, 'an identical source rejects the bucket without consuming it');
  assert.match(await block(), /^minecraft:water(?:\[|$)/);
  fixture.mark('one bucket forms a source; a second identical source is rejected with custody intact');

  // This target change must clear the preceding failed-placement cache. Source
  // SimpleWaterloggedBlock.canPlaceLiquid accepts water even when already wet;
  // AE2 ignores the later placeLiquid(false) result and consumes another bucket.
  await fixture.command(`setblock ${target} minecraft:oak_slab[type=bottom,waterlogged=false]`);
  assert.equal((await fixture.ae(`fluid grid insert ${host}`)).inserted_mb, 1000);
  assert.match(await block(), /minecraft:oak_slab\[.*waterlogged=true/);
  await bucket('minecraft:water_bucket');
  assert.equal((await fixture.ae(`fluid grid insert ${host}`)).inserted_mb, 1000);
  assert.match(await block(), /minecraft:oak_slab\[.*waterlogged=true/);
  assert.match(await selected(), /minecraft:bucket/);
  fixture.mark('neighbor change clears the blocked cache and both dry and already-wet slabs accept water as in source');

  await fixture.command(`setblock ${target} minecraft:air`);
  await bucket('minecraft:lava_bucket');
  assert.equal((await fixture.ae(`fluid grid insert ${host}`)).inserted_mb, 1000);
  assert.match(await block(), /^minecraft:lava(?:\[|$)/);
  await fixture.restart();
  await until(() => fixture.ae(`grid status ${host}`), s => s.online && s.powered, 'formation plane restored', 30000);
  assert.match(await block(), /^minecraft:lava(?:\[|$)/, 'placed lava survives restart');
  fixture.mark('lava uses its own source block and survives restart');

  await fixture.command(`setblock ${target} minecraft:air`);
  await bucket('minecraft:water_bucket');
  assert.equal((await fixture.ae(`bus filter_fluid ${host} east 0`)).configured, true);
  await bucket('minecraft:lava_bucket');
  await refuse();
  assert.match(await selected(), /minecraft:lava_bucket/);
  assert.match(await block(), /^minecraft:air(?:\[|$)/);
  await bucket('minecraft:water_bucket');
  assert.equal((await fixture.ae(`fluid grid insert ${host}`)).inserted_mb, 1000);
  assert.match(await block(), /^minecraft:water(?:\[|$)/);
  fixture.mark('the physical fluid filter rejects lava and admits the configured water');

  await fixture.command(`setblock ${target} minecraft:air`);
  await fixture.command(`setblock ${power} minecraft:air`);
  await until(() => fixture.ae(`grid status ${host}`), s => !s.powered, 'formation grid loses power', 30000);
  await bucket('minecraft:water_bucket');
  await refuse();
  assert.match(await selected(), /minecraft:water_bucket/);
  assert.match(await block(), /^minecraft:air(?:\[|$)/);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('an unpowered plane leaves both the bucket and target untouched');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
