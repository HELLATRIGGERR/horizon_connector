/** Provider block/part menu, return custody and stale owners in both paths. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('provider-session', mode);
const entry = (screen: any, slot: number) => screen.slots?.find((value: any) => value.slot === slot);
const text = (reply: any) => (reply.chat ?? []).join('\n');
// The agent's inventory counts already include the carried stack.
const owned = (inventory: any, id: string) => inventory.counts?.[id] ?? 0;

try {
  const state = await fixture.start();
  const [px, py, pz] = state.player.position;
  const x = Math.floor(px), y = Math.ceil(py) + 3, z = Math.floor(pz);
  const at = (dx: number) => `${x + dx} ${y} ${z}`;
  const block = at(0), donor = at(1), part = at(3);
  await fixture.command(`fill ${x - 2} ${y - 1} ${z - 2} ${x + 5} ${y - 1} ${z + 3} minecraft:bedrock`);
  await fixture.command(`fill ${x - 2} ${y} ${z - 2} ${x + 5} ${y + 5} ${z + 3} minecraft:air`);
  await fixture.command(`setblock ${block} ae2:pattern_provider`);
  await fixture.command(`setblock ${part} ae2:cable_bus`);
  await fixture.command(`setblock ${donor} ae2:cable_bus`);
  await fixture.command('clear @s');
  await fixture.replace('hotbar.0', 'ae2:cable_pattern_provider');
  await fixture.select(0, 'ae2:cable_pattern_provider');
  assert.equal((await fixture.ae(`grid part ${part} south`)).attached, true);
  assert.equal((await fixture.ae(`grid part ${donor} south`)).attached, true);
  await fixture.replace('hotbar.8', 'minecraft:stick');

  const open = async (target: string, dx: number) => {
    await fixture.select(8, 'minecraft:stick');
    await fixture.command(`tp @s ${x + dx + 0.5} ${y} ${z + 2.5}`);
    await fixture.raw(`interact ${target} south`);
    const menu = await until(() => fixture.raw('screen'), view => /Pattern Provider/i.test(JSON.stringify(view.title ?? '')) && entry(view, 300), 'physical Pattern Provider menu', 8000);
    await Bun.sleep(300);
    assert.match(JSON.stringify((await fixture.raw('screen')).title ?? ''), /Pattern Provider/i, 'menu remains open across ticks');
    return menu;
  };
  const ownerData = (target: string, isPart: boolean) => fixture.command(`data get block ${target}${isPart ? ' ae2_lab_config.parts[{face:"south"}]' : ''}`);
  const putPattern = async (target: string, dx: number) => {
    await fixture.replace('hotbar.0', 'ae2:blank_pattern');
    await fixture.select(0, 'ae2:blank_pattern');
    assert.equal((await fixture.ae('crafting encode minecraft:oak_log - - - - - - - -')).encoded, true);
    const menu = await open(target, dx);
    const player = menu.slots.find((slot: any) => slot.item === 'ae2:crafting_pattern' && slot.slot < 36);
    assert(player, 'encoded physical pattern is in player inventory');
    await fixture.raw(`click-slot ${player.slot} shift`);
    await until(() => fixture.raw('screen'), view => entry(view, 300)?.item === 'ae2:crafting_pattern', 'pattern inserted in owner slot');
    assert.equal((await fixture.raw('inventory')).counts?.['ae2:crafting_pattern'] ?? 0, 0);
    await fixture.raw('close');
  };
  const putReturn = async (target: string, dx: number, item: string, count: number) => {
    await fixture.replace('hotbar.0', item, count);
    const menu = await open(target, dx);
    const player = menu.slots.find((slot: any) => slot.item === item && slot.slot < 36);
    assert(player, 'return item begins in player inventory');
    await fixture.raw(`click-slot ${player.slot} shift`);
    await until(() => fixture.raw('screen'), view => entry(view, 309)?.item === item && entry(view, 309)?.count === count, 'offline provider return slot');
    assert.equal((await fixture.raw('inventory')).counts?.[item] ?? 0, 0);
    await fixture.raw('close');
  };
  const setPriority = async (target: string, dx: number, isPart: boolean) => {
    await open(target, dx);
    await fixture.raw('click-slot 326');
    await until(() => fixture.raw('screen'), view => String(entry(view, 300)?.components ?? '').includes('Priority +1'), 'priority editor');
    await fixture.raw('type =2*3');
    await until(() => ownerData(target, isPart), reply => /\bpriority:\s*6\b/.test(text(reply)), 'typed expression reaches physical provider');
    await fixture.raw('ui-key Enter');
    await until(() => fixture.raw('screen'), view => /Pattern Provider/i.test(JSON.stringify(view.title ?? '')) && entry(view, 300)?.item === 'ae2:crafting_pattern', 'Enter returns to provider');
    await fixture.raw('close');
  };

  await putPattern(block, 0);
  await putPattern(part, 3);
  fixture.mark('block and mounted part open and accept one real encoded pattern each');
  await putReturn(block, 0, 'minecraft:diamond', 7);
  await putReturn(part, 3, 'minecraft:iron_ingot', 5);
  fixture.mark('offline block and part return slots accept physical item stacks');
  await setPriority(block, 0, false);
  await setPriority(part, 3, true);
  fixture.mark('block and part priority editors apply an expression and return on Enter');

  await fixture.restart();
  for (const [target, dx, isPart, item, count] of [[block, 0, false, 'minecraft:diamond', 7], [part, 3, true, 'minecraft:iron_ingot', 5]] as const) {
    const menu = await open(target, dx);
    assert.equal(entry(menu, 300)?.item, 'ae2:crafting_pattern');
    assert.equal(entry(menu, 309)?.item, item);
    assert.equal(entry(menu, 309)?.count, count);
    assert.match(text(await ownerData(target, isPart)), /\bpriority:\s*6\b/);
    await fixture.raw('click-slot 300 shift');
    await until(() => fixture.raw('screen'), view => !entry(view, 300)?.item, 'pattern removed from owner');
    await fixture.raw('click-slot 309 shift');
    await until(() => fixture.raw('inventory'), view => view.counts?.[item] === count, 'all real return items back in player inventory');
    await fixture.raw('close');
  }
  const recovered = await fixture.raw('inventory');
  assert.equal(recovered.counts?.['ae2:crafting_pattern'], 2);
  assert.equal(recovered.counts?.['minecraft:diamond'], 7);
  assert.equal(recovered.counts?.['minecraft:iron_ingot'], 5);
  fixture.mark('restart preserves both owners, priority and return contents; retrieval conserves exact counts');

  // Custody checks must not rely on creative item creation or resynchronization.
  await fixture.command('gamemode survival @s');
  await until(() => fixture.raw('status'), view => view.creative === false, 'survival mode');
  await fixture.command('clear @s');
  await fixture.replace('hotbar.8', 'minecraft:stick');
  await fixture.replace('hotbar.0', 'minecraft:diamond');
  await open(block, 0);
  await fixture.raw('click-slot 0');
  await until(() => fixture.raw('inventory'), view => view.carried?.item === 'minecraft:diamond', 'cursor owns stale-window sentinel');
  await fixture.command(`setblock ${block} minecraft:air`);
  await fixture.command(`setblock ${block} ae2:pattern_provider`);
  await until(() => fixture.raw('screen'), view => view.screen === 'playing', 'replaced block closes its old session');
  assert.equal(owned(await fixture.raw('inventory'), 'minecraft:diamond'), 1);
  assert(!text(await ownerData(block, false)).includes('minecraft:diamond'));
  fixture.mark('replacing the host closes the old session without transferring the cursor to its replacement');

  await fixture.command('clear @s');
  await fixture.replace('hotbar.8', 'minecraft:stick');
  await fixture.replace('hotbar.0', 'minecraft:emerald');
  await open(part, 3);
  await fixture.raw('click-slot 0');
  await until(() => fixture.raw('inventory'), view => view.carried?.item === 'minecraft:emerald', 'cursor owns part replacement sentinel');
  // The donor face has a distinct identity; the command reloads the host too.
  // This fixture does not represent a concurrent survival removal.
  await fixture.command(`data modify block ${part} ae2_lab_config.parts[{face:"south"}] set from block ${donor} ae2_lab_config.parts[{face:"south"}]`);
  await until(() => fixture.raw('screen'), view => view.screen === 'playing', 'replaced part owner closes its old session');
  assert.equal(owned(await fixture.raw('inventory'), 'minecraft:emerald'), 1);
  assert(!text(await ownerData(part, true)).includes('minecraft:emerald'));
  fixture.mark('replacing the mounted face closes its session and preserves the cursor item');

  await fixture.command('clear @s');
  await fixture.replace('hotbar.8', 'minecraft:stick');
  await fixture.replace('hotbar.0', 'minecraft:gold_ingot');
  await open(block, 0);
  await fixture.raw('click-slot 0');
  await until(() => fixture.raw('inventory'), view => view.carried?.item === 'minecraft:gold_ingot', 'cursor owns dimension-change sentinel');
  await fixture.command('execute in minecraft:the_nether run tp @s 0.5 100 0.5');
  await until(() => fixture.raw('screen'), view => view.screen === 'playing', 'dimension change closes the old provider session');
  const afterTravel = await fixture.raw('inventory');
  assert.equal(afterTravel.carried, null, 'dimension transition clears the closed menu cursor');
  assert.equal(owned(afterTravel, 'minecraft:gold_ingot'), 1);
  const serverInventory = text(await fixture.command('data get entity @s Inventory'));
  const serverGold = serverInventory.match(/\{[^{}]*id:\s*"minecraft:gold_ingot"[^{}]*\}/g) ?? [];
  assert.equal(serverGold.length, 1, 'one physical server inventory stack after travel');
  assert.match(serverGold[0], /\bcount:\s*1\b/, 'the server stack contains exactly one ingot');
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('dimension change closes the old owner and preserves the cursor item');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
