/** A real local client receives and sends physical container components. */
import assert from 'node:assert/strict';
import { WorldFixture, until } from './harness';

const fixture = new WorldFixture('client-container-component-wire', 'server-thread');
const shulker = 'minecraft:shulker_box[minecraft:lock={components:{"minecraft:custom_name":"Key"}},minecraft:container_loot={loot_table:"minecraft:chests/simple_dungeon",seed:12L},minecraft:custom_data={"horizon:wire_ae2_components":"preexisting"}]';
const hive = 'minecraft:beehive[minecraft:bees=[{entity_data:{id:"minecraft:bee",CustomName:"Queen"},ticks_in_hive:7,min_ticks_in_hive:600}],minecraft:block_state={honey_level:"4"}]';
const contains = (slot: any, ...values: string[]) => values.every(value => String(slot?.components ?? '').includes(value));
try {
  await fixture.start();
  await fixture.command('clear @s');
  await fixture.replace('hotbar.0', shulker);
  await fixture.replace('hotbar.2', hive);
  const authoritative = JSON.stringify(await fixture.command('data get entity @s Inventory'));
  for (const key of ['minecraft:lock', 'minecraft:container_loot', 'simple_dungeon', 'minecraft:bees', 'Queen', 'minecraft:block_state']) {
    assert(authoritative.includes(key), `${key} exists in the authoritative inventory`);
  }
  await until(() => fixture.raw('inventory'), view => contains(view.hotbar?.[0], 'minecraft:lock', 'minecraft:container_loot', 'simple_dungeon', 'preexisting')
    && contains(view.hotbar?.[2], 'minecraft:bees', 'Queen', 'minecraft:block_state', 'honey_level'), 'server components reach the real client');
  fixture.mark('the local client receives the shulker lock and loot plus the hive bees and block state');

  await fixture.raw('open-inventory');
  const creative = await until(() => fixture.raw('screen'), view => view.screen === 'creative' && view.creative_tabs?.length, 'creative inventory');
  const inventoryTab = creative.creative_tabs.find((tab: any) => String(tab.id).endsWith('inventory'));
  assert(inventoryTab, 'creative player-inventory tab exists');
  await fixture.raw(`click-at ${inventoryTab.rect[0] + 13} ${inventoryTab.rect[1] + 16}`);
  const playerTab = await until(() => fixture.raw('screen'), view => String(view.creative_tab).endsWith('inventory'), 'creative player inventory tab');
  const clickHotbar = (slot: number) => fixture.raw(`click-at ${playerTab.origin[0] + 17 + slot * 18} ${playerTab.origin[1] + 120}`);
  await clickHotbar(0);
  await until(() => fixture.raw('inventory'), view => view.carried?.item === 'minecraft:shulker_box', 'actual client holds the shulker');
  await clickHotbar(1);
  await until(() => fixture.raw('inventory'), view => !view.carried && contains(view.hotbar?.[1], 'minecraft:lock', 'minecraft:container_loot'), 'client places the shulker in another slot');
  await clickHotbar(2);
  await until(() => fixture.raw('inventory'), view => view.carried?.item === 'minecraft:beehive', 'actual client holds the hive');
  await clickHotbar(3);
  await until(() => fixture.raw('inventory'), view => !view.carried && contains(view.hotbar?.[3], 'minecraft:bees', 'minecraft:block_state'), 'client places the hive in another slot');
  await fixture.raw('close');
  await fixture.select(1, 'minecraft:shulker_box');
  const selectedShulker = await until(() => fixture.command('data get entity @s SelectedItem'), reply => /simple_dungeon/.test(JSON.stringify(reply)) && /minecraft:lock/.test(JSON.stringify(reply)), 'client creative-slot update reaches the authoritative shulker');
  assert.match(JSON.stringify(selectedShulker), /preexisting/, 'colliding custom data survives the round trip');
  await fixture.select(3, 'minecraft:beehive');
  const selectedHive = await until(() => fixture.command('data get entity @s SelectedItem'), reply => /Queen/.test(JSON.stringify(reply)) && /honey_level/.test(JSON.stringify(reply)), 'client creative-slot update reaches the authoritative hive');
  assert.match(JSON.stringify(selectedHive), /minecraft:bees/);
  fixture.mark('actual creative-slot updates return all four components and colliding custom data to the server');

  await fixture.restart();
  await until(() => fixture.raw('inventory'), view => contains(view.hotbar?.[1], 'minecraft:lock', 'minecraft:container_loot', 'preexisting')
    && contains(view.hotbar?.[3], 'minecraft:bees', 'Queen', 'minecraft:block_state'), 'both client-visible items survive restart');
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('both component-bearing player items survive a restart and reach the client again');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
