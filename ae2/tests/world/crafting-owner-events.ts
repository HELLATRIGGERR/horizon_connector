/** Owner-only crafting lifecycle, pinning, notice and persisted job identity. */
import assert from 'node:assert/strict';
import { WorldFixture, until } from './harness';
import { startPeer } from './peer';

const fixture = new WorldFixture('crafting-owner-events', 'server-thread');
let peer: Awaited<ReturnType<typeof startPeer>> | undefined;
const counts = (status: any, item: string) => status.items?.find((entry: any) => entry.item === item)?.count ?? 0;
const sameId = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);
try {
  const start = await fixture.start();
  const [px,py,pz]=start.player.position, x=Math.floor(px)+32, y=Math.ceil(py)+5, z=Math.floor(pz)+32;
  const at=(dx:number,dy=0,dz=0)=>`${x+dx} ${y+dy} ${z+dz}`;
  const drive=at(1),terminal=at(2),cpu=at(2,1),provider=at(3),assembler=at(3,0,-1);
  await fixture.command('clear @s');
  await fixture.command(`fill ${at(-2,0,-3)} ${at(6,5,3)} minecraft:air`);
  await fixture.command(`fill ${at(-2,-1,-3)} ${at(6,-1,3)} minecraft:stone`);
  for(const [pos,id] of [[at(0),'creative_energy_cell'],[drive,'drive'],[terminal,'cable_bus'],[cpu,'1k_crafting_storage'],[provider,'pattern_provider'],[assembler,'molecular_assembler']] as const) await fixture.command(`setblock ${pos} ae2:${id}`);
  await fixture.command(`item replace block ${drive} container.0 with ae2:item_storage_cell_1k`);
  await fixture.replace('hotbar.0','ae2:terminal');await fixture.select(0,'ae2:terminal');
  assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached,true);
  const ready=()=>until(()=>fixture.ae(`grid status ${drive}`),s=>s.online&&s.powered,'powered owner grid',30000);
  await ready();await until(()=>fixture.ae(`grid status ${assembler}`),s=>s.online&&s.powered,'assembler online',30000);
  await fixture.replace('hotbar.0','ae2:blank_pattern');await fixture.select(0,'ae2:blank_pattern');
  assert.equal((await fixture.ae('crafting encode minecraft:oak_log - - - - - - - -')).encoded,true);
  await fixture.command(`tp @s ${x+3.5} ${y} ${z+2.5}`);await fixture.raw(`interact ${provider} south`);
  const providerMenu=await until(()=>fixture.raw('screen'),s=>s.slots?.some((v:any)=>v.slot<36&&v.item==='ae2:crafting_pattern'),'provider pattern');
  await fixture.raw(`click-slot ${providerMenu.slots.find((v:any)=>v.slot<36&&v.item==='ae2:crafting_pattern').slot} shift`);
  await until(()=>fixture.raw('inventory'),s=>!s.counts?.['ae2:crafting_pattern'],'physical pattern installed');await fixture.raw('close');
  async function addLogs(){for(let i=0;i<3;i++){await fixture.replace('hotbar.0','minecraft:oak_log',64);await fixture.select(0,'minecraft:oak_log');assert.equal((await fixture.ae(`grid deposit ${drive}`)).inserted,64);}}
  await addLogs();
  // The source shows a completion toast only for a linked, charged wireless
  // terminal in the 36 regular slots and when no storage terminal is open.
  const wireless=`ae2:wireless_terminal[minecraft:custom_data={ae2_wireless_energy:10000.0d,ae2_wireless_link:{dimension:"minecraft:overworld",pos:[I;${x+1},${y},${z}]}}]`;
  await fixture.replace('hotbar.7',wireless);await fixture.replace('hotbar.8','minecraft:blaze_rod');await fixture.select(8,'minecraft:blaze_rod');
  await fixture.command(`tp @s ${x+2.5} ${y} ${z-2.5}`);
  await fixture.raw('lan open 0 commands');
  const lan=await fixture.raw('lan status');assert(lan.port>0,'localhost LAN listener');
  peer=await startPeer(fixture,`127.0.0.1:${lan.port}`,'AE2Observer');
  assert.equal((await peer.raw('ae2-job-ui')).events.length,0);

  const trace=()=>fixture.raw('ae2-job-ui');
  const openTerminal=async()=>{
    const pins=(await trace()).pins;
    const pinnedPlanks=pins.some((pin:any)=>pin.item==='minecraft:oak_planks' && !pin.prune);
    await fixture.raw(`interact ${terminal} north`);
    return until(()=>fixture.raw('screen'),s=>JSON.stringify(s.title??'').includes('AE2 ME Terminal')
      && (!pinnedPlanks || s.slots?.some((slot:any)=>slot.slot===300&&slot.item==='minecraft:oak_planks')),
      'storage terminal and any pending pinned row');
  };
  const submit=async()=>{
    const before=(await trace()).events.length;
    const view=await openTerminal();
    const slot=view.slots?.find((s:any)=>s.slot>=300&&s.slot<345&&s.item==='minecraft:oak_planks');assert(slot,'craftable output');
    await fixture.raw(`click-slot ${slot.slot} shift`);
    await until(()=>fixture.raw('screen'),s=>JSON.stringify(s.title??'').includes('AE2 Craft Amount'),'craft amount');
    await fixture.raw('ui-key right');await fixture.raw('ui-key backspace');await fixture.raw('type 672');await fixture.raw('wait 5');await fixture.raw('click-slot 302');
    const confirm=await until(()=>fixture.raw('screen'),s=>JSON.stringify(s.title??'').includes('AE2 Craft Confirm') && /bytes/.test(JSON.stringify(s)) && !/Calculating/i.test(JSON.stringify(s)),'completed craft confirmation');
    assert.doesNotMatch(JSON.stringify(confirm),/Missing materials/);
    await fixture.raw('click-slot 302');
    const state=await until(trace,s=>s.events.slice(before).some((e:any)=>e.status==='started'),'owner STARTED event');
    const event=state.events.slice(before).find((e:any)=>e.status==='started');
    assert.equal(event.requested,672);assert.equal(state.pending.some((id:any)=>sameId(id,event.job_id)),true);
    assert(state.pins.some((p:any)=>p.item==='minecraft:oak_planks'));
    return event.job_id;
  };

  const first=await submit();
  await fixture.raw('close');
  await openTerminal();
  await until(()=>fixture.raw('screen'),s=>s.slots?.some((v:any)=>v.slot===300&&v.item==='minecraft:oak_planks') && s.slots?.some((v:any)=>v.slot===301&&!v.item),'owner pin occupies the first terminal row');
  await fixture.raw('close');
  const finished=await until(trace,s=>s.events.some((e:any)=>e.status==='finished'&&sameId(e.job_id,first))&&s.toasts.some((t:any)=>t.item==='minecraft:oak_planks'),'owner FINISHED event and wireless notice',120000);
  assert(!finished.pending.some((id:any)=>sameId(id,first)));
  assert.equal((await peer.raw('ae2-job-ui')).events.length,0,'observer received no owner lifecycle packets');
  fixture.mark('only the owner receives STARTED/FINISHED, the output is pinned, and a linked charged wireless terminal shows the notice');

  await addLogs();await fixture.select(8,'minecraft:blaze_rod');
  const second=await submit();await fixture.raw('close');await openTerminal();
  await fixture.raw('click-slot 349');
  await until(()=>fixture.raw('screen'),s=>JSON.stringify(s.title??'').includes('AE2 Crafting Status'),'active CPU status');
  await fixture.raw('click-slot 303');
  const cancelled=await until(trace,s=>s.events.some((e:any)=>e.status==='cancelled'&&sameId(e.job_id,second)),'owner CANCELLED event');
  assert(!cancelled.pending.some((id:any)=>sameId(id,second)));
  assert.equal((await peer.raw('ae2-job-ui')).events.length,0,'observer still has no owner events');
  await fixture.raw('close');
  fixture.mark('cancellation is delivered only to the requesting owner');

  await addLogs();await fixture.select(8,'minecraft:blaze_rod');
  const beforeRestart=counts(await ready(),'minecraft:oak_planks');
  const third=await submit();await fixture.raw('close');
  await fixture.hold();await fixture.raw('save');
  await peer.stop();peer=undefined;
  await fixture.restart();
  await until(trace,s=>s.events.some((e:any)=>e.status==='finished'&&sameId(e.job_id,third)),'same owner receives FINISHED after restart',120000);
  await until(ready,s=>counts(s,'minecraft:oak_planks')>=beforeRestart+672,'restarted job produces its physical output',120000);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('a saved active job preserves its owner and sends FINISHED after restart');
  fixture.report('passed');
} catch(error) {fixture.report('failed',error);throw error;}
finally {if(peer)await peer.stop();await fixture.close();}
