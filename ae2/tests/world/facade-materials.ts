/** Source FacadeItem placement and FacadePart physical return for varied materials. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('facade-materials', mode);
const line = (reply: any) => String(reply.chat?.[0] ?? '');
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
  await fixture.command(`tp @s ${x + 0.5} ${y} ${z + 2.5}`); await fixture.raw(`interact ${host} south`);
  await until(() => fixture.command(`data get block ${host} ae2_lab_config`), reply => line(reply).includes('ae2:fluix_glass_cable'), 'physical cable center installed');
  const hostData = () => fixture.command(`data get block ${host} ae2_lab_config.facades`);
  const put = async (bar: number, source: string, stored: string) => {
    await fixture.replace(`hotbar.${bar}`, `ae2:facade[ae2:facade_item="${source}",minecraft:custom_data={test_marker:1}]`);
    await fixture.select(bar, 'ae2:facade'); await fixture.raw(`interact ${host} south`);
    const installed = await until(hostData, reply => line(reply).includes(stored), `${source} facade installed`);
    assert.ok(!line(installed).includes('test_marker'), 'placed facade reconstructs only its block material');
  };
  const take = async (count: number, material: string) => {
    await fixture.select(0, 'ae2:certus_quartz_wrench'); await fixture.raw('key shift down');
    try { await fixture.raw(`interact ${host} south`); } finally { await fixture.raw('key shift up'); }
    const inventory = await until(() => fixture.raw('inventory'), view => view.counts?.['ae2:facade'] === count, 'physical facade returned exactly once');
    const result = [...inventory.hotbar, ...inventory.main].find((item: any) => item.item === 'ae2:facade' && String(item.components ?? '').includes(material));
    assert.ok(result, `returned facade contains ${material}`); assert.doesNotMatch(String(result.components), /test_marker/);
  };
  await fixture.replace('hotbar.0', 'ae2:certus_quartz_wrench');
  // createFacadeForItem's whitelist allows furnaces; placement accepts even
  // valid encoded block-entity materials and returns their material item.
  await put(1, 'minecraft:furnace', 'minecraft:furnace'); await take(1, 'minecraft:furnace');
  // Block.byItem(stick) is AIR, which FacadeItem maps to the glass default.
  await put(2, 'minecraft:stick', 'minecraft:glass');
  assert.ok(!line(await hostData()).includes('minecraft:stick'));
  await fixture.restart();
  assert.ok(line(await hostData()).includes('minecraft:glass'), 'glass facade state survives restart');
  await take(2, 'minecraft:glass');
  // A facade item is itself a non-block item; it follows the same fallback.
  await put(3, 'ae2:facade', 'minecraft:glass'); await take(3, 'minecraft:glass');
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('block-entity facade material places and returns its source item');
  fixture.mark('non-block and facade materials become glass and return exactly once, including across restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
