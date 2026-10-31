/** Registered Memory Card components drive real P2P and machine interactions. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('memory-card-components', mode);
const text = (reply: any) => JSON.stringify(reply.chat ?? []);

try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const at = (dx: number) => `${x + dx} ${y} ${z}`;
  const input = at(0), output = at(2), inscriber = at(4);
  await fixture.command(`fill ${x - 2} ${y} ${z - 3} ${x + 6} ${y + 4} ${z + 3} minecraft:air`);
  await fixture.command(`fill ${x - 2} ${y - 1} ${z - 3} ${x + 6} ${y - 1} ${z + 3} minecraft:stone`);
  await fixture.command('clear @s');
  for (const pos of [input, output]) {
    await fixture.command(`setblock ${pos} ae2:cable_bus`);
    await fixture.replace('hotbar.0', 'ae2:fe_p2p_tunnel'); await fixture.select(0, 'ae2:fe_p2p_tunnel');
    assert.equal((await fixture.ae(`grid part ${pos} south`)).attached, true);
  }
  await fixture.command(`setblock ${inscriber} ae2:inscriber`);
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z + 1.5}`);
  await fixture.replace('hotbar.0', 'ae2:memory_card[ae2:exported_p2p_type="ae2:item_p2p_tunnel",ae2:exported_p2p_frequency=42s]');
  await fixture.select(0, 'ae2:memory_card');
  await fixture.raw(`interact ${output} south`);
  const outputData = await until(() => fixture.command(`data get block ${output} ae2_lab_config.parts`), r => text(r).includes('ae2:item_p2p_tunnel') && text(r).includes('p2p_frequency: 42s'), 'source-only card changes the physical tunnel type and frequency');
  assert.match(text(outputData), /ae2:item_p2p_tunnel/);
  fixture.mark('a source-only P2P card changes a real part and applies its frequency');

  await fixture.replace('hotbar.0', 'ae2:memory_card'); await fixture.select(0, 'ae2:memory_card');
  await fixture.command(`tp @s ${x + 0.5} ${y} ${z + 1.5}`);
  await fixture.raw('key shift down');
  try { await fixture.raw(`interact ${input} south`); } finally { await fixture.raw('key shift up'); }
  await until(() => fixture.command('data get entity @s SelectedItem'), r => {
    const item = text(r);
    return item.includes('ae2:exported_p2p_type') && item.includes('ae2:exported_p2p_frequency') && item.includes('ae2:memory_card_colors');
  }, 'physical P2P save emits the registered components');
  fixture.mark('saving a physical P2P card emits source type, frequency and LED components');

  const configured = 'ae2:memory_card[ae2:exported_settings_source={translate:"block.ae2.inscriber"},ae2:exported_settings={auto_export:"YES",inscriber_separate_sides:"NO",inscriber_input_capacity:"SIXTY_FOUR"},ae2:exported_upgrades=[]]';
  await fixture.replace('hotbar.0', configured); await fixture.select(0, 'ae2:memory_card');
  await fixture.command(`tp @s ${x + 4.5} ${y} ${z + 1.5}`);
  await fixture.raw(`interact ${inscriber} south`);
  await until(() => fixture.command(`data get block ${inscriber} ae2_lab_config`), r => /auto_export.*YES/.test(text(r)), 'source-only Inscriber settings apply');
  await fixture.raw('key shift down');
  try { await fixture.raw(`interact ${inscriber} south`); } finally { await fixture.raw('key shift up'); }
  await until(() => fixture.command('data get entity @s SelectedItem'), r => text(r).includes('ae2:exported_settings') && text(r).includes('ae2:exported_upgrades'), 'machine save emits source settings and upgrade components');
  await fixture.restart();
  await until(() => fixture.command(`data get block ${inscriber} ae2_lab_config`), r => /auto_export.*YES/.test(text(r)), 'source settings survive restart');
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('source-only Inscriber settings import, re-export and survive restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
