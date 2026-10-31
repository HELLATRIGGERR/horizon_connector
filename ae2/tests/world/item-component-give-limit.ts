/** /give uses the physical item's component maximum for both its limit and stacks. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';
const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('item-component-give-limit', mode);
try {
  await fixture.start();
  await fixture.command('clear @s');
  const item = 'minecraft:stone[minecraft:max_stack_size=16,minecraft:custom_data={variant:"give-max"}]';
  const rejected = await fixture.raw(`command give @s ${item} 1601`);
  assert.match(JSON.stringify(rejected), /1600/, 'the source 100-stack command limit uses the component maximum');
  const slots = (view: any) => [...(view.hotbar ?? []), ...(view.main ?? [])]
    .filter((s: any) => s.item === 'minecraft:stone' && s.components?.includes('give-max'));
  assert.equal(slots(await fixture.raw('inventory')).length, 0, 'rejected command gives no items');
  await fixture.command(`give @s ${item} 48`);
  const inventory = await until(() => fixture.raw('inventory'), view =>
    slots(view).reduce((sum: number, stack: any) => sum + stack.count, 0) === 48,
  'component-limited items reach the player');
  assert.equal(slots(inventory).length, 3);
  assert(slots(inventory).every((stack: any) => stack.count === 16 && stack.components.includes('minecraft:max_stack_size')));
  fixture.mark('/give rejects more than 100 component-limited stacks and splits an accepted amount into real max-16 slots');
  await fixture.restart();
  const restored = await fixture.raw('inventory');
  assert.equal(slots(restored).length, 3);
  assert(slots(restored).every((stack: any) => stack.count === 16 && stack.components.includes('minecraft:max_stack_size')));
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('the three component-limited stacks survive restart');
  await fixture.command('clear @s');
  await fixture.command('give @s minecraft:stone[!minecraft:max_stack_size] 4');
  const removedSlots = (view: any) => [...(view.hotbar ?? []), ...(view.main ?? [])]
    .filter((stack: any) => stack.item === 'minecraft:stone');
  const removed = await until(() => fixture.raw('inventory'), view =>
    removedSlots(view).reduce((sum: number, stack: any) => sum + stack.count, 0) === 4,
  'items with a removed max-stack component reach the player');
  assert.equal(removedSlots(removed).length, 4);
  assert(removedSlots(removed).every((stack: any) => stack.count === 1 && stack.components?.includes('!minecraft:max_stack_size')));
  await fixture.restart();
  const restoredRemoved = await fixture.raw('inventory');
  assert.equal(removedSlots(restoredRemoved).length, 4);
  assert(removedSlots(restoredRemoved).every((stack: any) => stack.count === 1 && stack.components?.includes('!minecraft:max_stack_size')));
  fixture.mark('an explicitly removed maximum resolves to one and survives the wire and restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
