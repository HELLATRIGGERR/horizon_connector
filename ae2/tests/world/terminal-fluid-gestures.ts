/** Wired and wireless ME terminals transact real buckets using source click gestures. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('terminal-fluid-gestures', mode);
const entry = (view: any) => view.slots?.find((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:water_bucket');
try {
  const start = await fixture.start(), [px, py, pz] = start.player.position;
  const x=Math.floor(px)+24, y=Math.ceil(py)+5, z=Math.floor(pz)+24;
  const at=(dx:number, dy=0, dz=0)=>`${x+dx} ${y+dy} ${z+dz}`, drive=at(1), terminal=at(2), access=at(-1);
  await fixture.command(`tp @s ${x+2.5} ${y} ${z-2.5}`);
  for(const dx of [-3,5]) for(const dz of [-3,3]) await until(() => fixture.raw(`block ${at(dx,0,dz)}`), value => value.loaded, 'terminal fixture chunk loaded', 30000);
  await fixture.command(`fill ${at(-3,0,-3)} ${at(5,4,3)} minecraft:air`);
  await fixture.command(`fill ${at(-3,-1,-3)} ${at(5,-1,3)} minecraft:stone`);
  await fixture.command(`setblock ${at(0)} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${drive} ae2:drive`);
  await fixture.command(`setblock ${terminal} ae2:cable_bus`);
  await fixture.command(`setblock ${access} ae2:wireless_access_point[facing=east]`);
  await fixture.command(`item replace block ${drive} container.0 with ae2:fluid_storage_cell_1k[ae2:storage_cell_inv=[{"#t":"ae2:f",id:"minecraft:water","#":2000L}]]`);
  await fixture.command(`item replace block ${drive} container.1 with ae2:item_storage_cell_1k[ae2:storage_cell_inv=[{"#t":"ae2:i",id:"minecraft:bucket","#":2L}]]`);
  await fixture.command('clear @s'); await fixture.replace('hotbar.0','ae2:terminal'); await fixture.select(0,'ae2:terminal');
  assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached,true);
  await until(() => fixture.ae(`grid status ${drive}`), value => value.online && value.powered, 'powered terminal grid', 30000);
  await fixture.command('gamemode survival @s'); await fixture.replace('hotbar.0','minecraft:bucket'); await fixture.replace('hotbar.8','minecraft:stick');
  await fixture.select(8,'minecraft:stick'); await fixture.command(`tp @s ${x+2.5} ${y} ${z-2.5}`); await fixture.raw(`interact ${terminal} north`);
  const fluidAmount = async () => (await fixture.ae(`fluid grid status ${drive}`)).entries?.find((value:any)=>value.fluid==='minecraft:water')?.amount_mb ?? 0;
  const openWater = () => until(() => fixture.raw('screen'), view => view.screen === 'container' && entry(view), 'terminal water entry', 30000);
  const perform = async (label:string) => {
    let view = await openWater(); await fixture.raw('click-slot 0');
    await until(() => fixture.raw('inventory'), state => state.carried?.item === 'minecraft:bucket', `${label}: empty bucket held`);
    await fixture.raw(`click-slot ${entry(view).slot} right`); await fixture.command(`data get block ${drive}`);
    assert.equal((await fixture.raw('inventory')).carried?.item,'minecraft:bucket',`${label}: right click cannot fill an empty bucket`);
    assert.equal(await fluidAmount(),2000);
    await fixture.raw(`click-slot ${entry(view).slot}`);
    await until(() => fixture.raw('inventory'), state => state.carried?.item === 'minecraft:water_bucket', `${label}: left click fills bucket`);
    assert.equal(await fluidAmount(),1000);
    view=await openWater(); await fixture.raw(`click-slot ${entry(view).slot}`); await fixture.command(`data get block ${drive}`);
    assert.equal((await fixture.raw('inventory')).carried?.item,'minecraft:water_bucket',`${label}: left click does not empty a filled bucket`);
    assert.equal(await fluidAmount(),1000);
    await fixture.raw(`click-slot ${entry(view).slot} right`);
    await until(() => fixture.raw('inventory'), state => state.carried?.item === 'minecraft:bucket', `${label}: right click empties bucket`);
    assert.equal(await fluidAmount(),2000);
    await fixture.raw('click-slot 0'); await until(() => fixture.raw('inventory'), state => !state.carried && state.hotbar?.[0]?.item === 'minecraft:bucket', `${label}: bucket returned once`);
    await fixture.raw('close');
  };
  await perform('wired');
  const wireless=`ae2:wireless_terminal[ae2:wireless_link_target={dimension:"minecraft:overworld",pos:[I;${x-1},${y},${z}]},ae2:stored_energy=10000.0d]`;
  await fixture.replace('hotbar.1',wireless); await fixture.select(1,'ae2:wireless_terminal'); await fixture.raw('look 0 -80'); await fixture.raw('use-item');
  await perform('wireless');
  // With no carried item, source borrows a real empty bucket from ME storage.
  await fixture.replace('hotbar.0','minecraft:air'); await fixture.select(8,'minecraft:stick');
  await fixture.command(`tp @s ${x+2.5} ${y} ${z-2.5}`); await fixture.raw(`interact ${terminal} north`);
  let view=await openWater(); await fixture.raw(`click-slot ${entry(view).slot}`);
  await until(()=>fixture.raw('inventory'),state=>state.carried?.item==='minecraft:water_bucket','stored bucket is filled on an empty-cursor click');
  assert.equal(await fluidAmount(),1000); let grid=await fixture.ae(`grid status ${drive}`);
  assert.equal(grid.items?.find((item:any)=>item.item==='minecraft:bucket')?.count,1,'one physical empty bucket remains in storage');
  view=await openWater(); await fixture.raw(`click-slot ${entry(view).slot} right`);
  await until(()=>fixture.raw('inventory'),state=>state.carried?.item==='minecraft:bucket','borrowed bucket is emptied into the grid');
  await fixture.raw('click-slot 0'); await until(()=>fixture.raw('inventory'),state=>!state.carried&&state.hotbar?.[0]?.item==='minecraft:bucket','borrowed empty bucket returned to player');
  view=await openWater(); await fixture.raw(`click-slot ${entry(view).slot} shift`);
  await until(()=>fixture.raw('inventory'),state=>!state.carried&&state.counts?.['minecraft:water_bucket']===1,'shift-click fills a stored bucket and moves it to the player');
  assert.equal(await fluidAmount(),1000); grid=await fixture.ae(`grid status ${drive}`);
  assert.equal(grid.items?.find((item:any)=>item.item==='minecraft:bucket')?.count??0,0,'both borrowed buckets left storage exactly once');
  await fixture.raw('close'); await fixture.restart(); await until(() => fixture.ae(`grid status ${drive}`), value => value.online && value.powered, 'reloaded fluid grid', 30000);
  assert.equal(await fluidAmount(),1000); const saved=await fixture.raw('inventory'); assert.equal(saved.counts?.['minecraft:bucket'],1); assert.equal(saved.counts?.['minecraft:water_bucket'],1);
  fixture.assertHidden(await fixture.raw('status')); fixture.mark('wired and wireless bucket gestures and storage-borrowed buckets preserve exact fluid and item custody across restart'); fixture.report('passed');
} catch(error) { fixture.report('failed',error); throw error; }
finally { await fixture.close(); }
