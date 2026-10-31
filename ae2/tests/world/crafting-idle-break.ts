/** Breaking a finished CPU preserves idle item custody and source fluid voiding. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('crafting-idle-break', mode);
const stone = 'ae2:item_storage_cell_1k[ae2:storage_cell_inv=[{"#t":"ae2:i",id:"minecraft:stone","#":1L}]]';
const pattern = 'ae2:processing_pattern[ae2:encoded_processing_pattern={sparseInputs:[{"#t":"ae2:i",id:"minecraft:stone","#":1L}],sparseOutputs:[{"#t":"ae2:i",id:"minecraft:diamond","#":1L},{"#t":"ae2:f",id:"minecraft:water","#":1000L}]}]';
const count = (status: any, item: string) => status.items?.find((entry: any) => entry.item === item)?.count ?? 0;
try {
  const state = await fixture.start();
  const [px, py, pz] = state.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const energy = at(0), drive = at(1), terminal = at(2), cpu = at(2, 1), provider = at(3), condenser = at(3, 0, -1);
  await fixture.command(`tp @s ${x + .5} ${y} ${z + .5}`);
  for (const dx of [-2, 6]) for (const dz of [-3, 3]) {
    await until(async () => (await fixture.command(`execute if loaded ${at(dx, 0, dz)}`)).chat ?? [],
      lines => lines.includes('Test passed'), 'loaded build corner', 30000);
  }
  await fixture.command('clear @s');
  await fixture.command(`fill ${at(-2, 0, -3)} ${at(6, 5, 3)} minecraft:air`);
  await fixture.command(`fill ${at(-2, -1, -3)} ${at(6, -1, 3)} minecraft:stone`);
  for (const [pos, id] of [[energy, 'energy_cell'], [drive, 'drive'], [terminal, 'cable_bus'],
    [cpu, '64k_crafting_storage'], [provider, 'pattern_provider'], [condenser, 'condenser']] as const) {
    await fixture.command(`setblock ${pos} ae2:${id}`);
  }
  await fixture.command(`data merge block ${energy} {ae2_lab_energy:100000.0d}`);
  await fixture.command(`item replace block ${drive} container.0 with ${stone}`);
  await fixture.command(`item replace block ${condenser} container.1 with ae2:cell_component_1k`);
  assert.equal((await fixture.ae(`condenser output ${condenser} matter_balls`)).output, 'matter_balls');
  await fixture.replace('hotbar.0', 'ae2:terminal'); await fixture.select(0, 'ae2:terminal');
  assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached, true);
  const ready = () => until(() => fixture.ae(`grid status ${drive}`), view => view.online && view.powered, 'finite-power processing network', 30000);
  await ready(); await until(() => fixture.ae(`grid status ${provider}`), view => view.online && view.powered, 'online provider', 30000);
  assert.equal(count(await ready(), 'minecraft:stone'), 1);
  await fixture.replace('hotbar.0', pattern); await fixture.replace('hotbar.8', 'minecraft:blaze_rod'); await fixture.select(8, 'minecraft:blaze_rod');
  await fixture.command(`tp @s ${x + 5.5} ${y} ${z + .5}`); await fixture.raw(`interact ${provider} east`);
  const menu = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot < 36 && slot.item === 'ae2:processing_pattern'), 'physical processing pattern in inventory');
  await fixture.raw(`click-slot ${menu.slots.find((slot: any) => slot.slot < 36 && slot.item === 'ae2:processing_pattern').slot} shift`);
  await until(() => fixture.raw('inventory'), view => !view.counts?.['ae2:processing_pattern'], 'pattern installed'); await fixture.raw('close');
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`); await fixture.raw(`interact ${terminal} north`);
  const screen = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:diamond'), 'processing result is craftable');
  await fixture.raw(`click-slot ${screen.slots.find((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:diamond').slot} shift`);
  await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Craft Amount'), 'amount');
  await fixture.raw('click-slot 302');
  const confirmation = await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Craft Confirm') && /bytes|Missing materials|craft planning:/.test(JSON.stringify(view)), 'processing confirmation');
  assert.doesNotMatch(JSON.stringify(confirmation), /Missing materials|craft planning:/);
  await fixture.raw('click-slot 302'); await fixture.raw('close');
  await until(() => fixture.ae(`condenser status ${condenser}`), view => view.stored === 1, 'the provider physically sends its stone to the Condenser', 90000);
  assert.equal(count(await ready(), 'minecraft:stone'), 0);
  // Both output channels are blocked after the input has been consumed.
  await fixture.command(`item replace block ${drive} container.0 with minecraft:air`);

  // A processing target may return outputs from elsewhere. These are actual
  // player-held outputs (a bucket and a diamond), offered to the network;
  // they are not inserted directly into the CPU's saved state.
  await fixture.replace('hotbar.0', 'minecraft:water_bucket'); await fixture.select(0, 'minecraft:water_bucket');
  assert.equal((await fixture.ae(`fluid grid insert ${drive}`)).inserted_mb, 1000);
  const cpuData = async () => (await fixture.command(`data get block ${cpu}`)).chat?.join('\n') ?? '';
  assert.match(await cpuData(), /ae2_cpu_fluid_escrow/, 'the offered water is held by the CPU');
  await fixture.replace('hotbar.1','minecraft:diamond');await fixture.select(1,'minecraft:diamond');
  assert.equal((await fixture.ae(`grid deposit ${drive}`)).inserted,1);
  const idle=await until(cpuData,text=>text.includes('ae2_cpu_idle_inventory')&&!text.includes('ae2_recursive_job'),'finished CPU retains both blocked outputs',90000);
  assert.match(idle,/minecraft:diamond/);assert.match(idle,/minecraft:water/);assert.match(idle,/1000L/);
  await fixture.restart();await ready();
  const reloaded=await cpuData();assert.match(reloaded,/ae2_cpu_idle_inventory/);assert.match(reloaded,/minecraft:diamond/);assert.match(reloaded,/1000L/);
  fixture.mark('both real blocked outputs remain in the finished CPU across restart');
  await fixture.command(`tp @s ${x + 12.5} ${y} ${z + 4.5}`);
  await fixture.command(`setblock ${cpu} minecraft:air`);
  await until(()=>fixture.command(`execute if block ${cpu} minecraft:air`),reply=>reply.chat?.includes('Test passed'),'idle CPU removed',30000);
  const diamonds=async()=>(await fixture.raw('entities 32 minecraft:item')).entities.filter((entity:any)=>entity.item?.item==='minecraft:diamond');
  const drops=await until(diamonds,items=>items.length>0,'idle item becomes a physical drop',30000);
  assert.equal(drops.reduce((sum:number,entity:any)=>sum+entity.item.count,0),1,'one exact item leaves the CPU');
  assert(drops.every((entity:any)=>entity.item.count>0&&entity.item.count<=64));
  assert(!((await fixture.ae(`fluid grid status ${drive}`)).entries??[]).some((entry:any)=>entry.fluid==='minecraft:water'),'upstream AEFluidKey.addDrops voids the unstoreable fluid');
  assert(!(await fixture.raw('ae2-job-ui')).events.some((event:any)=>event.status==='cancelled'),'breaking an already finished CPU does not cancel a job');
  await fixture.restart();
  const persisted=await until(diamonds,items=>items.length>0,'physical drop survives restart',30000);
  assert.equal(persisted.reduce((sum:number,entity:any)=>sum+entity.item.count,0),1);
  assert(!((await fixture.ae(`fluid grid status ${drive}`)).entries??[]).some((entry:any)=>entry.fluid==='minecraft:water'));
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('breaking idle CPU drops its item once and voids its unstoreable fluid, matching source drop behavior');fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
