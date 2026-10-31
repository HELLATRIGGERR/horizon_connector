/** Wrapped generic fluids are not normally picked up and are drained once. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';
const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('wrapped-player-inventory', mode);
const slot = (view: any, index: number) => view.slots?.find((entry: any) => entry.slot === index);
try {
  const start = await fixture.start(); await fixture.command('clear @s'); await fixture.command('gamemode survival @s');
  await fixture.replace('hotbar.0', 'ae2:wrapped_generic_stack[ae2:wrapped_stack={"#t":"ae2:f",id:"minecraft:water","#":2000L}]');
  await fixture.replace('hotbar.1', 'minecraft:bucket', 2);
  await fixture.raw('key e tap');
  await until(() => fixture.raw('screen'), view => view.screen === 'inventory' && slot(view,0)?.item === 'ae2:wrapped_generic_stack', 'player inventory with physical wrapper');
  await fixture.raw('click-slot 0');
  await until(() => fixture.raw('inventory'), view => !view.carried && view.hotbar?.[0]?.item === 'ae2:wrapped_generic_stack', 'wrapper cannot be normally picked up');
  await fixture.raw('click-slot 1');
  await until(() => fixture.raw('inventory'), view => view.carried?.item === 'minecraft:bucket' && view.carried.count === 2, 'two empty buckets carried');
  await fixture.raw('click-slot 0');
  const first = await until(() => fixture.raw('inventory'), view => view.carried?.item === 'minecraft:bucket' && view.carried.count === 1 && view.counts?.['minecraft:water_bucket'] === 1, 'first bucket fills and returns to inventory');
  assert.match(String(first.hotbar?.[0]?.components ?? ''), /1000/);
  await fixture.raw('close'); await until(() => fixture.raw('inventory'), view => !view.carried && view.counts?.['minecraft:bucket'] === 1, 'remaining empty bucket returned');
  await fixture.restart();
  const saved = await fixture.raw('inventory'); assert.equal(saved.counts?.['minecraft:water_bucket'], 1); assert.match(String(saved.hotbar?.[0]?.components ?? ''), /1000/);
  await fixture.raw('key e tap'); await until(() => fixture.raw('screen'), view => view.screen === 'inventory', 'reopened player inventory');
  const inventory = await fixture.raw('inventory'); const bucket = [...inventory.hotbar, ...inventory.main].find((entry: any) => entry.item === 'minecraft:bucket');
  assert.ok(bucket); await fixture.raw(`click-slot ${bucket.slot}`); await fixture.raw('click-slot 0');
  await until(() => fixture.raw('inventory'), view => view.carried?.item === 'minecraft:water_bucket' && !view.counts?.['ae2:wrapped_generic_stack'] && view.counts?.['minecraft:water_bucket'] === 2, 'second bucket drains and removes wrapper');
  await fixture.raw('close');
  await until(() => fixture.raw('inventory'), view => !view.carried && view.counts?.['minecraft:water_bucket'] === 2, 'second bucket returned to inventory');
  fixture.mark('a wrapped fluid fills two physical buckets exactly once across restart');

  // The same item override must run on player slots inside a terminal.
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const at = (dx: number) => `${x + dx} ${y} ${z}`;
  await fixture.command(`fill ${x - 2} ${y} ${z - 3} ${x + 4} ${y + 4} ${z + 3} minecraft:air`);
  await fixture.command(`fill ${x - 2} ${y - 1} ${z - 3} ${x + 4} ${y - 1} ${z + 3} minecraft:stone`);
  await fixture.command(`setblock ${at(0)} ae2:creative_energy_cell`); await fixture.command(`setblock ${at(1)} ae2:drive`); await fixture.command(`setblock ${at(2)} ae2:cable_bus`);
  await fixture.replace('hotbar.8', 'ae2:terminal'); await fixture.select(8, 'ae2:terminal');
  assert.equal((await fixture.ae(`grid part ${at(2)} north`)).attached, true);
  await until(() => fixture.ae(`grid status ${at(2)}`), state => state.online && state.powered, 'terminal grid ready', 30000);
  await fixture.replace('hotbar.6', 'ae2:wrapped_generic_stack[ae2:wrapped_stack={"#t":"ae2:f",id:"minecraft:water","#":1000L}]');
  await fixture.replace('hotbar.7', 'minecraft:bucket'); await fixture.replace('hotbar.8', 'minecraft:stick'); await fixture.select(8, 'minecraft:stick');
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`); await fixture.raw(`interact ${at(2)} north`);
  await until(() => fixture.raw('screen'), view => view.screen === 'container', 'physical terminal opens');
  await fixture.raw('click-slot 6');
  await until(() => fixture.raw('inventory'), view => !view.carried && view.hotbar?.[6]?.item === 'ae2:wrapped_generic_stack', 'terminal player slot refuses wrapper pickup');
  await fixture.raw('click-slot 7'); await until(() => fixture.raw('inventory'), view => view.carried?.item === 'minecraft:bucket', 'terminal bucket on cursor');
  await fixture.raw('click-slot 6');
  await until(() => fixture.raw('inventory'), view => view.carried?.item === 'minecraft:water_bucket' && !view.counts?.['ae2:wrapped_generic_stack'] && view.counts?.['minecraft:water_bucket'] === 3, 'terminal player slot drains exactly one bucket');
  fixture.assertHidden(await fixture.raw('status')); fixture.mark('the source item override also applies inside terminal player slots'); fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
