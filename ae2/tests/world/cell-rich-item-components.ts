/** Preserve source-valid rich items through a physical AE2 cell and client. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';
const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('cell-rich-item-components', mode);
const potion = 'minecraft:potion[minecraft:potion_contents={potion:"minecraft:water",custom_color:1193046,custom_effects:[{id:"minecraft:speed",duration:120,amplifier:1b}]}]';
const sword = 'minecraft:diamond_sword[minecraft:enchantments={levels:{"minecraft:sharpness":3},show_in_tooltip:0b}]';
const book = 'minecraft:written_book[minecraft:written_book_content={title:{raw:"Ledger",filtered:"Masked"},author:"Tester",pages:[{raw:{text:"scarlet",color:"red"},filtered:{text:"hidden"}}],resolved:1b}]';
const text=(value:any)=>JSON.stringify(value);
const has=(value:any,...needles:string[])=>needles.every(needle=>text(value).includes(needle));
try {
  await fixture.start(); await fixture.command('clear @s');
  await fixture.replace('weapon.offhand','ae2:item_storage_cell_1k');
  await fixture.replace('hotbar.0',potion); await fixture.replace('hotbar.1',book); await fixture.replace('hotbar.2',sword);
  await fixture.select(0,'minecraft:potion');
  assert(has(await fixture.command('data get entity @s SelectedItem'),'custom_color','1193046','custom_effects','minecraft:speed'),'real potion retains source custom color and effects');
  assert.equal((await fixture.ae('insert')).inserted,1);
  await fixture.select(1,'minecraft:written_book');
  assert(has(await fixture.command('data get entity @s SelectedItem'),'scarlet','red','filtered','hidden','resolved'),'real written book retains styled and filtered source pages');
  assert.equal((await fixture.ae('insert')).inserted,1);
  await fixture.select(2,'minecraft:diamond_sword');
  assert(has(await fixture.command('data get entity @s SelectedItem'),'minecraft:sharpness','show_in_tooltip'),'real enchantments retain the source hidden-tooltip flag');
  assert.equal((await fixture.ae('insert')).inserted,1);
  const stored=await fixture.ae('status'); assert.equal(stored.items,3); assert.equal(stored.types,3);
  fixture.mark('a real AE2 cell accepts the potion, written book and enchanted sword as three source component-bearing keys');
  await fixture.restart();
  assert.equal((await fixture.ae('status')).items,3);
  for(const id of ['minecraft:potion','minecraft:written_book','minecraft:diamond_sword']) {
    const status=await fixture.ae('status'); const entry=status.entries.find((entry:any)=>entry.item===id);
    assert(entry, `${id} remains in the actual saved cell`); assert.equal((await fixture.ae(`extract ${entry.index} 1`)).extracted,1);
  }
  const authoritative=await fixture.command('data get entity @s Inventory');
  assert(has(authoritative,'custom_color','1193046','custom_effects','minecraft:speed'));
  assert(has(authoritative,'scarlet','red','filtered','hidden','resolved'));
  assert(has(authoritative,'minecraft:sharpness','show_in_tooltip'));
  await until(()=>fixture.raw('inventory'), view=>has(view,'custom_color','1193046','custom_effects','minecraft:speed')
    && has(view,'scarlet','red','filtered','hidden','resolved') && has(view,'minecraft:sharpness','show_in_tooltip'),'retrieved source components reach the real client');
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('the actual cell restores potion, book and enchantment data after restart and the items reach the client');
  fixture.report('passed');
} catch(error) { fixture.report('failed',error); throw error; }
finally { await fixture.close(); }
