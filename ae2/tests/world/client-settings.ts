/** AE2 client search settings and crafting/encoding close behavior. */
import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('client-settings', mode);
const config = join(fixture.game, 'config', 'ae2-client.toml');
const saveSettings = (remember: boolean) => writeFileSync(config, `[search]\nrememberLastSearch = ${remember}\nautoFocusSearch = true\n[terminals]\nclearGridOnClose = true\n`);
const has = (view: any, id: string) => view.slots?.some((slot: any) => slot.slot >= 300 && slot.slot < 327 && slot.item === id);
try {
  mkdirSync(join(fixture.game, 'config'), {recursive:true});
  saveSettings(true);
  const state = await fixture.start();
  const [px,py,pz] = state.player.position, x=Math.floor(px), y=Math.ceil(py)+3, z=Math.floor(pz);
  const at=(dx:number)=>`${x+dx} ${y} ${z}`, power=at(0), drive=at(1), terminal=at(2);
  await fixture.command(`fill ${x-2} ${y-1} ${z-3} ${x+4} ${y-1} ${z+3} minecraft:bedrock`);
  await fixture.command(`fill ${x-2} ${y} ${z-3} ${x+4} ${y+4} ${z+3} minecraft:air`);
  await fixture.command('clear @s');
  for (const [pos,id] of [[power,'creative_energy_cell'],[drive,'drive'],[terminal,'cable_bus']] as const) {
    await fixture.command(`setblock ${pos} ae2:${id}`);
    await fixture.ae(`grid status ${pos}`);
  }
  await fixture.command(`item replace block ${drive} container.0 with ae2:item_storage_cell_1k`);
  await fixture.replace('hotbar.0','ae2:crafting_terminal'); await fixture.select(0,'ae2:crafting_terminal');
  assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached,true);
  await until(()=>fixture.ae(`grid status ${terminal}`),view=>view.online,'powered crafting terminal');
  for (const item of ['minecraft:diamond','minecraft:iron_ingot']) {
    await fixture.replace('hotbar.0',item); await fixture.select(0,item);
    assert.equal((await fixture.ae(`grid deposit ${terminal}`)).inserted,1);
  }
  await fixture.replace('hotbar.8','minecraft:stick'); await fixture.select(8,'minecraft:stick');
  await fixture.command(`tp @s ${x+2.5} ${y} ${z-2.5}`);
  const open=async()=>{
    await fixture.raw(`interact ${terminal} north`);
    return until(()=>fixture.raw('screen'),view=>JSON.stringify(view.title??'').includes('AE2 Crafting Terminal'),'crafting terminal menu');
  };
  await open();
  // No focus command: configured auto-focus must route actual typing to search.
  await fixture.raw('type diamond');
  await until(()=>fixture.raw('screen'),view=>has(view,'minecraft:diamond')&&!has(view,'minecraft:iron_ingot'),'auto-focused query filters actual inventory');
  await fixture.raw('close'); await open();
  await until(()=>fixture.raw('screen'),view=>has(view,'minecraft:diamond')&&!has(view,'minecraft:iron_ingot'),'remembered query is restored and applied after reopening');
  fixture.mark('configured auto-focus accepts typing and remembered search filters the reopened terminal');
  await fixture.raw('close'); saveSettings(false); await open();
  await until(()=>fixture.raw('screen'),view=>has(view,'minecraft:diamond')&&has(view,'minecraft:iron_ingot'),'disabling remembered search reopens unfiltered');
  fixture.mark('disabling rememberLastSearch starts a new terminal with an empty query');
  await fixture.raw('close');

  await fixture.replace('hotbar.0','minecraft:oak_log'); await fixture.select(8,'minecraft:stick'); await open();
  await fixture.raw('click-slot 0');
  await until(()=>fixture.raw('inventory'),view=>view.carried?.item==='minecraft:oak_log','one real log on cursor');
  await fixture.raw('click-slot 327');
  await until(()=>fixture.raw('screen'),view=>view.slots?.some((slot:any)=>slot.slot===327 && slot.item==='minecraft:oak_log'),'real log in crafting matrix');
  await fixture.raw('close');
  await until(()=>fixture.ae(`grid status ${terminal}`),view=>view.items?.some((item:any)=>item.item==='minecraft:oak_log' && item.count===1),'close clears matrix into real network storage');
  await open();
  const cleared=await fixture.raw('screen');
  assert(!cleared.slots?.some((slot:any)=>slot.slot===327 && slot.item==='minecraft:oak_log'));
  await fixture.raw('close'); fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('clearGridOnClose moves the physical crafting input to the network once');

  await fixture.replace('hotbar.0','ae2:pattern_encoding_terminal');
  await fixture.select(0,'ae2:pattern_encoding_terminal');
  assert.equal((await fixture.ae(`grid part ${terminal} south`)).attached,true);
  await fixture.replace('hotbar.0','minecraft:diamond');
  await fixture.select(8,'minecraft:stick');
  await fixture.command(`tp @s ${x+2.5} ${y} ${z+2.5}`);
  const encodingData=async()=> (await fixture.command(`data get block ${terminal} ae2_lab_config.parts[{face:"south"}]`)).chat.join('\n');
  const openEncoding=async()=>{
    await fixture.raw(`interact ${terminal} south`);
    await until(()=>fixture.raw('screen'),view=>JSON.stringify(view.title??'').includes('AE2 ME Terminal') && view.slots?.some((s:any)=>s.slot===348 && String(s.components??'').includes('Encode crafting pattern')),'encoding terminal menu');
    await fixture.raw('click-slot 348');
    await until(()=>fixture.raw('screen'),view=>view.slots?.some((s:any)=>s.slot===348 && String(s.components??'').includes('Back to storage')),'encoding grid');
  };
  await openEncoding();
  await fixture.raw('click-slot 0');
  await until(()=>fixture.raw('inventory'),view=>view.carried?.item==='minecraft:diamond','real diamond on cursor');
  await fixture.raw('click-slot 300');
  await until(encodingData,data=>data.includes('minecraft:diamond'),'encoding ghost stored by the physical part');
  await fixture.raw('close');
  await until(encodingData,data=>!data.includes('minecraft:diamond'),'close clears the stored encoding ghost');
  const afterEncoding=await fixture.raw('inventory');
  assert.equal(afterEncoding.counts?.['minecraft:diamond'],1,'the real cursor item returns once');
  await openEncoding();
  const emptyEncoding=await fixture.raw('screen');
  assert(!emptyEncoding.slots?.some((s:any)=>s.slot===300 && s.item==='minecraft:diamond'));
  await fixture.raw('close');
  fixture.mark('clearGridOnClose removes encoding ghosts without consuming their physical source');
  fixture.report('passed');
} catch(error) { fixture.report('failed',error); throw error; }
finally { await fixture.close(); }
