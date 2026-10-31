/** Source Interface per-item limits and the registered four-bucket fluid cap. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';
const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('interface-source-capacity', mode);
const text = (reply: any) => JSON.stringify(reply.chat ?? []);
try {
  const start = await fixture.start(), [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 24, y = Math.ceil(py) + 5, z = Math.floor(pz) + 24, pos = `${x} ${y} ${z}`;
  await fixture.command(`fill ${x-2} ${y} ${z-3} ${x+2} ${y+3} ${z+3} minecraft:air`);
  await fixture.command(`fill ${x-2} ${y-1} ${z-3} ${x+2} ${y-1} ${z+3} minecraft:stone`);
  await fixture.command(`setblock ${pos} ae2:interface`); await fixture.command('clear @s');
  const card = 'ae2:memory_card[ae2:exported_settings_source={translate:"block.ae2.interface"},ae2:exported_config_inv=[{"#t":"ae2:f",id:"minecraft:water","#":10000L},{"#t":"ae2:i",id:"minecraft:diamond_sword","#":20L},{"#t":"ae2:i",id:"minecraft:diamond","#":20L}]]';
  await fixture.replace('hotbar.0', card); await fixture.select(0, 'ae2:memory_card');
  await fixture.command(`tp @s ${x+.5} ${y} ${z+2.5}`); await fixture.raw(`interact ${pos} south`);
  const cfg = () => fixture.command(`data get block ${pos} ae2_lab_config`);
  await until(cfg, r => /4000L/.test(text(r)) && text(r).includes('minecraft:diamond_sword') && text(r).includes('minecraft:diamond'), 'source Interface stock targets');
  const itemEntry = (index: number) => fixture.command(`data get block ${pos} ae2_lab_config.ae2_interface_config[${index}]`);
  assert.match(text(await itemEntry(0)), /minecraft:diamond_sword/); assert.match(text(await itemEntry(0)), /count:\s*1\b/);
  assert.match(text(await itemEntry(1)), /minecraft:diamond/); assert.match(text(await itemEntry(1)), /count:\s*20\b/);
  fixture.mark('a source-only card clamps the fluid target to 4000 mB and item requests to each real item maximum');

  for (let i = 0; i < 5; i++) {
    await fixture.replace('hotbar.1', 'minecraft:water_bucket'); await fixture.replace('hotbar.0', 'minecraft:air');
    await fixture.raw('hotbar 1');
    await until(() => fixture.raw('inventory'), v => !v.held?.item, 'empty selected hand');
    await fixture.raw(`interact ${pos} south`);
    await until(() => fixture.raw('screen'), s => s.slots?.some((slot: any) => slot.slot === 1 && slot.item === 'minecraft:water_bucket'), 'Interface menu with a physical filled bucket');
    await fixture.raw('click-slot 1');
    await until(() => fixture.raw('inventory'), v => v.carried?.item === 'minecraft:water_bucket', 'water bucket on cursor');
    await fixture.raw('click-slot 300');
    const expected = i < 4 ? 'minecraft:bucket' : 'minecraft:water_bucket';
    await until(() => fixture.raw('inventory'), v => v.carried?.item === expected, i < 4 ? 'physical empty bucket returned' : 'fifth filled bucket retained at capacity');
    await fixture.raw('click-slot 1');
    await until(() => fixture.raw('inventory'), v => !v.carried && v.hotbar?.[1]?.item === expected, 'returned bucket in physical inventory');
    await fixture.raw('close');
  }
  const stock = () => fixture.command(`data get block ${pos} ae2_lab_config.ae2_interface_fluids`);
  await until(stock, r => /4000L/.test(text(r)), 'four buckets physically stock the Interface');
  fixture.mark('four physical water buckets fill the Interface and the fifth stays with the player');
  await fixture.restart();
  assert.match(text(await cfg()), /4000L/); assert.match(text(await stock()), /4000L/);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('the source request and 4000 mB physical fluid stock survive restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
