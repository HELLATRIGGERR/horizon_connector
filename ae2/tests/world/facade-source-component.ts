/** Source facade items place, render, and return the physical material. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('facade-source-component', mode);
const text = (reply: any) => JSON.stringify(reply.chat ?? []);
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const host = `${x} ${y} ${z}`;
  await fixture.command(`fill ${x - 2} ${y} ${z - 3} ${x + 2} ${y + 4} ${z + 3} minecraft:air`);
  await fixture.command(`fill ${x - 2} ${y - 1} ${z - 3} ${x + 2} ${y - 1} ${z + 3} minecraft:stone`);
  await fixture.command(`setblock ${host} ae2:cable_bus`); await fixture.command('clear @s');
  await fixture.replace('hotbar.0', 'ae2:terminal'); await fixture.select(0, 'ae2:terminal');
  assert.equal((await fixture.ae(`grid part ${host} north`)).attached, true);
  await fixture.command('gamemode survival @s');
  await fixture.replace('hotbar.0', 'ae2:fluix_glass_cable'); await fixture.select(0, 'ae2:fluix_glass_cable');
  await fixture.command(`tp @s ${x + 0.5} ${y} ${z + 2.5}`);
  await fixture.raw(`interact ${host} south`);
  await until(() => fixture.command(`data get block ${host} ae2_lab_config`), reply => text(reply).includes('ae2:fluix_glass_cable'), 'physical cable center installed');
  await fixture.replace('hotbar.0', 'ae2:facade[ae2:facade_item="minecraft:stone"]');
  await fixture.select(0, 'ae2:facade');
  await fixture.raw(`interact ${host} south`);
  const hostData = () => fixture.command(`data get block ${host} ae2_lab_config.facades`);
  await until(hostData, reply => text(reply).includes('ae2:facade_item') && text(reply).includes('minecraft:stone'), 'source-only facade mounted on physical cable');
  assert.equal((await fixture.raw('inventory')).counts?.['ae2:facade'] ?? 0, 0);
  await fixture.replace('hotbar.0', 'ae2:certus_quartz_wrench'); await fixture.select(0, 'ae2:certus_quartz_wrench');
  await fixture.raw('key shift down');
  try { await fixture.raw(`interact ${host} south`); } finally { await fixture.raw('key shift up'); }
  await until(() => fixture.raw('inventory'), view => view.counts?.['ae2:facade'] === 1, 'same physical facade returned');
  const returned = (await fixture.raw('inventory')).hotbar.find((item: any) => item.item === 'ae2:facade') ?? (await fixture.raw('inventory')).main.find((item: any) => item.item === 'ae2:facade');
  assert.match(String(returned?.components ?? ''), /ae2:facade_item/);
  assert.match(String(returned?.components ?? ''), /minecraft:stone/);
  assert.equal((await fixture.raw('inventory')).counts?.['ae2:facade'], 1);

  // Observer has two independent properties. Source FacadePart.onClicked
  // selects the next property on the wrench; onUseItemOn cycles its value.
  await fixture.replace('hotbar.2', 'ae2:facade[ae2:facade_item="minecraft:observer"]');
  await fixture.select(2, 'ae2:facade');
  await fixture.raw(`interact ${host} south`);
  const firstLine = (reply: any) => String(reply.chat?.[0] ?? '');
  const observer = await until(hostData, reply => firstLine(reply).includes('minecraft:observer') && firstLine(reply).includes('powered:'), 'observer facade state stored');
  const powered = firstLine(observer).match(/powered:\s*"(true|false)"/)?.[1];
  assert.ok(powered, firstLine(observer));
  await fixture.select(0, 'ae2:certus_quartz_wrench');
  // The preceding physical interaction aimed the crosshair at the same face.
  await fixture.raw('mouse left click');
  await until(() => fixture.raw('inventory'), view => String(view.hotbar?.[0]?.components ?? '').includes('ae2:facade_cycle_property') && String(view.hotbar?.[0]?.components ?? '').includes('powered'), 'wrench selects next facade property');
  await fixture.raw(`interact ${host} south`);
  await until(hostData, reply => firstLine(reply).includes(`powered: "${powered === 'true' ? 'false' : 'true'}"`), 'selected facade property cycles');
  await fixture.restart();
  const persisted = firstLine(await hostData());
  assert.ok(persisted.includes('minecraft:observer') && persisted.includes(`powered: "${powered === 'true' ? 'false' : 'true'}"`), persisted);
  const inventory = await fixture.raw('inventory');
  const wrench = inventory.hotbar?.[0];
  assert.match(String(wrench?.components ?? ''), /ae2:facade_cycle_property/);
  assert.equal(inventory.counts?.['ae2:facade'], 1, 'the original returned stone facade is still owned');
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('source-only facade material mounts and returns exactly once');
  fixture.mark('wrench selects and cycles facade properties with world and wrench state retained across restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
