import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
const reference=process.env.AE2_REFERENCE;assert(reference,'Set AE2_REFERENCE to the pinned AE2 source checkout');
const revision=Bun.spawnSync(['git','-C',reference,'rev-parse','HEAD']);
assert.equal(revision.exitCode,0);assert.equal(revision.stdout.toString().trim(),'db17504a86128fdf3dae31f5fb7a112a646e0b93');
const source=readFileSync(join(reference,'src/main/java/appeng/me/cells/BasicCellInventory.java'),'utf8');
let template=readFileSync(join(import.meta.dir,'SourceMixedCellOracle.java'),'utf8');
for(const [marker,signature] of [
  ['// ACTUAL_PERSIST','    public void persist()'],
  ['// ACTUAL_SAVE_CHANGES','    protected void saveChanges()'],
  ['// ACTUAL_GET_CELL_ITEMS','    protected Object2LongMap<AEKey> getCellItems()'],
  ['// ACTUAL_LOAD_CELL_ITEMS','    private void loadCellItems()'],
  ['// ACTUAL_AVAILABLE_STACKS','    public void getAvailableStacks(KeyCounter out)'],
  ['// ACTUAL_EXTRACT','    public long extract(AEKey what, long amount, Actionable mode, IActionSource source)'],
  ['// ACTUAL_INNER_INSERT','    private long innerInsert(AEKey what, long amount, Actionable mode)'],
  ['// ACTUAL_CAN_HOLD_NEW_ITEM','    public boolean canHoldNewItem()'],
  ['// ACTUAL_TOTAL_ITEM_TYPES','    public long getTotalItemTypes()'],
  ['// ACTUAL_REMAINING_ITEM_TYPES','    public long getRemainingItemTypes()'],
  ['// ACTUAL_BYTES_PER_TYPE','    public int getBytesPerType()'],
  ['// ACTUAL_TOTAL_BYTES','    public long getTotalBytes()'],
  ['// ACTUAL_STORED_COUNT','    public long getStoredItemCount()'],
  ['// ACTUAL_STORED_TYPES','    public long getStoredItemTypes()'],
  ['// ACTUAL_FREE_BYTES','    public long getFreeBytes()'],
  ['// ACTUAL_USED_BYTES','    public long getUsedBytes()'],
  ['// ACTUAL_UNUSED_COUNT','    public int getUnusedItemCount()'],
  ['// ACTUAL_REMAINING_COUNT','    public long getRemainingItemCount()'],
] as const) {
  const start=source.indexOf(signature);assert(start>=0,`missing ${signature}`);
  const open=source.indexOf('{',start);let depth=1,end=open+1;
  while(depth&&end<source.length){if(source[end]==='{')depth++;if(source[end]==='}')depth--;end++;}
  assert.equal(depth,0);assert(template.includes(marker));template=template.replace(marker,source.slice(start,end));
}
const dir=mkdtempSync(join(tmpdir(),'ae2-mixed-cell-oracle-'));
const java=join(dir,'SourceMixedCellOracle.java');writeFileSync(java,template);
for(const command of [['javac','-d',dir,java],['java','-cp',dir,'SourceMixedCellOracle']]) {
  const result=Bun.spawnSync(command,{stdout:'pipe',stderr:'pipe'});
  process.stdout.write(result.stdout);process.stderr.write(result.stderr);assert.equal(result.exitCode,0);
}
