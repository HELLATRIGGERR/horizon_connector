/** Direct AE2 Memory Card components on physical buses and machines. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('machine-memory-components', mode);
const text = (reply: any) => JSON.stringify(reply.chat ?? []);

try {
  const start = await fixture.start(); const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 24, y = Math.ceil(py) + 5, z = Math.floor(pz) + 24;
  const at = (dx: number) => `${x + dx} ${y} ${z}`;
  const data = (dx: number) => fixture.command(`data get block ${at(dx)}`);
  const goto = (dx: number) => fixture.command(`tp @s ${x + dx + .5} ${y} ${z + 2.5}`);
  const useCard = async (dx: number, item: string) => {
    await fixture.replace('hotbar.0', item); await fixture.select(0, 'ae2:memory_card'); await goto(dx);
    await fixture.raw(`interact ${at(dx)} south`);
  };
  await fixture.command(`fill ${x - 2} ${y} ${z - 3} ${x + 22} ${y + 3} ${z + 3} minecraft:air`);
  await fixture.command(`fill ${x - 2} ${y - 1} ${z - 3} ${x + 22} ${y - 1} ${z + 3} minecraft:stone`);
  await fixture.command('clear @s');
  await fixture.command(`setblock ${at(0)} ae2:cable_bus`);
  await fixture.replace('hotbar.0', 'ae2:export_bus'); await fixture.select(0, 'ae2:export_bus');
  assert.equal((await fixture.ae(`grid part ${at(0)} south`)).attached, true);
  for (const [dx, block] of [[4, 'ae2:interface'], [8, 'ae2:chest'], [12, 'ae2:condenser'], [16, 'ae2:inscriber'], [20, 'ae2:io_port']] as const)
    await fixture.command(`setblock ${at(dx)} ${block}`);
  await fixture.command(`item replace block ${at(8)} container.0 with ae2:item_storage_cell_1k`);
  await fixture.command(`item replace block ${at(12)} container.1 with ae2:cell_component_1k`);
  await fixture.command('gamemode survival');
  await fixture.replace('hotbar.1', 'ae2:speed_card');
  const bus = 'ae2:memory_card[ae2:exported_settings_source={translate:"item.ae2.export_bus"},ae2:exported_settings={redstone_controlled:"IGNORE",scheduling_mode:"ROUNDROBIN",fuzzy_mode:"IGNORE_ALL",craft_only:"NO"},ae2:exported_config_inv=[{"#t":"ae2:i",id:"minecraft:emerald","#":0L},{"#t":"ae2:i",id:"minecraft:diamond","#":0L}],ae2:exported_upgrades=[{id:"ae2:speed_card",count:1}]]';
  await useCard(0, bus);
  const busStatus = () => fixture.ae(`bus status ${at(0)} south`);
  const exported = await until(busStatus, s => s.state?.schedule === 'RoundRobin' && s.state?.speed_cards === 1 && s.state?.config?.[0]?.id === 'minecraft:emerald', 'source bus settings, ghost and physical card');
  assert.equal(exported.state.config[1].id, 'minecraft:diamond');
  await until(() => fixture.raw('inventory'), v => (v.counts?.['ae2:speed_card'] ?? 0) === 0, 'physical Speed Card moved to bus');
  fixture.mark('a source-only Export Bus card transfers filters and settings while consuming the real Speed Card');

  await fixture.replace('hotbar.1', 'ae2:fuzzy_card');
  const iface = 'ae2:memory_card[ae2:exported_settings_source={translate:"block.ae2.interface"},ae2:exported_settings={fuzzy_mode:"PERCENT_75"},ae2:exported_priority=23,ae2:exported_config_inv=[{"#t":"ae2:i",id:"minecraft:gold_ingot","#":3L}],ae2:exported_upgrades=[{id:"ae2:fuzzy_card",count:1}]]';
  await useCard(4, iface);
  await until(() => data(4), r => /priority:\s*23/.test(text(r)) && text(r).includes('minecraft:gold_ingot') && text(r).includes('ae2:fuzzy_card'), 'source Interface settings and physical upgrade');
  await until(() => fixture.raw('inventory'), v => (v.counts?.['ae2:fuzzy_card'] ?? 0) === 0, 'physical Fuzzy Card moved to Interface');
  await useCard(4, bus);
  const generic = await until(() => data(4), r => text(r).includes('minecraft:diamond') && !text(r).includes('minecraft:gold_ingot'), 'zero-amount bus filters apply to Interface');
  assert(!text(generic).includes('minecraft:emerald'), 'nonpositive source stack clears a previously occupied config slot');
  await until(() => fixture.raw('inventory'), v => (v.counts?.['ae2:fuzzy_card'] ?? 0) === 1, 'incompatible generic upgrade returns the physical Fuzzy Card');
  fixture.mark('a source Interface card imports its physical upgrade; a bus card applies generic zero-amount slot rules');

  const chest = 'ae2:memory_card[ae2:exported_settings_source={translate:"block.ae2.chest"},ae2:exported_settings={sort_by:"AMOUNT",view_mode:"STORED",sort_direction:"DESCENDING"},ae2:exported_priority=37,ae2:exported_upgrades=[]]';
  await useCard(8, chest);
  const chestData = await until(() => data(8), r => /priority:\s*37/.test(text(r)) && text(r).includes('ae2:item_storage_cell_1k'), 'source ME Chest card with physical cell');
  assert.match(text(chestData), /sort_by/); assert.match(text(chestData), /amount/);
  fixture.mark('source ME Chest settings apply without replacing its physical storage cell');

  // Accumulate real consumed items in trash mode, then let the imported source
  // mode invoke the normal condenser callback on the stored matter.
  for (let i = 0; i < 4; i++) {
    await fixture.replace('hotbar.0', 'minecraft:stone', 64); await fixture.select(0, 'minecraft:stone');
    assert.equal((await fixture.ae(`condenser insert ${at(12)} 64`)).consumed, 64);
  }
  const condenser = 'ae2:memory_card[ae2:exported_settings_source={translate:"block.ae2.condenser"},ae2:exported_settings={condenser_output:"MATTER_BALLS"}]';
  await useCard(12, condenser);
  await until(() => data(12), r => text(r).includes('ae2:matter_ball') && text(r).includes('ae2:cell_component_1k'), 'source Condenser setting produces physical matter ball');
  fixture.mark('source Condenser mode triggers the real output callback on 256 consumed items');

  await fixture.replace('hotbar.1', 'ae2:speed_card');
  const inscriber = 'ae2:memory_card[ae2:exported_settings_source={translate:"block.ae2.inscriber"},ae2:exported_upgrades=[{id:"ae2:speed_card",count:1}]]';
  await useCard(16, inscriber);
  await until(() => data(16), r => text(r).includes('ae2:speed_card'), 'source Inscriber card transfers its physical Speed Card');
  const styled = 'ae2:memory_card[ae2:exported_settings_source={translate:"block.ae2.inscriber",color:"red"},ae2:exported_custom_name="foreign name",ae2:exported_settings={auto_export:"YES"}]';
  await useCard(16, styled);
  const inscriberData = await until(() => data(16), r => /auto_export.*YES/.test(text(r)) && text(r).includes('ae2:speed_card'), 'foreign generic Inscriber setting with upgrade absent');
  assert(!text(inscriberData).includes('foreign name'), 'styled source is not the matching machine description');
  const io = 'ae2:memory_card[ae2:exported_settings_source={translate:"block.ae2.inscriber"},ae2:exported_settings={operation_mode:"FILL",redstone_controlled:"HIGH_SIGNAL"}]';
  await useCard(20, io);
  await until(() => data(20), r => /io_operation.*fill/.test(text(r)) && /io_redstone.*high/.test(text(r)), 'foreign IO Port generic settings');
  fixture.mark('foreign source settings apply to Inscriber and IO Port without removing an absent upgrade component or copying the name');

  await fixture.restart();
  assert.equal((await busStatus()).state.speed_cards, 1);
  assert.match(text(await data(4)), /minecraft:diamond/);
  assert.match(text(await data(8)), /ae2:item_storage_cell_1k/);
  assert.match(text(await data(12)), /ae2:matter_ball/);
  assert.match(text(await data(16)), /ae2:speed_card/); assert.match(text(await data(20)), /io_operation.*fill/);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('physical upgrade, filter, cell and condenser output survive restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
