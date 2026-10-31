/** The actual item argument registry, source codecs, and physical custody. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('ae2-component-commands', mode);
try {
  await fixture.start();
  await fixture.command('clear @s');
  await fixture.command('give @s ae2:name_press[ae2:name_press_name={text:"Registry",color:"red"}]');
  const first = await until(() => fixture.raw('inventory'), s => s.hotbar?.some((slot: any) => slot?.item === 'ae2:name_press'), 'component-bearing Name Press given');
  const pressSlot = first.hotbar.findIndex((slot: any) => slot?.item === 'ae2:name_press');
  await fixture.select(pressSlot, 'ae2:name_press');
  const press = JSON.stringify(await fixture.command('data get entity @s SelectedItem'));
  assert.match(press, /ae2:name_press_name.*color.*red.*text.*Registry/);
  fixture.mark('/give accepts a real styled Name Press component and retains its payload');

  // Int is intentionally unsuffixed: Codec.LONG must normalize it to Long.
  await fixture.command('item replace entity @s hotbar.1 with ae2:quantum_entangled_singularity[ae2:entangled_singularity_id=42]');
  await fixture.select(1, 'ae2:quantum_entangled_singularity');
  const selected = () => fixture.command('data get entity @s SelectedItem');
  const before = JSON.stringify(await selected());
  assert.match(before, /ae2:entangled_singularity_id.{0,12}42L/);
  fixture.mark('an unsuffixed numeric item argument is normalized to the source Long component');

  for (const bad of [
    'item replace entity @s hotbar.1 with ae2:quantum_entangled_singularity[ae2:entangled_singularity_id="bad"]',
    'item replace entity @s hotbar.1 with ae2:quantum_entangled_singularity[ae2:not_registered=1]',
  ]) {
    const response = await fixture.raw(`command ${bad}`);
    assert(response.ok === false || /invalid|unknown|malformed|failed|expected|number/i.test(JSON.stringify(response)), `command must reject the value: ${JSON.stringify(response)}`);
    assert.equal(JSON.stringify(await selected()), before, 'a rejected component never replaces the physical item');
  }
  fixture.mark('unknown component IDs and invalid source-codec values are rejected without replacing the item');
  await fixture.restart();
  await fixture.select(1, 'ae2:quantum_entangled_singularity');
  assert.match(JSON.stringify(await selected()), /ae2:entangled_singularity_id.{0,12}42L/);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('source components survive player inventory restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
