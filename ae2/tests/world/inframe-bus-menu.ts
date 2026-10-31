/** In-frame bus filter, physical upgrade and face-owner transactions. */
import assert from 'node:assert/strict';
import { WorldFixture, until } from './harness';

const fixture = new WorldFixture('inframe-bus-menu', 'in-frame');
const slot = (view: any, index: number) => view.slots?.find((entry: any) => entry.slot === index);
const chat = (reply: any) => (reply.chat ?? []).join('\n');
try {
  const start=await fixture.start(); const [px,py,pz]=start.player.position;
  const x=Math.floor(px)+24,y=Math.ceil(py)+5,z=Math.floor(pz)+24,bus=`${x} ${y} ${z}`,donor=`${x+3} ${y} ${z}`;
  await fixture.command(`tp @s ${x+1.5} ${y} ${z+2.5}`);
  for(const [dx,dz] of [[-2,-3],[-2,3],[5,-3],[5,3]] as const)
    await until(()=>fixture.raw(`block ${x+dx} ${y} ${z+dz}`),v=>v.loaded,'bus fixture chunk loaded',30000);
  await fixture.command(`fill ${x-2} ${y} ${z-3} ${x+5} ${y+4} ${z+3} minecraft:air`);
  await fixture.command(`fill ${x-2} ${y-1} ${z-3} ${x+5} ${y-1} ${z+3} minecraft:stone`);
  await fixture.command(`setblock ${bus} ae2:cable_bus`);await fixture.command(`setblock ${donor} ae2:cable_bus`);
  await fixture.command('clear @s');await fixture.replace('hotbar.0','ae2:import_bus');await fixture.select(0,'ae2:import_bus');
  assert.equal((await fixture.ae(`grid part ${bus} south`)).attached,true);
  assert.equal((await fixture.ae(`grid part ${donor} south`)).attached,true);
  await fixture.command('gamemode survival @s');await fixture.replace('hotbar.0','minecraft:diamond',4);
  await fixture.replace('hotbar.1','ae2:speed_card',2);await fixture.replace('hotbar.8','minecraft:stick');
  await fixture.replace('weapon.offhand','ae2:fuzzy_card');
  const open=async()=>{await fixture.select(8,'minecraft:stick');await fixture.command(`tp @s ${x+.5} ${y} ${z+2.5}`);await fixture.raw(`interact ${bus} south`);
    await until(()=>fixture.raw('screen'),v=>/AE2 Bus/i.test(JSON.stringify(v.title ?? '')) && slot(v,300),'local bus menu');};
  const drag=async(a:number,b:number,fake=false)=>{const widgets=await fixture.raw('ui');const center=(i:number)=>{const n=widgets.widgets.find((w:any)=>w.id===`slot-${i}`);assert(n);return [n.bounds[0]+8,n.bounds[1]+8];};
    const [ax,ay]=center(a),[bx,by]=center(b);
    // AEBaseScreen.mouseDragged records fake slots on movement, not on press.
    // Move back across the first fake slot so both receive a drag event.
    await fixture.raw(`drag-at ${ax} ${ay} ${bx} ${by}${fake ? ` ${ax} ${ay}` : ''}`);};
  await open();await fixture.raw('click-slot 0');await drag(300,301,true);
  let view=await until(()=>fixture.raw('screen'),v=>slot(v,300)?.item==='minecraft:diamond' && slot(v,301)?.item==='minecraft:diamond' && v.carried?.count===4,'bus fake drag preserves template');
  assert.equal(view.carried.count,4);await fixture.raw('click-slot 0');await fixture.raw('click-slot 342');
  await fixture.raw('click-slot 1');await drag(336,337);
  view=await until(()=>fixture.raw('screen'),v=>slot(v,336)?.item==='ae2:speed_card' && slot(v,337)?.item==='ae2:speed_card' && !v.carried,'physical upgrades distributed once');
  await fixture.raw('key f tap');
  await until(()=>fixture.raw('screen'),v=>slot(v,337)?.item==='ae2:fuzzy_card' && !v.carried,'offhand swaps with physical bus upgrade');
  assert.equal((await fixture.raw('inventory')).offhand?.item,'ae2:speed_card');await fixture.raw('key f tap');
  await until(()=>fixture.raw('screen'),v=>slot(v,337)?.item==='ae2:speed_card' && !v.carried,'second offhand swap restores physical card');
  assert.equal((await fixture.raw('inventory')).offhand?.item,'ae2:fuzzy_card');
  await fixture.raw('close');await fixture.restart();
  const saved=chat(await fixture.command(`data get block ${bus} ae2_lab_config.parts`));assert.match(saved,/minecraft:diamond/);assert.equal((saved.match(/ae2:speed_card/g)??[]).length,2);
  assert.equal((await fixture.raw('inventory')).counts?.['minecraft:diamond'],4);fixture.mark('bus filters, settings and physical upgrades survive restart');
  await open();await fixture.raw('click-slot 0');
  await until(()=>fixture.raw('inventory'),v=>v.carried?.item==='minecraft:diamond' && v.carried.count===4,'real cursor before replacement');
  await fixture.command(`data modify block ${bus} ae2_lab_config.parts[{face:"south"}] set from block ${donor} ae2_lab_config.parts[{face:"south"}]`);
  await until(()=>fixture.raw('screen'),v=>v.screen==='playing','replaced bus closes old owner menu');
  const inventory=await fixture.raw('inventory');assert.equal(inventory.counts?.['minecraft:diamond'],4);assert(!inventory.carried);
  assert(!chat(await fixture.command(`data get block ${bus} ae2_lab_config.parts`)).includes('minecraft:diamond'));
  fixture.assertHidden(await fixture.raw('status'));fixture.mark('replacement cannot receive stale configuration or the carried stack');fixture.report('passed');
} catch(error){fixture.report('failed',error);throw error;}
finally{await fixture.close();}
