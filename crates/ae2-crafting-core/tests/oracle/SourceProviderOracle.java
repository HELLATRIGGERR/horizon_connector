// LGPL-3.0-or-later: methods inserted at runtime are from AE2 19.2.18,
// AEProcessingPattern.pushInputsToExternalInventory,
// IPatternDetails.pushInputsToExternalInventory and PatternProviderLogic
// addToSendList/sendStacksOut. Stubs model keys, insertion and retry only.
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Iterator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Objects;

record AEKey(String id, int perOperation) {
    AEKey(String id) { this(id,1); }
    int getAmountPerOperation() { return perOperation; }
}
record GenericStack(AEKey what, long amount) {}
enum Actionable { MODULATE, SIMULATE }
interface IActionSource {}
class MEStorage { static void checkPreconditions(AEKey key,long amount,Actionable mode,IActionSource source) {} }
class CondenserBlockEntity {
    boolean outputAvailable=true; double power=0;
    boolean canAddOutput() {return outputAvailable;}
    void addPower(double amount) {power+=amount;}
}
class CondenserMEStorage {
    final CondenserBlockEntity target;
    CondenserMEStorage(CondenserBlockEntity target) {this.target=target;}
    // ACTUAL_CONDENSER_INSERT
}
interface PatternInputSink { void pushInput(AEKey key, long amount); }
class KeyCounter implements Iterable<KeyCounter.Entry> {
    record Entry(AEKey key, long value) {
        AEKey getKey() { return key; }
        long getLongValue() { return value; }
    }
    final LinkedHashMap<AEKey, Long> values = new LinkedHashMap<>();
    void add(AEKey key, long amount) { values.merge(key, amount, Long::sum); }
    void addAll(KeyCounter source) { source.values.forEach(this::add); }
    long get(AEKey key) { return values.getOrDefault(key, 0L); }
    void remove(AEKey key, long amount) { values.put(key, get(key) - amount); }
    public Iterator<Entry> iterator() { return values.entrySet().stream().map(e -> new Entry(e.getKey(), e.getValue())).iterator(); }
}
interface IPatternDetails {
    // ACTUAL_DEFAULT_PUSH
}
class AEProcessingPattern implements IPatternDetails {
    final List<GenericStack> sparseInputs;
    final Object[] inputs;
    AEProcessingPattern(List<GenericStack> slots, int condensed) { sparseInputs=slots; inputs=new Object[condensed]; }
    // ACTUAL_PROCESSING_PUSH
}
interface PatternProviderTarget { long insert(AEKey key, long amount, Actionable action); }
class MainNode { void ifPresent(java.util.function.BiConsumer<Object, Object> fn) {} }
class Grid { TickManager getTickManager() { return new TickManager(); } }
class TickManager { void alertDevice(Object node) {} }
class PatternProviderLogic {
    final List<GenericStack> sendList = new ArrayList<>();
    final TestMainNode mainNode = new TestMainNode();
    final PatternProviderTarget adapter;
    Integer sendDirection=1;
    PatternProviderLogic(PatternProviderTarget adapter) { this.adapter=adapter; }
    PatternProviderTarget findAdapter(Integer direction) { return adapter; }
    // ACTUAL_ADD_SEND
    // ACTUAL_SEND_STACKS
}
class TestMainNode { void ifPresent(java.util.function.BiConsumer<Grid,Object> fn) {} }
public class SourceProviderOracle {
    static void eq(Object actual, Object expected, String name) {
        if (!Objects.equals(actual, expected)) throw new AssertionError(name + ": " + actual + " != " + expected);
        System.out.println(name + "=1");
    }
    public static void main(String[] args) {
        var water=new AEKey("fluid:water");var iron=new AEKey("item:iron");
        var sparse=List.of(new GenericStack(water,1000),new GenericStack(iron,3),new GenericStack(water,500),new GenericStack(iron,2));
        var pattern=new AEProcessingPattern(sparse,2);
        var first=new KeyCounter();first.add(water,1500);var second=new KeyCounter();second.add(iron,5);
        var order=new ArrayList<GenericStack>();pattern.pushInputsToExternalInventory(new KeyCounter[]{first,second},(key,amount)->order.add(new GenericStack(key,amount)));
        eq(order,sparse,"sparse_mixed_order");
        var amounts=new ArrayDeque<Long>(List.of(400L,1L,500L,0L));var calls=new ArrayList<GenericStack>();
        PatternProviderTarget target=(key,amount,action)->{calls.add(new GenericStack(key,amount));return amounts.remove();};
        var provider=new PatternProviderLogic(target);
        provider.addToSendList(water,900);provider.addToSendList(iron,2);provider.addToSendList(water,500);provider.addToSendList(iron,2);
        eq(provider.sendStacksOut(),true,"partial_retry_reports_progress");
        eq(calls,List.of(new GenericStack(water,900),new GenericStack(iron,2),new GenericStack(water,500),new GenericStack(iron,2)),"retry_insertion_order");
        eq(provider.sendList,List.of(new GenericStack(water,500),new GenericStack(iron,1),new GenericStack(iron,2)),"partial_remainders");
        provider.sendList.clear();provider.addToSendList(iron,999999);
        eq(provider.sendList,List.of(new GenericStack(iron,999999)),"large_pending_amount_is_one_generic_stack");
        var condenser=new CondenserBlockEntity();var me=new CondenserMEStorage(condenser);
        var fluid=new AEKey("fluid:water",125);
        eq(me.insert(fluid,1377,Actionable.SIMULATE,null),1377L,"condenser_simulates_full_amount");
        eq(condenser.power,0.0,"condenser_simulation_preserves_power");
        eq(me.insert(fluid,1377,Actionable.MODULATE,null),1377L,"condenser_accepts_over_a_bucket");
        eq(condenser.power,11.0,"condenser_uses_integer_division");
        eq(me.insert(fluid,124,Actionable.MODULATE,null),124L,"condenser_accepts_subunit_fluid");
        eq(condenser.power,11.0,"condenser_discards_subunit_power");
        condenser.outputAvailable=false;
        eq(me.insert(fluid,1377,Actionable.MODULATE,null),0L,"full_output_blocks_me_insertion");
        eq(condenser.power,11.0,"blocked_output_preserves_power");
    }
}
