/** The physical knife produces a source-component press used by an inscriber. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('quartz-knife-name-component', mode);
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 6, y = Math.ceil(py) + 3, z = Math.floor(pz) + 6;
  const machine = `${x} ${y} ${z}`, power = `${x + 1} ${y} ${z}`;
  await fixture.command('clear @s');
  await fixture.command(`fill ${x - 3} ${y - 1} ${z - 3} ${x + 3} ${y - 1} ${z + 3} minecraft:stone`);
  await fixture.command(`fill ${x - 3} ${y} ${z - 3} ${x + 3} ${y + 4} ${z + 3} minecraft:air`);
  await fixture.command(`tp @s ${x + 0.5} ${y} ${z - 2.5}`);
  await fixture.raw('hotbar 1');
  await fixture.replace('hotbar.0', 'ae2:certus_quartz_cutting_knife');
  await fixture.replace('hotbar.1', 'minecraft:iron_ingot', 2);
  await fixture.command('gamemode survival'); await fixture.raw('look 0 -80'); await fixture.raw('use-item');
  await until(() => fixture.raw('screen'), s => s.slots?.some((v: any) => v.slot === 600), 'physical quartz knife menu');
  await fixture.raw('type Hello'); await fixture.raw('click-slot 1');
  await until(() => fixture.raw('inventory'), s => s.carried?.item === 'minecraft:iron_ingot', 'physical ingots carried');
  await fixture.raw('click-slot 600');
  await until(() => fixture.raw('screen'), s => s.slots?.some((v: any) => v.slot === 600 && v.item === 'minecraft:iron_ingot' && v.count === 1), 'one ingot in knife');
  await fixture.raw('click-slot 1'); await until(() => fixture.raw('inventory'), s => !s.carried, 'remaining ingot returned');
  await until(() => fixture.raw('screen'), s => s.slots?.some((v: any) => v.slot === 650 && v.item === 'ae2:name_press'), 'name-press preview');
  await fixture.raw('click-slot 650');
  await until(() => fixture.raw('inventory'), s => s.carried?.item === 'ae2:name_press', 'physical name press taken');
  await fixture.raw('click-slot 2'); await until(() => fixture.raw('inventory'), s => !s.carried, 'press stored');
  await fixture.raw('close'); await fixture.select(2, 'ae2:name_press');
  const press = JSON.stringify(await fixture.command('data get entity @s SelectedItem'));
  assert.match(press, /ae2:name_press_name/); assert.match(press, /Hello/);
  assert.doesNotMatch(press, /minecraft:custom_data/, 'the name is the actual AE2 component');
  fixture.mark('the real quartz knife creates a physical source-component Name Press');

  await fixture.command(`setblock ${machine} ae2:inscriber`);
  await fixture.command(`setblock ${power} ae2:creative_energy_cell`);
  await fixture.raw(`interact ${machine} north`);
  await until(() => fixture.raw('screen'), s => s.slots?.some((v: any) => v.slot === 300), 'inscriber menu');
  await fixture.raw('click-slot 2');
  await until(() => fixture.raw('inventory'), s => s.carried?.item === 'ae2:name_press', 'press carried into machine');
  await fixture.raw('click-slot 300'); await until(() => fixture.raw('inventory'), s => !s.carried, 'press installed');
  await fixture.raw('click-slot 1');
  await until(() => fixture.raw('inventory'), s => s.carried?.item === 'minecraft:iron_ingot', 'input ingot carried');
  await fixture.raw('click-slot 302'); await until(() => fixture.raw('inventory'), s => !s.carried, 'input installed');
  await until(() => fixture.raw('screen'), s => s.slots?.some((v: any) => v.slot === 303 && v.item === 'minecraft:iron_ingot' && /Hello/.test(String(v.components))), 'named physical output', 30000);
  const physical = JSON.stringify(await fixture.command(`data get block ${machine}`));
  assert.match(physical, /ae2:name_press_name/); assert.match(physical, /Hello/);
  await fixture.raw('close'); await fixture.restart();
  await until(async () => JSON.stringify(await fixture.command(`data get block ${machine}`)), value => /ae2:name_press_name/.test(value) && /Hello/.test(value), 'knife press and output survive restart', 30000);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('the real press names an item and both survive restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
