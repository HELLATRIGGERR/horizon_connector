/** In-frame access-point link slots return their real item on close/replacement. */
import assert from 'node:assert/strict';
import { WorldFixture, until } from './harness';
const fixture=new WorldFixture('inframe-wireless-access-point','in-frame');
const slot=(view:any,index:number)=>view.slots?.find((s:any)=>s.slot===index);
const chat=(reply:any)=>(reply.chat??[]).join('\n');
try {
  const start=await fixture.start();const [px,py,pz]=start.player.position;
  const x=Math.floor(px)+24,y=Math.ceil(py)+5,z=Math.floor(pz)+24,access=`${x} ${y} ${z}`;
  await fixture.command(`tp @s ${x+.5} ${y} ${z+2.5}`);
  for(const [dx,dz] of [[-2,-3],[-2,3],[2,-3],[2,3]] as const)
    await until(()=>fixture.raw(`block ${x+dx} ${y} ${z+dz}`),v=>v.loaded,'access point fixture chunk loaded',30000);
  await fixture.command(`fill ${x-2} ${y} ${z-3} ${x+2} ${y+4} ${z+3} minecraft:air`);
  await fixture.command(`fill ${x-2} ${y-1} ${z-3} ${x+2} ${y-1} ${z+3} minecraft:stone`);
  await fixture.command(`setblock ${access} ae2:wireless_access_point`);await fixture.command('clear @s');await fixture.command('gamemode survival @s');
  await fixture.replace('hotbar.0','ae2:wireless_terminal');await fixture.replace('hotbar.8','minecraft:stick');
  const open=async()=>{await fixture.select(8,'minecraft:stick');await fixture.command(`tp @s ${x+.5} ${y} ${z+2.5}`);await fixture.raw(`interact ${access} south`);
    await until(()=>fixture.raw('screen'),v=>v.screen==='container' && slot(v,301) && slot(v,302),'access point menu');};
  const link=async()=>{await fixture.raw('click-slot 0');await fixture.raw('click-slot 301');
    await until(()=>fixture.raw('screen'),v=>slot(v,302)?.item==='ae2:wireless_terminal' && String(slot(v,302)?.components??'').includes('ae2:wireless_link_target') && !v.carried,'linked output owns the physical terminal');};
  await open();await link();await fixture.raw('close');
  let inv=await fixture.raw('inventory');assert.equal(inv.counts?.['ae2:wireless_terminal'],1);assert(!inv.carried);
  assert.match(chat(await fixture.command('data get entity @s Inventory')),/ae2:wireless_link_target/);
  await fixture.restart();await fixture.select(0,'ae2:wireless_terminal');
  assert.match(chat(await fixture.command('data get entity @s SelectedItem')),/ae2:wireless_link_target/);
  fixture.mark('close returns the linked terminal and its source component survives restart');
  await open();await link();await fixture.command(`setblock ${access} minecraft:stone`);
  await until(()=>fixture.raw('screen'),v=>v.screen==='playing','replacing access point closes the original session');
  inv=await fixture.raw('inventory');assert.equal(inv.counts?.['ae2:wireless_terminal'],1);assert(!inv.carried);
  fixture.assertHidden(await fixture.raw('status'));fixture.mark('replacement returns the exact pending linked terminal');fixture.report('passed');
} catch(error){fixture.report('failed',error);throw error;}
finally{await fixture.close();}
