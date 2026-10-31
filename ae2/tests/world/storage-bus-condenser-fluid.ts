/** Storage Bus uses the Condenser's ME storage, including output gating. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('storage-bus-condenser-fluid', mode);
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 6, y = Math.ceil(py) + 6, z = Math.floor(pz) + 6;
  const at = (dx = 0, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const host = at(), power = at(-1), condenser = at(1);
  await fixture.command(`fill ${at(-3, -2, -3)} ${at(4, -2, 3)} minecraft:stone`);
  await fixture.command(`fill ${at(-3, -1, -3)} ${at(4, 3, 3)} minecraft:air`);
  await fixture.command(`tp @s ${x + 2.5} ${y + 1} ${z - 2.5}`);
  await fixture.command('clear @s');
  await fixture.replace('hotbar.0', 'ae2:storage_bus');
  await fixture.select(0, 'ae2:storage_bus');
  await fixture.hold();
  await fixture.command(`setblock ${power} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${condenser} ae2:condenser`);
  await fixture.command(`setblock ${host} ae2:cable_bus`);
  await fixture.command(`item replace block ${condenser} container.1 with ae2:cell_component_1k`);
  assert.equal((await fixture.ae(`grid part ${host} east`)).attached, true);
  await fixture.stepServer(120);
  await until(() => fixture.ae(`grid status ${host}`), s => s.online && s.powered, 'storage bus grid online');
  const state = () => fixture.ae(`condenser status ${condenser}`);
  assert.equal((await state()).stored, 0);
  // Player inventory changes are delivered to the client on server ticks.
  // No machine in this setup transfers fluid without the explicit command.
  await fixture.hold(false);

  await fixture.replace('hotbar.1', 'minecraft:water_bucket');
  await fixture.select(1, 'minecraft:water_bucket');
  assert.equal((await fixture.ae(`fluid grid insert ${host}`)).inserted_mb, 1000);
  assert.equal((await state()).stored, 8, 'one real bucket contributes eight source fluid operations');
  assert.equal((await fixture.ae(`fluid grid status ${host}`)).entries.length, 0, 'Condenser consumes fluid and advertises no fluid stock');
  fixture.mark('a physical bucket enters the adjacent Condenser through its Storage Bus');

  await fixture.ae(`condenser output ${condenser} matter_balls`);
  await fixture.command(`item replace block ${condenser} container.0 with ae2:matter_ball 64`);
  await fixture.replace('hotbar.1', 'minecraft:water_bucket');
  await fixture.select(1, 'minecraft:water_bucket');
  const refused = await fixture.command(`ae2 fluid grid insert ${host}`);
  assert(!(refused.chat ?? []).some((line: string) => line.startsWith('AE2 {')), 'full output rejects the ME-storage insertion');
  assert.match(JSON.stringify(await fixture.command('data get entity @s SelectedItem')), /minecraft:water_bucket/);
  assert.equal((await state()).stored, 8, 'a rejected bucket changes no condenser power');
  fixture.mark('a full output blocks the ME-storage operation without consuming the physical bucket');

  await fixture.command(`item replace block ${condenser} container.0 with ae2:matter_ball 63`);
  assert.equal((await fixture.ae(`fluid grid insert ${host}`)).inserted_mb, 1000);
  assert.equal((await state()).stored, 16);
  await fixture.restart();
  await until(() => fixture.ae(`grid status ${host}`), s => s.online && s.powered, 'storage bus restored', 30000);
  assert.equal((await state()).stored, 16);
  assert.equal((await fixture.ae(`fluid grid status ${host}`)).entries.length, 0);
  const output = await fixture.raw(`container ${condenser}`);
  assert(output.slots?.some((slot: any) => slot?.item === 'ae2:matter_ball' && slot.count === 63));
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('freeing output resumes fluid insertion, and power and output survive restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
