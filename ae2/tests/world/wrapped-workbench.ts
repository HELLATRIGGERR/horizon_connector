/** Source-configured fluid ghosts and normal bucket filtering preserve physical items. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('wrapped-workbench', mode);
const fluid = 'ae2:wrapped_generic_stack[ae2:wrapped_stack={"#t":"ae2:f",id:"minecraft:water",components:{"minecraft:custom_data":{variant:"warm"}},"#":1250L}]';
const item = 'ae2:wrapped_generic_stack[ae2:wrapped_stack={"#t":"ae2:i",id:"minecraft:diamond","#":4L}]';
const cell = 'ae2:fluid_storage_cell_1k[ae2:storage_cell_config_inv=[{"#t":"ae2:f",id:"minecraft:water",components:{"minecraft:custom_data":{variant:"warm"}},"#":0L}]]';
const slot = (view: any, index: number) => view.slots?.find((entry: any) => entry.slot === index);
try {
  const state = await fixture.start(); const [px, py, pz] = state.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`; const bench = at(0);
  await fixture.command('clear @s');
  await fixture.command(`fill ${at(-3, 0, -3)} ${at(3, 4, 3)} minecraft:air`);
  await fixture.command(`fill ${at(-3, -1, -3)} ${at(3, -1, 3)} minecraft:stone`);
  await fixture.command(`setblock ${bench} ae2:cell_workbench`);
  await fixture.replace('hotbar.0', cell);
  await fixture.replace('hotbar.1', fluid); await fixture.replace('hotbar.2', item);
  await fixture.replace('hotbar.3', 'minecraft:water_bucket'); await fixture.replace('hotbar.4', 'minecraft:diamond');
  await fixture.replace('hotbar.8', 'minecraft:blaze_rod'); await fixture.select(8, 'minecraft:blaze_rod');
  await fixture.command(`tp @s ${x + 0.5} ${y} ${z - 2.5}`); await fixture.raw(`interact ${bench} north`);
  await until(() => fixture.raw('screen'), view => slot(view, 300), 'Workbench opens');
  await fixture.raw('click-slot 0');
  await until(() => fixture.raw('inventory'), view => view.carried?.item === 'ae2:fluid_storage_cell_1k', 'physical fluid cell on cursor');
  await fixture.raw('click-slot 300');
  const loaded = await until(() => fixture.raw('screen'), view => slot(view, 300)?.item === 'ae2:fluid_storage_cell_1k' && /variant/.test(String(slot(view, 305)?.components ?? '')), 'source warm-water filter loads in Workbench');
  assert.match(String(slot(loaded, 305)?.components ?? ''), /minecraft:water/);

  // AEBaseScreen.slotClicked refuses CLONE of wrapped stacks, even in creative.
  await fixture.raw('click slot-1 middle'); await fixture.command('data get entity @s SelectedItem');
  const afterClone = await fixture.raw('inventory');
  assert.equal(afterClone.carried, null, 'Workbench does not clone a wrapped stack');
  assert.equal(afterClone.hotbar?.[1]?.item, 'ae2:wrapped_generic_stack', 'the real wrapped stack stays in its slot');
  await fixture.raw('click slot-4 middle'); await fixture.command('data get entity @s SelectedItem');
  const ordinaryClone = await until(() => fixture.raw('inventory'), view => view.carried?.item === 'minecraft:diamond', 'ordinary creative clone remains available');
  assert.equal(ordinaryClone.carried?.count, 64); assert.equal(ordinaryClone.hotbar?.[4]?.count, 1);
  await fixture.raw('click-slot 5'); await fixture.command('data get entity @s SelectedItem');
  await until(() => fixture.raw('inventory'), view => !view.carried && view.hotbar?.[5]?.item === 'minecraft:diamond' && view.hotbar?.[5]?.count === 64, 'ordinary clone is stored');

  // A source-valid EMPTY_ITEM action sets a fluid ghost from the filled bucket
  // without consuming the bucket or changing the component-bearing first key.
  // Send the ghost action immediately after pickup: a cursor-only revision
  // must not make an unchanged filter slot reject a normal fast interaction.
  await fixture.raw('click-slot 3');
  await fixture.raw('click-slot 306 right');
  let configured: any;
  try {
    configured = await until(() => fixture.raw('screen'), view => slot(view, 306)?.item === 'ae2:wrapped_generic_stack' && view.carried?.item === 'minecraft:water_bucket', 'bucket sets a fluid ghost without being consumed');
  } catch (failure) {
    const before = await fixture.ae(`workbench status ${bench}`);
    await fixture.raw('click-slot 306 right');
    let second = false;
    try { await until(() => fixture.raw('screen'), view => slot(view, 306)?.item === 'ae2:wrapped_generic_stack' && view.carried?.item === 'minecraft:water_bucket', 'diagnostic second bucket action', 5000); second = true; } catch {}
    console.error('WORKBENCH_BUCKET_DIAGNOSTIC', JSON.stringify({ before, second_click_succeeded: second }));
    throw failure;
  }
  assert.match(String(slot(configured, 306)?.components ?? ''), /minecraft:water/);
  // Left click treats the bucket as an item; only EMPTY_ITEM on right click
  // may copy its contained fluid into a fluid-only configuration slot.
  await fixture.raw('click-slot 308');
  await fixture.command('data get entity @s SelectedItem');
  const afterLeft = await fixture.ae(`workbench status ${bench}`);
  assert(!afterLeft.filters.some((entry: any) => entry.index === 3), 'server rejects a filled bucket as an item filter');
  const rejectedBucket = await until(() => fixture.raw('screen'), view => !slot(view, 308)?.item && view.carried?.item === 'minecraft:water_bucket', 'left-click bucket rejected by fluid filter');
  assert(!slot(rejectedBucket, 308)?.item); assert.equal(rejectedBucket.carried?.item, 'minecraft:water_bucket');
  await fixture.raw('click-slot 3'); await fixture.command('data get entity @s SelectedItem');
  await until(() => fixture.raw('inventory'), view => !view.carried && view.hotbar?.[3]?.item === 'minecraft:water_bucket', 'water bucket returned to its slot');

  await fixture.raw('click-slot 4');
  await fixture.command('data get entity @s SelectedItem');
  await until(() => fixture.raw('inventory'), view => view.carried?.item === 'minecraft:diamond', 'real diamond on cursor');
  await fixture.raw('click-slot 307');
  const rejected = await until(() => fixture.raw('screen'), view => !slot(view, 307)?.item && view.carried?.item === 'minecraft:diamond', 'server rejects item ghost in fluid cell');
  assert(!slot(rejected, 307)?.item); assert.equal(rejected.carried?.item, 'minecraft:diamond');
  await fixture.raw('click-slot 4'); await fixture.command('data get entity @s SelectedItem');
  await until(() => fixture.raw('inventory'), view => !view.carried && view.hotbar?.[4]?.item === 'minecraft:diamond', 'diamond returned to its slot');
  await fixture.raw('close');
  const saved = await fixture.ae(`workbench status ${bench}`);
  assert.deepEqual(saved.filters.map((entry: any) => [entry.index, entry.fluid, entry.item]), [[0, 'minecraft:water', null], [1, 'minecraft:water', null]], 'only the two accepted fluid filters are stored');
  const raw = (await fixture.command(`data get block ${bench}`)).chat?.join('\n') ?? '';
  assert.match(raw, /ae2:storage_cell_config_inv/); assert.match(raw, /variant/);
  fixture.mark('source filter components and a bucket-set fluid ghost persist without consuming physical items');

  await fixture.restart(); await fixture.command(`tp @s ${x + 0.5} ${y} ${z - 2.5}`); await fixture.raw(`interact ${bench} north`);
  await until(() => fixture.raw('screen'), view => /variant/.test(String(slot(view, 305)?.components ?? '')) && /minecraft:water/.test(String(slot(view, 306)?.components ?? '')), 'both exact fluid ghosts survive restart');
  const inventory = await fixture.raw('inventory');
  assert.equal(inventory.hotbar?.[1]?.item, 'ae2:wrapped_generic_stack'); assert.equal(inventory.hotbar?.[2]?.item, 'ae2:wrapped_generic_stack');
  assert.equal(inventory.hotbar?.[3]?.item, 'minecraft:water_bucket'); assert.equal(inventory.hotbar?.[4]?.item, 'minecraft:diamond');
  assert.equal(inventory.hotbar?.[5]?.count, 64);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('wrapped items, water bucket, diamond and both filters survive restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
