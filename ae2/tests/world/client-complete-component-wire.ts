/** A real local client preserves physical source component patches on return. */
import assert from 'node:assert/strict';
import { WorldFixture, until } from './harness';

const fixture = new WorldFixture('client-complete-component-wire', 'server-thread');
const spawner = 'minecraft:spawner[minecraft:block_entity_data={id:"minecraft:mob_spawner",SpawnData:{entity:{id:"minecraft:zombie"}}},minecraft:custom_data={"horizon:wire_ae2_components":"existing"}]';
const banner = 'minecraft:white_banner[minecraft:banner_patterns=[{pattern:"minecraft:flower",color:"black"}],minecraft:custom_name={text:"Garden",color:"red"}]';
const limited = 'minecraft:gold_ingot[minecraft:max_stack_size=5]';
const removed = 'minecraft:diamond_pickaxe[!minecraft:tool]';
const components = (slot: any) => String(slot?.components ?? '');
const has = (slot: any, ...values: string[]) => values.every(value => components(slot).includes(value));
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 5, y = Math.ceil(py) + 4, z = Math.floor(pz) + 5;
  await fixture.command(`fill ${x - 2} ${y - 1} ${z - 3} ${x + 3} ${y + 3} ${z + 3} minecraft:air`);
  await fixture.command(`fill ${x - 2} ${y - 1} ${z - 3} ${x + 3} ${y - 1} ${z + 3} minecraft:stone`);
  await fixture.command(`tp @s ${x + 0.5} ${y} ${z + 2.5}`);
  await fixture.command('clear @s');
  await fixture.replace('hotbar.0', spawner);
  await fixture.replace('hotbar.2', banner);
  await fixture.replace('hotbar.4', limited);
  await fixture.replace('hotbar.6', removed);
  const server = JSON.stringify(await fixture.command('data get entity @s Inventory'));
  for (const value of ['minecraft:block_entity_data', 'minecraft:zombie', 'minecraft:banner_patterns', 'minecraft:flower', 'minecraft:max_stack_size', '!minecraft:tool']) {
    assert(server.includes(value), `${value} is present in authoritative inventory`);
  }
  await until(() => fixture.raw('inventory'), view => has(view.hotbar?.[0], 'minecraft:block_entity_data', 'minecraft:zombie', 'existing')
    && has(view.hotbar?.[2], 'minecraft:banner_patterns', 'minecraft:flower', 'Garden', 'red')
    && has(view.hotbar?.[4], 'minecraft:max_stack_size') && has(view.hotbar?.[6], '!minecraft:tool'), 'all source components reach the real client');
  fixture.mark('the actual client receives block-entity data, banner layers, the stack limit and a source removal');

  await fixture.raw('open-inventory');
  const creative = await until(() => fixture.raw('screen'), view => view.screen === 'creative' && view.creative_tabs?.length, 'creative inventory');
  const inventoryTab = creative.creative_tabs.find((tab: any) => String(tab.id).endsWith('inventory'));
  assert(inventoryTab);
  await fixture.raw(`click-at ${inventoryTab.rect[0] + 13} ${inventoryTab.rect[1] + 16}`);
  const playerTab = await until(() => fixture.raw('screen'), view => String(view.creative_tab).endsWith('inventory'), 'creative player inventory tab');
  const clickHotbar = (slot: number) => fixture.raw(`click-at ${playerTab.origin[0] + 17 + slot * 18} ${playerTab.origin[1] + 120}`);
  for (const [from,to,item] of [[0,1,'minecraft:spawner'],[2,3,'minecraft:white_banner'],[4,5,'minecraft:gold_ingot'],[6,7,'minecraft:diamond_pickaxe']] as const) {
    await clickHotbar(from);
    await until(() => fixture.raw('inventory'), view => view.carried?.item === item, `client holds ${item}`);
    await clickHotbar(to);
    await until(() => fixture.raw('inventory'), view => !view.carried && view.hotbar?.[to]?.item === item, `client places ${item} in its new slot`);
  }
  await fixture.raw('close');
  for (const [slot,item,expected] of [[1,'minecraft:spawner','minecraft:zombie'],[3,'minecraft:white_banner','minecraft:flower'],[5,'minecraft:gold_ingot','minecraft:max_stack_size'],[7,'minecraft:diamond_pickaxe','!minecraft:tool']] as const) {
    await fixture.select(slot,item);
    await until(() => fixture.command('data get entity @s SelectedItem'), reply => JSON.stringify(reply).includes(expected), `creative-slot update returns ${expected}`);
  }
  fixture.mark('actual creative-slot packets return all four component-bearing items to the server');
  await fixture.restart();
  await until(() => fixture.raw('inventory'), view => has(view.hotbar?.[1], 'minecraft:block_entity_data', 'minecraft:zombie', 'existing')
    && has(view.hotbar?.[3], 'minecraft:banner_patterns', 'minecraft:flower', 'Garden')
    && has(view.hotbar?.[5], 'minecraft:max_stack_size') && has(view.hotbar?.[7], '!minecraft:tool'), 'all component-bearing items survive restart');
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('all four item patches survive a world restart and reach the client again');

  await fixture.select(3, 'minecraft:white_banner');
  await fixture.raw(`interact ${x} ${y - 1} ${z} up`);
  await until(() => fixture.command(`data get block ${x} ${y} ${z}`), reply => JSON.stringify(reply).includes('minecraft:flower'), 'placed banner retains its source layer');
  await fixture.select(1, 'minecraft:spawner');
  await fixture.raw(`interact ${x + 1} ${y - 1} ${z} up`);
  await until(() => fixture.command(`data get block ${x + 1} ${y} ${z}`), reply => JSON.stringify(reply).includes('minecraft:zombie'), 'placed spawner retains its block-entity data');
  fixture.mark('real player placement applies the transported banner layers and spawner data');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
