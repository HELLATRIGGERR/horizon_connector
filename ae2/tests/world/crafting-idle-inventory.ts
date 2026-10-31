/** Source CPU finishes and independently retains unstoreable inventory. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('crafting-idle-inventory', mode);
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
  const cpuTitle = async (blocked:boolean) => {
    await fixture.replace('hotbar.8','minecraft:blaze_rod');await fixture.select(8,'minecraft:blaze_rod');
    await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`);await fixture.raw(`interact ${terminal} north`);
    await until(() => fixture.raw('screen'),view=>JSON.stringify(view.title??'').includes('AE2 ME Terminal'),'terminal for idle CPU status');
    await fixture.raw('click-slot 349');
    const screen=await until(() => fixture.raw('screen'),view=>{
      const title=JSON.stringify(view.title??'');
      return title.includes('AE2 Crafting Status') && title.includes("Can't Store Contents!")===blocked;
    },blocked?'blocked contents warning':'cleared contents warning',30000);
    if(blocked) assert(screen.title?.some((span:any)=>span.text?.includes("Can't Store Contents!") && span.color==='#FF5555'), 'source warning is red');
    await fixture.raw('close');
  };
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

  // A processing target may return outputs from elsewhere. These are actual
  // player-held outputs (a bucket and a diamond), offered to the network;
  // they are not inserted directly into the CPU's saved state.
  await fixture.replace('hotbar.0', 'minecraft:water_bucket'); await fixture.select(0, 'minecraft:water_bucket');
  assert.equal((await fixture.ae(`fluid grid insert ${drive}`)).inserted_mb, 1000);
  const cpuData = async () => (await fixture.command(`data get block ${cpu}`)).chat?.join('\n') ?? '';
  assert.match(await cpuData(), /ae2_cpu_fluid_escrow/, 'the offered water is held by the CPU');
  await fixture.replace('hotbar.1', 'minecraft:diamond'); await fixture.select(1, 'minecraft:diamond');
  const eventsBefore=(await fixture.raw('ae2-job-ui')).events.length;
  assert.equal((await fixture.ae(`grid deposit ${drive}`)).inserted, 1);
  await until(ready, view => count(view, 'minecraft:diamond') === 1, 'the item output is returned independently', 90000);
  const idle=await until(cpuData, text => text.includes('ae2_cpu_idle_inventory') && !text.includes('ae2_recursive_job'), 'finished CPU retains its blocked fluid', 30000);
  assert.match(idle,/minecraft:water/);assert.match(idle,/1000L/);
  const finished=await until(() => fixture.raw('ae2-job-ui'), view => view.events.some((event: any) => event.status==='finished'), 'finished owner lifecycle', 30000);
  assert(finished.events.length>=eventsBefore);
  assert(!((await fixture.ae(`fluid grid status ${drive}`)).entries??[]).some((entry:any)=>entry.fluid==='minecraft:water'));
  await cpuTitle(true);
  await fixture.restart();await ready();
  const restored=await cpuData();assert.match(restored,/ae2_cpu_idle_inventory/);assert.match(restored,/minecraft:water/);assert.match(restored,/1000L/);
  assert.equal(count(await ready(),'minecraft:diamond'),1);
  fixture.mark('a finished job stores its item and retains 1,000 mB of blocked idle fluid through restart');

  // Source allows a new job to use the CPU even while idle leftovers exist.
  // Cancel while its input is at the real Condenser; the earlier water must
  // remain owned by the CPU and the already-dispatched stone stays consumed.
  await fixture.replace('hotbar.0','minecraft:stone');await fixture.select(0,'minecraft:stone');
  assert.equal((await fixture.ae(`grid deposit ${drive}`)).inserted,1);
  await fixture.replace('hotbar.8','minecraft:blaze_rod');await fixture.select(8,'minecraft:blaze_rod');
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`);await fixture.raw(`interact ${terminal} north`);
  const again=await until(() => fixture.raw('screen'),view => view.slots?.some((slot:any)=>slot.slot>=300&&slot.slot<345&&slot.item==='minecraft:diamond'),'same output can be crafted again');
  await fixture.raw(`click-slot ${again.slots.find((slot:any)=>slot.slot>=300&&slot.slot<345&&slot.item==='minecraft:diamond').slot} shift`);
  await until(() => fixture.raw('screen'),view=>JSON.stringify(view.title??'').includes('AE2 Craft Amount'),'second amount');await fixture.raw('click-slot 302');
  const second=await until(() => fixture.raw('screen'),view=>JSON.stringify(view.title??'').includes('AE2 Craft Confirm')&&/bytes|Missing materials|craft planning:/.test(JSON.stringify(view)),'second confirmation');
  assert.doesNotMatch(JSON.stringify(second),/Missing materials|craft planning:/);
  const traceBefore=(await fixture.raw('ae2-job-ui')).events.length;
  await fixture.raw('click-slot 302');await fixture.raw('close');
  await until(() => fixture.ae(`condenser status ${condenser}`),view=>view.stored===2,'second physical input is accepted',90000);
  const active=await cpuData();assert.match(active,/ae2_recursive_job/);assert.match(active,/ae2_cpu_fluid_escrow/);assert.match(active,/1000L/);
  await fixture.raw(`interact ${terminal} north`);
  await until(() => fixture.raw('screen'),view=>JSON.stringify(view.title??'').includes('AE2 ME Terminal'),'terminal for second CPU status');
  await fixture.raw('click-slot 349');
  await until(() => fixture.raw('screen'),view=>JSON.stringify(view.title??'').includes('AE2 Crafting Status'),'running CPU status');
  await fixture.raw('click-slot 303');
  await until(() => fixture.raw('ae2-job-ui'),view=>view.events.slice(traceBefore).some((event:any)=>event.status==='cancelled'),'second owner cancellation');
  await fixture.raw('close');
  const canceled=await until(cpuData,text=>text.includes('ae2_cpu_idle_inventory')&&!text.includes('ae2_recursive_job'),'cancelled CPU is idle while retaining prior fluid');
  assert.match(canceled,/1000L/);assert.equal(count(await ready(),'minecraft:diamond'),1);
  assert.equal((await fixture.ae(`condenser status ${condenser}`)).stored,2,'cancel does not recreate dispatched input');
  fixture.mark('a new job reuses the CPU and immediate cancellation preserves the old idle fluid without reclaiming machine input');

  await fixture.command(`item replace block ${drive} container.1 with ae2:fluid_storage_cell_1k`);
  const fluids=()=>fixture.ae(`fluid grid status ${drive}`);
  await until(fluids,view=>view.entries?.some((entry:any)=>entry.fluid==='minecraft:water'&&entry.amount_mb===1000),'idle water retries into newly available storage',90000);
  await until(cpuData,text=>!text.includes('ae2_cpu_idle_inventory')&&!text.includes('ae2_cpu_fluid_escrow'),'empty idle CPU persists without escrow');
  await cpuTitle(false);
  await fixture.restart();assert.equal(count(await ready(),'minecraft:diamond'),1);
  assert.equal((await fluids()).entries?.find((entry:any)=>entry.fluid==='minecraft:water')?.amount_mb,1000);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('idle retry drains exactly the retained water into a physical cell and survives restart');fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
