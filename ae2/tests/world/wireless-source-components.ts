/** Registered wireless link and crafting inventory components own real items. */
import assert from 'node:assert/strict';
import { WorldFixture, until } from './harness';

const fixture = new WorldFixture('wireless-source-components', 'server-thread');
const slot = (view: any, index: number) => view.slots?.find((entry: any) => entry.slot === index);
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const at = (dx: number) => `${x + dx} ${y} ${z}`;
  const access = at(1);
  await fixture.command(`fill ${x - 2} ${y} ${z - 3} ${x + 5} ${y + 4} ${z + 3} minecraft:air`);
  await fixture.command(`fill ${x - 2} ${y - 1} ${z - 3} ${x + 5} ${y - 1} ${z + 3} minecraft:stone`);
  await fixture.command(`setblock ${at(0)} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${access} ae2:wireless_access_point[facing=east]`);
  await fixture.command(`setblock ${at(-1)} ae2:drive`);
  await until(() => fixture.ae(`grid status ${access}`), state => state.online && state.powered, 'powered wireless access point', 30000);
  await fixture.command('clear @s'); await fixture.command('gamemode survival @s');
  const item = `ae2:wireless_crafting_terminal[ae2:wireless_link_target={dimension:"minecraft:overworld",pos:[I;${x+1},${y},${z}]},ae2:stored_energy=10000.0d,ae2:crafting_inv=[{slot:0,item:{id:"minecraft:oak_log",count:1}}]]`;
  await fixture.replace('hotbar.0', item); await fixture.select(0, 'ae2:wireless_crafting_terminal');
  await fixture.command(`tp @s ${x + 1.5} ${y} ${z + 1.5}`);
  const open = async () => {
    await fixture.raw('use-item');
    return until(() => fixture.raw('screen'), view => view.screen === 'container' && slot(view, 327)?.item === 'minecraft:oak_log', 'source-only wireless link and physical crafting input');
  };
  const view = await open();
  assert.equal(slot(view, 336)?.item, 'minecraft:oak_planks'); assert.equal(slot(view, 336)?.count, 4);
  await fixture.raw('click-slot 336');
  await until(() => fixture.raw('inventory'), state => state.carried?.item === 'minecraft:oak_planks' && state.carried.count === 4, 'wireless recipe consumes its source input');
  await fixture.raw('click-slot 1'); await fixture.raw('close');
  const itemData = JSON.stringify((await fixture.command('data get entity @s SelectedItem')).chat);
  assert.match(itemData, /ae2:wireless_link_target/);
  assert.match(itemData, /ae2:crafting_inv/);
  assert.doesNotMatch(itemData, /minecraft:oak_log/);
  assert.equal((await fixture.raw('inventory')).counts?.['minecraft:oak_planks'], 4);
  fixture.mark('a source-only wireless link opens a physical matrix and crafting updates its item component');

  await fixture.restart();
  await until(() => fixture.ae(`grid status ${access}`), state => state.online && state.powered, 'reloaded wireless access point', 30000);
  await fixture.select(0, 'ae2:wireless_crafting_terminal');
  await fixture.raw('use-item');
  const reopened = await until(() => fixture.raw('screen'), view => view.screen === 'container' && slot(view, 327)?.item === null, 'source wireless item and empty matrix survive restart');
  assert.equal(slot(reopened, 336)?.item, null);
  assert.equal((await fixture.raw('inventory')).counts?.['minecraft:oak_planks'], 4);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('the linked owner and conserved output persist across restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
