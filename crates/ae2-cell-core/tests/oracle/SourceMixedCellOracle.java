// LGPL-3.0-or-later. The marked methods are inserted at runtime from
// AE2 19.2.18 BasicCellInventory. Stubs model only the key, counter and
// primitive map operations those methods use; this is not a running game.
import java.util.*;

interface AEKey {}
interface AEItemKey extends AEKey {default Object toStack(){return new Object();}}
record ItemKey(String id) implements AEItemKey {}
interface NestedCell {boolean canFitInsideCell();}
class StorageCells {static NestedCell getCellInventory(Object item,Object container){return null;}}
record FluidKey(String id) implements AEKey {}
record GenericStack(AEKey what, long amount) {}
enum Actionable { SIMULATE, MODULATE }
interface IActionSource {}
interface Object2LongMap<K> {
    interface Entry<K> { K getKey(); long getLongValue(); }
    long getLong(K key);
    long put(K key,long value);
    boolean remove(K key,long value);
    int size();
    Collection<Long> values();
    Iterable<Entry<K>> entries();
    Iterable<Entry<K>> object2LongEntrySet();
}
class Object2LongOpenHashMap<K> implements Object2LongMap<K> {
    final Map<K,Long> values=new LinkedHashMap<>();
    public long getLong(K key) { return values.getOrDefault(key,0L); }
    public long put(K key,long value) { var previous=values.put(key,value); return previous==null ? 0 : previous; }
    public boolean remove(K key,long value) { return values.remove(key,value); }
    public int size(){return values.size();}
    public Collection<Long> values(){return values.values();}
    public Iterable<Entry<K>> object2LongEntrySet(){return entries();}
    public Iterable<Entry<K>> entries() {
        var out=new ArrayList<Entry<K>>();
        for(var entry:values.entrySet()) out.add(new Entry<>() {
            public K getKey(){return entry.getKey();}
            public long getLongValue(){return entry.getValue();}
        });
        return out;
    }
}
class Object2LongMaps {
    static <K> Iterable<Object2LongMap.Entry<K>> fastIterable(Object2LongMap<K> values){return values.entries();}
}
class KeyCounter {
    final Map<AEKey,Long> values=new LinkedHashMap<>();
    void add(AEKey key,long amount){values.merge(key,amount,Long::sum);}
}
class CellType {
    int getBytesPerType(Object item){return 8;}
    long getBytes(Object item){return 1024;}
}
class KeyType { int getAmountPerByte(){return 8;} }
interface ISaveProvider {void saveChanges();}
class AEComponents {static final Object STORAGE_CELL_INV=new Object();}
class CellItem {
    List<GenericStack> stored;
    CellItem(List<GenericStack> stored){this.stored=stored;}
    void set(Object component,List<GenericStack> stacks){stored=List.copyOf(stacks);}
    void remove(Object component){stored=List.of();}
}
class BasicCellInventory {
    Object2LongMap<AEKey> storedAmounts;
    final CellItem i;
    final CellType cellType=new CellType();
    final KeyType keyType=new KeyType();
    final long maxItemsPerType=Long.MAX_VALUE;
    final int maxItemTypes=63;
    long storedItemCount;
    int storedItems;
    boolean isPersisted=true;
    int saves;
    final ISaveProvider container=()->saves++;
    BasicCellInventory(List<GenericStack> stored){this.i=new CellItem(stored);this.storedItems=stored.size();this.storedItemCount=stored.stream().mapToLong(GenericStack::amount).sum();}
    List<GenericStack> getStoredStacks(){return i.stored;}
    // ACTUAL_PERSIST
    // ACTUAL_SAVE_CHANGES
    // ACTUAL_GET_CELL_ITEMS
    // ACTUAL_LOAD_CELL_ITEMS
    // ACTUAL_AVAILABLE_STACKS
    // ACTUAL_EXTRACT
    // ACTUAL_INNER_INSERT
    // ACTUAL_CAN_HOLD_NEW_ITEM
    // ACTUAL_TOTAL_ITEM_TYPES
    // ACTUAL_REMAINING_ITEM_TYPES
    // ACTUAL_BYTES_PER_TYPE
    // ACTUAL_TOTAL_BYTES
    // ACTUAL_STORED_COUNT
    // ACTUAL_STORED_TYPES
    // ACTUAL_FREE_BYTES
    // ACTUAL_USED_BYTES
    // ACTUAL_UNUSED_COUNT
    // ACTUAL_REMAINING_COUNT
    long insertDirect(AEKey key,long amount,Actionable mode){return innerInsert(key,amount,mode);}
}
public class SourceMixedCellOracle {
    static void check(boolean value,String message){if(!value)throw new AssertionError(message);System.out.println(message+"=1");}
    public static void main(String[] args) {
        var water=new FluidKey("same:id");
        var recovered=new ItemKey("same:id");
        var cell=new BasicCellInventory(List.of(new GenericStack(water,1000),new GenericStack(recovered,3)));
        var visible=new KeyCounter();cell.getAvailableStacks(visible);
        check(visible.values.get(water)==1000 && visible.values.get(recovered)==3,"both_key_channels_visible");
        check(cell.extract(recovered,2,Actionable.SIMULATE,null)==2 && cell.saves==0,"foreign_simulation_no_mutation");
        check(cell.extract(recovered,2,Actionable.MODULATE,null)==2 && cell.saves==1,"foreign_extraction_commits");
        var after=new KeyCounter();cell.getAvailableStacks(after);
        check(after.values.get(recovered)==1 && after.values.get(water)==1000,"other_channel_and_remainder_preserved");
        var overfilledKey=new ItemKey("minecraft:stone");
        var overfilled=new BasicCellInventory(List.of(new GenericStack(overfilledKey,9000)));
        var large=new KeyCounter();overfilled.getAvailableStacks(large);
        check(large.values.get(overfilledKey)==9000 && overfilled.getRemainingItemCount()==0 && overfilled.extract(overfilledKey,64,Actionable.MODULATE,null)==64,"overfilled_source_key_remains_extractable");
        var repeated=new ItemKey("duplicate");
        var duplicates=new BasicCellInventory(List.of(new GenericStack(repeated,4),new GenericStack(repeated,7)));
        check(duplicates.getStoredItemTypes()==2 && duplicates.getStoredItemCount()==11,"duplicate_initial_accounting_uses_list");
        var compacted=new KeyCounter();duplicates.getAvailableStacks(compacted);
        check(compacted.values.get(repeated)==7 && duplicates.getStoredItemCount()==11,"duplicate_loaded_map_keeps_last");
        check(duplicates.extract(repeated,2,Actionable.MODULATE,null)==2 && duplicates.getStoredItemTypes()==1 && duplicates.getStoredItemCount()==5,"duplicate_mutation_recounts_map");
        var noRoom=new BasicCellInventory(List.of(new GenericStack(repeated,4000),new GenericStack(repeated,5000)));
        check(noRoom.getRemainingItemCount()==0 && noRoom.insertDirect(repeated,1,Actionable.MODULATE)==0,"overfilled_duplicate_first_insert_accepts_zero");
        check(noRoom.getStoredItemCount()==5000 && noRoom.getStoredItemTypes()==1 && noRoom.insertDirect(repeated,1,Actionable.MODULATE)==1,"zero_insert_still_recounts_before_next_attempt");
        check(noRoom.i.stored.size()==2 && !noRoom.isPersisted,"mounted_source_list_remains_until_persist");
        noRoom.persist();var reloaded=new BasicCellInventory(noRoom.i.stored);
        var mountedValues=new KeyCounter();noRoom.getAvailableStacks(mountedValues);
        var reloadedValues=new KeyCounter();reloaded.getAvailableStacks(reloadedValues);
        check(noRoom.i.stored.size()==1 && mountedValues.values.equals(reloadedValues.values)
            && noRoom.getStoredItemCount()==reloaded.getStoredItemCount() && noRoom.getStoredItemTypes()==reloaded.getStoredItemTypes(),"positive_duplicate_mounted_and_reloaded_state_agree");
        var zero=new ItemKey("zero");var negative=new ItemKey("negative");
        var odd=new BasicCellInventory(List.of(new GenericStack(zero,0),new GenericStack(negative,-3),new GenericStack(repeated,7)));
        var oddVisible=new KeyCounter();odd.getAvailableStacks(oddVisible);
        check(oddVisible.values.get(zero)==0 && oddVisible.values.get(negative)==-3 && odd.extract(negative,1,Actionable.MODULATE,null)==0,"nonpositive_map_values_are_visible_but_not_extractable");
        check(odd.extract(repeated,2,Actionable.MODULATE,null)==2 && odd.getStoredItemTypes()==3 && odd.getStoredItemCount()==2,"nonpositive_map_values_contribute_after_mutation");
        odd.persist();var oddReloaded=new BasicCellInventory(odd.i.stored);
        check(odd.getStoredItemTypes()==3 && odd.getStoredItemCount()==2 && oddReloaded.getStoredItemTypes()==1 && oddReloaded.getStoredItemCount()==5,"nonpositive_mounted_state_differs_from_reloaded_state");
    }
}
