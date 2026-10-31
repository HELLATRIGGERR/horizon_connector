// LGPL-3.0-or-later. The marked method is inserted from AE2 19.2.18
// IOPortBlockEntity. Stubs model only its grid, key and storage contracts.
import java.util.*;

enum Actionable { SIMULATE, MODULATE }
enum OperationMode { EMPTY, FILL }
class Settings { static final Object OPERATION_MODE=new Object(); }
class Manager { OperationMode getSetting(Object setting){return OperationMode.EMPTY;} }
interface IActionSource {}
record Key(String id,int amountPerOperation) { int getAmountPerOperation(){return amountPerOperation;} }
class KeyCounter implements Iterable<KeyCounter.Entry> {
    record Entry(Key getKey,long getLongValue) {
        public Key getKey(){return getKey;}
        public long getLongValue(){return getLongValue;}
    }
    final List<Entry> entries=new ArrayList<>();
    public Iterator<Entry> iterator(){return entries.iterator();}
}
interface MEStorage {
    long insert(Key key,long amount,Actionable action,IActionSource source);
    long extract(Key key,long amount,Actionable action,IActionSource source);
    KeyCounter getAvailableStacks();
}
interface StorageCell extends MEStorage {}
class Storage implements StorageCell {
    final Map<Key,Long> values=new LinkedHashMap<>();
    boolean acceptFluid;
    public long insert(Key key,long amount,Actionable action,IActionSource source){
        if(key.amountPerOperation()>1 && !acceptFluid)return 0;
        if(action==Actionable.MODULATE)values.merge(key,amount,Long::sum);
        return amount;
    }
    public long extract(Key key,long amount,Actionable action,IActionSource source){
        long moved=Math.min(values.getOrDefault(key,0L),amount);
        if(action==Actionable.MODULATE)values.put(key,values.getOrDefault(key,0L)-moved);
        return moved;
    }
    public KeyCounter getAvailableStacks(){
        var out=new KeyCounter();for(var entry:values.entrySet())out.entries.add(new KeyCounter.Entry(entry.getKey(),entry.getValue()));return out;
    }
}
class StorageService {
    final Storage inventory;
    StorageService(Storage inventory){this.inventory=inventory;}
    MEStorage getInventory(){return inventory;}
    KeyCounter getCachedInventory(){return inventory.getAvailableStacks();}
}
class IGrid {
    final StorageService storage;
    IGrid(Storage network){storage=new StorageService(network);}
    StorageService getStorageService(){return storage;}
    Object getEnergyService(){return new Object();}
}
class StorageHelper {
    static long poweredInsert(Object energy,MEStorage destination,Key key,long amount,IActionSource source){
        return destination.insert(key,amount,Actionable.MODULATE,source);
    }
}
class IOPortBlockEntity {
    final Manager manager=new Manager();
    final IActionSource mySrc=new IActionSource(){};
    // ACTUAL_TRANSFER_CONTENTS
    long transfer(IGrid grid,StorageCell cell,long budget){return transferContents(grid,cell,budget);}
}
public class SourceIoPortOracle {
    static void check(boolean value,String name){if(!value)throw new AssertionError(name);System.out.println(name+"=1");}
    public static void main(String[] args){
        var water=new Key("water",125);var missing=new Key("missing-content",1);
        var source=new Storage();source.values.put(water,1000L);source.values.put(missing,3L);
        var destination=new Storage();var port=new IOPortBlockEntity();var grid=new IGrid(destination);
        check(port.transfer(grid,source,256)==253,"blocked_fluid_does_not_block_later_item");
        check(source.values.get(water)==1000 && source.values.get(missing)==0 && destination.values.get(missing)==3,"first_transfer_custody");
        destination.acceptFluid=true;
        check(port.transfer(grid,source,256)==248,"fluid_budget_uses_amount_per_operation");
        check(source.values.get(water)==0 && destination.values.get(water)==1000,"second_transfer_custody");
    }
}
