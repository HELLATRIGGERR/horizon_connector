#!/usr/bin/env python3
"""Compile actual AE2 accounting/mutation method bodies against minimal Java stubs.
No game runtime is involved. Compares the unupgraded inner cell logic to Rust.
"""
from pathlib import Path
import os, random, subprocess, tempfile

base = Path(__file__).resolve().parents[2]
source = (base / 'ae2-reference/src/main/java/appeng/me/cells/BasicCellInventory.java').read_text()
names = ['canHoldNewItem', 'getTotalBytes', 'getFreeBytes', 'getTotalItemTypes',
         'getStoredItemCount', 'getStoredItemTypes', 'getRemainingItemTypes',
         'getUsedBytes', 'getRemainingItemCount', 'getUnusedItemCount', 'innerInsert', 'extract']
def method(name):
    import re
    match = re.search(r'    (?:public|private) (?:boolean|long|int) ' + name + r'\(', source)
    if not match: raise ValueError(name)
    start = match.start(); brace = source.index('{', match.end()); depth = 1; i = brace + 1
    while depth:
        depth += (source[i] == '{') - (source[i] == '}'); i += 1
    return source[start:i]
methods = '\n'.join(method(n) for n in names)
java = r'''
import java.util.*;
public class Oracle {
  enum Actionable { SIMULATE, MODULATE }
  static class AEKey { final String v; AEKey(String v) {this.v=v;} public boolean equals(Object o) {return o instanceof AEKey && v.equals(((AEKey)o).v);} public int hashCode() {return v.hashCode();} }
  static class AEItemKey extends AEKey { AEItemKey(String v) {super(v);} Object toStack() {return null;} }
  static class StorageCells { static StorageCell getCellInventory(Object x, Object y) {return null;} }
  static class StorageCell { boolean canFitInsideCell() {return true;} }
  static class IActionSource {}
  static class KeyType { int amount; int getAmountPerByte() {return amount;} }
  static class CellType { int bytes; int getBytes(Object ignored) {return bytes;} }
  static class Counts extends HashMap<AEKey,Long> { long getLong(AEKey k) {return getOrDefault(k,0L);} }
  final KeyType keyType = new KeyType(); final CellType cellType = new CellType(); final Object i = null;
  final Counts items = new Counts(); int storedItems = 0; long storedItemCount = 0; int maxItemTypes = 63, perType = 8; long maxItemsPerType = Long.MAX_VALUE;
  Counts getCellItems() {return items;} int getBytesPerType() {return perType;}
  void saveChanges() { storedItems=items.size(); storedItemCount=items.values().stream().mapToLong(Long::longValue).sum(); }
''' + methods + r'''
  public static void main(String[] args) { Scanner s=new Scanner(System.in); Oracle c=new Oracle(); int tier=Integer.parseInt(args[0]); boolean fluid=args[1].equals("fluid");
    c.cellType.bytes=tier*1024; c.perType=tier*8; c.keyType.amount=fluid?8000:8; c.maxItemTypes=fluid?18:63;
    if (args[2].equals("equal")) {long total=(long)(c.cellType.bytes-c.perType*c.maxItemTypes)*c.keyType.amount; c.maxItemsPerType=(total+c.maxItemTypes-1)/c.maxItemTypes;}
    while(s.hasNextLine()) {String[] p=s.nextLine().split(" "); AEKey k=new AEKey(p[1]+"|"+p[2]); long n=Long.parseLong(p[3]);
      Actionable a=p[4].equals("M")?Actionable.MODULATE:Actionable.SIMULATE;
      long r=p[0].equals("I")?(n == 0 ? 0 : c.innerInsert(k,n,a)):c.extract(k,n,a,null);
      System.out.println(r+" "+c.getUsedBytes()+" "+c.storedItemCount+" "+c.storedItems);
    }
  }
}
'''
rng = random.Random(49102)
# Explicit capacity and type-boundary transitions, followed by reproducible mixed operations.
actions = [('I','x','a',8063,'M'),('I','y','b',10,'S'),('I','y','b',10,'M'),('E','x','a',8063,'M')]
actions += [('I',str(i),'a',1,'M') for i in range(64)]
actions += [('E','1','a',1,'S'),('E','1','a',1,'M'),('I','63','a',1,'M')]
for _ in range(10000):
    actions.append((rng.choice('IIE'),str(rng.randrange(75)),rng.choice(['a','b']),rng.choice([0,1,7,8,9,63,64,100,1000,8128,100000,8000000]),rng.choice(['M','M','S'])))
payload=''.join(' '.join(map(str,a))+'\n' for a in actions)
with tempfile.TemporaryDirectory(prefix='ae2-oracle-') as d:
    p=Path(d); (p/'Oracle.java').write_text(java)
    subprocess.run(['javac',str(p/'Oracle.java')],check=True)
    subprocess.run(['cargo','build','--offline','--quiet','--example','oracle_runner'],cwd=base/'ae2-cell-core',check=True)
    for tier in [1,4,16,64,256]:
        for channel in ['item','fluid']:
            for upgrade in ['none','equal']:
                args=[str(tier),channel,upgrade]
                j=subprocess.run(['java','-cp',d,'Oracle',*args],input=payload,text=True,capture_output=True,check=True).stdout.splitlines()
                r=subprocess.run([str(base/'ae2-cell-core/target/debug/examples/oracle_runner'),*args],input=payload,text=True,capture_output=True,check=True).stdout.splitlines()
                assert len(j)==len(r)==len(actions), (len(j),len(r),len(actions))
                for i,(x,y) in enumerate(zip(j,r)):
                    if x!=y: raise AssertionError(f'{args} action {i} {actions[i]} Java={x} Rust={y}')
    print(f'PASS: {len(actions)} operations across 20 tier/channel/distribution combinations match upstream Java method bodies (seed 49102).')
