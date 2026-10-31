/** Source key-type preferences filter the physical wireless network view. */
import assert from 'node:assert/strict';
import { WorldFixture, until } from './harness';

const fixture = new WorldFixture('wireless-key-types', 'server-thread');
const slot = (view: any, index: number) => view.slots?.find((entry: any) => entry.slot === index);
const shown = (view: any, item: string) => (view.slots ?? []).some((entry: any) => entry.slot >= 300 && entry.slot < 345 && entry.item === item);
const label = (view: any, index: number) => String(slot(view, index)?.components ?? '');
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const at = (dx: number) => `${x + dx} ${y} ${z}`;
  const access = at(1), drive = at(-1);
  await fixture.command(`fill ${x - 3} ${y} ${z - 3} ${x + 4} ${y + 4} ${z + 3} minecraft:air`);
  await fixture.command(`fill ${x - 3} ${y - 1} ${z - 3} ${x + 4} ${y - 1} ${z + 3} minecraft:stone`);
  await fixture.command(`setblock ${at(0)} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${access} ae2:wireless_access_point[facing=east]`);
  await fixture.command(`setblock ${drive} ae2:drive`);
  await fixture.command(`item replace block ${drive} container.0 with ae2:item_storage_cell_1k[ae2:storage_cell_inv=[{"#t":"ae2:i",id:"minecraft:oak_log","#":2L}]]`);
  await fixture.command(`item replace block ${drive} container.1 with ae2:fluid_storage_cell_1k[ae2:storage_cell_inv=[{"#t":"ae2:f",id:"minecraft:water","#":1000L}]]`);
  await until(() => fixture.ae(`grid status ${access}`), state => state.online && state.powered, 'wireless access point joins powered drive', 30000);
  await until(() => fixture.ae(`fluid grid status ${drive}`), state => state.entries?.some((entry: any) => entry.fluid === 'minecraft:water'), 'physical water stored');
  await fixture.command('clear @s'); await fixture.command('gamemode survival @s');
  const terminal = (types: string) => `ae2:wireless_terminal[ae2:wireless_link_target={dimension:"minecraft:overworld",pos:[I;${x + 1},${y},${z}]},ae2:stored_energy=10000.0d,ae2:enabled_key_types=${types}]`;
  await fixture.replace('hotbar.0', terminal('["ae2:i"]')); await fixture.select(0, 'ae2:wireless_terminal');
  await fixture.command(`tp @s ${x + 1.5} ${y} ${z + 1.5}`); await fixture.raw('use-item');
  await until(() => fixture.raw('screen'), view => shown(view, 'minecraft:oak_log') && !shown(view, 'minecraft:water_bucket'), 'source item-only selection');
  await fixture.raw('click-slot 352');
  await until(() => fixture.raw('screen'), view => /Items: enabled/.test(label(view,300)) && /Fluids: disabled/.test(label(view,301)), 'key-type selection screen');
  await fixture.raw('click-slot 301');
  await until(() => fixture.raw('screen'), view => /Fluids: enabled/.test(label(view,301)), 'fluids enabled');
  await fixture.raw('click-slot 300');
  await until(() => fixture.raw('screen'), view => /Items: disabled/.test(label(view,300)), 'items disabled');
  await fixture.raw('click-slot 301');
  await until(() => fixture.raw('screen'), view => /Fluids: enabled/.test(label(view,301)), 'last selected type cannot be disabled');
  await fixture.raw('click-slot 302');
  await until(() => fixture.raw('screen'), view => shown(view, 'minecraft:water_bucket') && !shown(view, 'minecraft:oak_log'), 'fluid-only selection filters live results');
  await fixture.raw('close');
  assert.match(JSON.stringify((await fixture.command('data get entity @s SelectedItem')).chat?.[0]), /ae2:enabled_key_types.*ae2:f/);
  await fixture.restart();
  await until(() => fixture.ae(`grid status ${access}`), state => state.online && state.powered, 'reloaded wireless access point', 30000);
  await fixture.select(0, 'ae2:wireless_terminal'); await fixture.raw('use-item');
  await until(() => fixture.raw('screen'), view => shown(view, 'minecraft:water_bucket') && !shown(view, 'minecraft:oak_log'), 'fluid selection survives reload');
  await fixture.raw('click-slot 352 shift');
  await until(() => fixture.raw('screen'), view => shown(view, 'minecraft:water_bucket') && shown(view, 'minecraft:oak_log'), 'shift-click cycles back to all types');
  await fixture.raw('close');
  await fixture.replace('hotbar.0', terminal('[]')); await fixture.select(0, 'ae2:wireless_terminal'); await fixture.raw('use-item');
  await until(() => fixture.raw('screen'), view => view.screen === 'container' && !shown(view, 'minecraft:water_bucket') && !shown(view, 'minecraft:oak_log'), 'explicit empty selection remains empty');
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('source item, fluid and empty type selections filter real network entries');
  fixture.mark('selection controls preserve the last enabled type and persist on the wireless owner through restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
