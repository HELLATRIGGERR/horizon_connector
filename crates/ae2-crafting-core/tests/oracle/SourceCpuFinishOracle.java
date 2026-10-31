/* Minimal stubs around methods extracted unchanged from AE2 19.2.18
 * CraftingCpuLogic.java (LGPL-3.0-or-later). This is a method oracle,
 * not Minecraft, a complete CPU, its network service, or a world. */
import java.util.*;

record AEKey(String id) {}
record Output(AEKey what) {}
class Pattern { List<Output> getOutputs() { return List.of(new Output(new AEKey("scheduled"))); } }
class KeyCounter implements Iterable<KeyCounter.Entry> {
    private final LinkedHashMap<AEKey, Entry> entries = new LinkedHashMap<>();
    static class Entry {
        final AEKey key; long value;
        Entry(AEKey key,long value){this.key=key;this.value=value;}
        AEKey getKey(){return key;} long getLongValue(){return value;} void setValue(long value){this.value=value;}
    }
    void put(AEKey key,long value){entries.put(key,new Entry(key,value));}
    long get(AEKey key){var e=entries.get(key);return e==null?0:e.value;}
    boolean isEmpty(){return entries.isEmpty();}
    void removeZeros(){entries.values().removeIf(e->e.value==0);}
    public Iterator<Entry> iterator(){return entries.values().iterator();}
}
class ListCraftingInventory { final KeyCounter list = new KeyCounter(); }
enum Actionable { MODULATE }
class Preconditions { static void checkState(boolean value,String message){if(!value)throw new IllegalStateException(message);} }
class Link {
    boolean done,canceled;
    void markDone(){done=true;} void cancel(){canceled=true;} boolean isCanceled(){return canceled;}
}
class WaitList { boolean cleared; void clear(){cleared=true;} }
class ExecutingCraftingJob {
    final Link link = new Link(); final WaitList waitingFor = new WaitList();
    final Map<Pattern,Integer> tasks = new LinkedHashMap<>(); boolean suspended;
}
class CraftingJobStatusPacket { enum Status { STARTED, FINISHED, CANCELLED } }
class Storage {
    final Map<AEKey,Long> capacity = new HashMap<>(); final Map<AEKey,Long> inserted = new HashMap<>();
    final List<String> history; CraftingCpuLogic cpu;
    Storage(List<String> history){this.history=history;}
    long insert(AEKey key,long offered,Actionable action,Object source){
        if(action!=Actionable.MODULATE||cpu.job!=null)throw new AssertionError("store callback contract");
        long n=Math.min(offered,capacity.getOrDefault(key,0L));
        capacity.put(key,capacity.getOrDefault(key,0L)-n);inserted.merge(key,n,Long::sum);
        history.add("insert:"+key.id()+":"+n);return n;
    }
}
class StorageService {final Storage storage;StorageService(Storage s){storage=s;} Storage getInventory(){return storage;} }
class Grid {final StorageService service;Grid(Storage s){service=new StorageService(s);} StorageService getStorageService(){return service;} }
class Cluster {
    boolean active=true;Grid grid;int dirty;final List<String> history;
    Cluster(List<String> history){this.history=history;}
    boolean isActive(){return active;} Grid getGrid(){return grid;} Object getSrc(){return this;}
    int getCoProcessors(){return 0;} Object getLevel(){return null;} void markDirty(){dirty++;}
    void updateOutput(Object output){history.add("output:"+output);}
}
interface IEnergyService {} class CraftingService {}
class CraftingCpuLogic {
    final List<String> history=new ArrayList<>(); final Cluster cluster=new Cluster(history);
    final ListCraftingInventory inventory=new ListCraftingInventory(); ExecutingCraftingJob job;
    final int[] usedOps=new int[3]; boolean cantStoreItems;
    void postChange(AEKey what){history.add("change:"+what.id());}
    void notifyJobOwner(ExecutingCraftingJob observed,CraftingJobStatusPacket.Status status){
        if(observed!=job)throw new AssertionError("owner before job clear");history.add("owner:"+status);
    }
    int executeCrafting(int n,CraftingService cc,IEnergyService eg,Object level){return 0;}
    void finish(boolean success){finishJob(success);}
    // ACTUAL_FINISH
    // ACTUAL_CANCEL
    // ACTUAL_STORE
    // ACTUAL_TICK
}
public class SourceCpuFinishOracle {
    static final AEKey ITEM=new AEKey("item"),FLUID=new AEKey("fluid");
    static void check(boolean value,String label){if(!value)throw new AssertionError(label);System.out.println(label+"=1");}
    static CraftingCpuLogic cpu(){
        var cpu=new CraftingCpuLogic();var storage=new Storage(cpu.history);storage.cpu=cpu;cpu.cluster.grid=new Grid(storage);
        cpu.inventory.list.put(ITEM,4);cpu.inventory.list.put(FLUID,1000);cpu.job=new ExecutingCraftingJob();cpu.job.tasks.put(new Pattern(),1);
        return cpu;
    }
    public static void main(String[] args){
        var cpu=cpu();var job=cpu.job;var storage=cpu.cluster.grid.service.storage;
        storage.capacity.put(ITEM,2L);cpu.finish(true);
        check(job.link.done && !job.link.canceled && job.waitingFor.cleared && cpu.job==null,"finished_clears_job");
        check(cpu.history.indexOf("owner:FINISHED")<cpu.history.indexOf("insert:item:2"),"owner_notified_before_dump");
        check(cpu.inventory.list.get(ITEM)==2 && cpu.inventory.list.get(FLUID)==1000 && storage.inserted.get(ITEM)==2,"independent_partial_inserts");
        cpu.cluster.active=false;storage.capacity.put(FLUID,1000L);cpu.tickCraftingLogic(null,null);
        check(cpu.inventory.list.get(FLUID)==1000,"offline_cpu_does_not_retry");
        cpu.cluster.active=true;cpu.tickCraftingLogic(null,null);
        check(cpu.inventory.list.get(ITEM)==2 && cpu.inventory.list.get(FLUID)==0 && cpu.cantStoreItems,"active_idle_retry_keeps_blocked_item");
        storage.capacity.put(ITEM,2L);cpu.tickCraftingLogic(null,null);
        check(cpu.inventory.list.isEmpty() && !cpu.cantStoreItems && storage.inserted.get(ITEM)==4,"idle_retry_eventually_drains");
        var cancel=cpu();var canceled=cancel.job;cancel.cancel();
        check(canceled.link.canceled && cancel.job==null && cancel.history.contains("owner:CANCELLED") && cancel.inventory.list.get(FLUID)==1000,"cancel_is_immediate_with_custody");
        var noGrid=cpu();noGrid.cluster.grid=null;noGrid.finish(true);
        check(noGrid.job==null && noGrid.inventory.list.get(ITEM)==4 && noGrid.inventory.list.get(FLUID)==1000 && noGrid.cluster.dirty==0,"missing_grid_keeps_idle_inventory");
    }
}
