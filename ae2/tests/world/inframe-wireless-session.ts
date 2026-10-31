/** In-frame wireless crafting owns its item, matrix and key-type settings. */
import assert from 'node:assert/strict';
import { WorldFixture, until } from './harness';

const fixture = new WorldFixture('inframe-wireless-session', 'in-frame');
const slot = (view: any, index: number) => view.slots?.find((entry: any) => entry.slot === index);
const shown = (view: any, item: string) => view.slots?.some((entry: any) => entry.slot >= 300 && entry.slot < 327 && entry.item === item);
try {
  const start = await fixture.start(), [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 24, y = Math.ceil(py) + 5, z = Math.floor(pz) + 24;
  const at = (dx: number) => `${x + dx} ${y} ${z}`, access = at(1), drive = at(-1);
  await fixture.command(`tp @s ${x + 1.5} ${y} ${z + 2.5}`);
  for (const [dx, dz] of [[-3, -3], [-3, 3], [4, -3], [4, 3]] as const) {
    await until(() => fixture.raw(`block ${x + dx} ${y} ${z + dz}`), state => state.loaded, 'fixture corner chunk loaded', 30000);
  }
  await fixture.command(`fill ${x - 3} ${y} ${z - 3} ${x + 4} ${y + 4} ${z + 3} minecraft:air`);
  await fixture.command(`fill ${x - 3} ${y - 1} ${z - 3} ${x + 4} ${y - 1} ${z + 3} minecraft:stone`);
  await fixture.command(`setblock ${at(0)} ae2:creative_energy_cell`); await fixture.command(`setblock ${access} ae2:wireless_access_point[facing=east]`); await fixture.command(`setblock ${drive} ae2:drive`);
  await fixture.command(`item replace block ${drive} container.0 with ae2:item_storage_cell_1k[ae2:storage_cell_inv=[{"#t":"ae2:i",id:"minecraft:oak_log","#":1L}]]`);
  await fixture.command(`item replace block ${drive} container.1 with ae2:fluid_storage_cell_1k[ae2:storage_cell_inv=[{"#t":"ae2:f",id:"minecraft:water","#":1000L}]]`);
  await until(() => fixture.ae(`grid status ${access}`), state => state.online && state.powered, 'powered wireless access point', 30000);
  await fixture.command('clear @s'); await fixture.command('gamemode survival @s');
  const terminal = `ae2:wireless_crafting_terminal[ae2:wireless_link_target={dimension:"minecraft:overworld",pos:[I;${x + 1},${y},${z}]},ae2:stored_energy=10000.0d,ae2:crafting_inv=[{slot:0,item:{id:"minecraft:oak_log",count:1}}]]`;
  await fixture.replace('hotbar.0', terminal); await fixture.select(0, 'ae2:wireless_crafting_terminal');
  const open = async () => { await fixture.raw('look 0 -80'); await fixture.raw('use-item');
    return until(() => fixture.raw('screen'), view => slot(view, 327)?.item === 'minecraft:oak_log' && shown(view, 'minecraft:water_bucket'), 'owned wireless crafting terminal with item and fluid storage'); };
  let view = await open(); assert.equal(slot(view, 336)?.item, 'minecraft:oak_planks');
  await fixture.raw('click-slot 0'); await until(() => fixture.raw('inventory'), state => !state.carried && state.hotbar[0]?.item === 'ae2:wireless_crafting_terminal', 'owner item cannot be moved out of its open menu');
  await fixture.raw('click-slot 352'); await until(() => fixture.raw('screen'), state => String(slot(state, 301)?.components ?? '').includes('Fluids: enabled'), 'key-type selector opens');
  await fixture.raw('click-slot 301'); await fixture.raw('click-slot 302');
  view = await until(() => fixture.raw('screen'), state => shown(state, 'minecraft:oak_log') && !shown(state, 'minecraft:water_bucket'), 'source key selection hides fluids');
  await fixture.raw('click-slot 352'); await fixture.raw('click-slot 300');
  await until(() => fixture.raw('screen'), state => String(slot(state, 300)?.components ?? '').includes('Items: enabled'), 'last enabled type cannot be disabled');
  await fixture.raw('click-slot 301'); await fixture.raw('click-slot 302');
  await until(() => fixture.raw('screen'), state => shown(state, 'minecraft:water_bucket'), 'fluid type restored');
  await fixture.raw('click-slot 336');
  await until(() => fixture.raw('inventory'), state => state.carried?.item === 'minecraft:oak_planks' && state.carried.count === 4, 'wireless crafting produces four real planks');
  await fixture.raw('close'); await until(() => fixture.raw('inventory'), state => !state.carried && state.counts?.['minecraft:oak_planks'] === 4, 'wireless output returned once');
  await fixture.select(0, 'ae2:wireless_crafting_terminal');
  const savedOwner = JSON.stringify(await fixture.command('data get entity @s SelectedItem')); assert.match(savedOwner, /ae2:crafting_inv/); assert.match(savedOwner, /minecraft:oak_log/); assert.match(savedOwner, /ae2:stored_energy/);
  await fixture.restart();
  await until(() => fixture.ae(`grid status ${access}`), state => state.online && state.powered, 'reloaded wireless access point', 30000);
  await fixture.select(0, 'ae2:wireless_crafting_terminal'); view = await open(); assert.equal(slot(view, 336)?.item, 'minecraft:oak_planks');
  await fixture.raw('click-slot 336'); await until(() => fixture.raw('inventory'), state => state.carried?.item === 'minecraft:oak_planks' && state.carried.count === 4, 'retained matrix crafts after restart');
  await fixture.raw('close'); await until(() => fixture.raw('inventory'), state => !state.carried && state.counts?.['minecraft:oak_planks'] === 8, 'both outputs retained without duplication');
  const finalOwner = JSON.stringify(await fixture.command('data get entity @s SelectedItem')); assert.doesNotMatch(finalOwner, /minecraft:oak_log/);
  fixture.assertHidden(await fixture.raw('status')); fixture.mark('wireless item ownership, key types, crafting/refill and exact output survive restart'); fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
