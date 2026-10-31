/*
 * Test-only stubs for the actual AE2 CraftingTreeNode and CraftingTreeProcess
 * source classes at commit db17504a86128fdf3dae31f5fb7a112a646e0b93.
 * Those classes are supplied from the source checkout by run-tree-oracle.ts;
 * they are Copyright (c) 2013-2014 AlgorithmX2, LGPL-3.0-or-later.
 * The stubs model only the inputs used by these cases, not a Minecraft world.
 */
import java.util.*;
import java.util.function.Predicate;

class Level {}
enum Actionable { SIMULATE, MODULATE }
record AEKey(String id, int amountPerByte) {
    boolean matches(GenericStack stack) { return equals(stack.what()); }
    public String toString() { return id; }
}
record GenericStack(AEKey what, long amount) {}
record InputTemplate(AEKey key, long amount) {}
class CraftBranchFailure extends Exception {
    CraftBranchFailure(AEKey key, long amount) { super(key + ":" + amount); }
}
interface ICraftingInventory {
    long extract(AEKey key, long amount, Actionable action);
}
class KeyCounter implements Iterable<KeyCounter.Entry> {
    record Entry(AEKey key, long amount) {
        AEKey getKey() { return key; }
        long getLongValue() { return amount; }
    }
    final LinkedHashMap<AEKey,Long> items = new LinkedHashMap<>();
    void add(AEKey key,long amount) { items.merge(key,amount,Long::sum); }
    public Iterator<Entry> iterator() {
        return items.entrySet().stream().map(e -> new Entry(e.getKey(),e.getValue())).iterator();
    }
    long get(AEKey key) { return items.getOrDefault(key,0L); }
}
interface IPatternDetails {
    interface IInput {
        GenericStack[] getPossibleInputs();
        long getMultiplier();
        boolean isValid(AEKey key, Level level);
        AEKey getRemainingKey(AEKey key);
    }
    IInput[] getInputs();
    List<GenericStack> getOutputs();
    String getDefinition();
}
record PatternInput(AEKey key,long multiplier,AEKey remainder) implements IPatternDetails.IInput {
    public GenericStack[] getPossibleInputs(){return new GenericStack[]{new GenericStack(key,1)};}
    public long getMultiplier(){return multiplier;}
    public boolean isValid(AEKey candidate,Level level){return key.equals(candidate);}
    public AEKey getRemainingKey(AEKey candidate){return key.equals(candidate)?remainder:null;}
}
record ChoiceInput(AEKey[] keys,long multiplier) implements IPatternDetails.IInput {
    public GenericStack[] getPossibleInputs(){return Arrays.stream(keys).map(key->new GenericStack(key,1)).toArray(GenericStack[]::new);}
    public long getMultiplier(){return multiplier;}
    public boolean isValid(AEKey candidate,Level level){return Arrays.asList(keys).contains(candidate);}
    public AEKey getRemainingKey(AEKey candidate){return null;}
}
record ChoiceReturnInput(AEKey[] keys,AEKey[] returns,long multiplier) implements IPatternDetails.IInput {
    public GenericStack[] getPossibleInputs(){return Arrays.stream(keys).map(key->new GenericStack(key,1)).toArray(GenericStack[]::new);}
    public long getMultiplier(){return multiplier;}
    public boolean isValid(AEKey candidate,Level level){return Arrays.asList(keys).contains(candidate);}
    public AEKey getRemainingKey(AEKey candidate){int index=Arrays.asList(keys).indexOf(candidate);return index<0?null:returns[index];}
}
record MixedAmountInput(GenericStack[] candidates,long multiplier) implements IPatternDetails.IInput {
    public GenericStack[] getPossibleInputs(){return candidates;}
    public long getMultiplier(){return multiplier;}
    public boolean isValid(AEKey candidate,Level level){return Arrays.stream(candidates).anyMatch(s->s.what().equals(candidate));}
    public AEKey getRemainingKey(AEKey candidate){return null;}
}
record MixedAmountReturnInput(GenericStack[] candidates,AEKey[] returns,long multiplier) implements IPatternDetails.IInput {
    public GenericStack[] getPossibleInputs(){return candidates;}
    public long getMultiplier(){return multiplier;}
    public boolean isValid(AEKey candidate,Level level){return Arrays.stream(candidates).anyMatch(s->s.what().equals(candidate));}
    public AEKey getRemainingKey(AEKey candidate){for(int i=0;i<candidates.length;i++){if(candidates[i].what().equals(candidate))return returns[i];}return null;}
}
record Pattern(String name,IPatternDetails.IInput[] inputs,List<GenericStack> outputs) implements IPatternDetails {
    public IInput[] getInputs(){return inputs;}
    public List<GenericStack> getOutputs(){return outputs;}
    public String getDefinition(){return name;}
}
interface ICraftingService {
    boolean canEmitFor(AEKey key);
    Collection<IPatternDetails> getCraftingFor(AEKey key);
    AEKey getFuzzyCraftable(AEKey key,Predicate<AEKey> valid);
}
class Service implements ICraftingService {
    final LinkedHashMap<AEKey,List<IPatternDetails>> patterns = new LinkedHashMap<>();
    void add(Pattern p){patterns.computeIfAbsent(p.outputs().get(0).what(),k->new ArrayList<>()).add(p);}
    public boolean canEmitFor(AEKey key){return false;}
    public Collection<IPatternDetails> getCraftingFor(AEKey key){return patterns.getOrDefault(key,List.of());}
    // These stub keys have no components, so a fuzzy primary-key search has
    // one exact-key match. This exercises choosing another possible input.
    public AEKey getFuzzyCraftable(AEKey key,Predicate<AEKey> valid){
        return patterns.keySet().stream().filter(candidate->candidate.id().equals(key.id()) && valid.test(candidate)).findFirst().orElse(null);
    }
}
record Grid(Service crafting) { Service getCraftingService(){return crafting;} }
record GridNode(Grid grid) { Grid getGrid(){return grid;} }
record Requester(GridNode node) { GridNode getGridNode(){return node;} }
class CraftingCalculation {
    final Requester simRequester;
    final KeyCounter missing = new KeyCounter();
    boolean simulation;
    CraftingCalculation(Service service){simRequester=new Requester(new GridNode(new Grid(service)));}
    Level getLevel(){return new Level();}
    void handlePausing() throws InterruptedException {}
    boolean isSimulation(){return simulation;}
    void addMissing(AEKey key,long amount){missing.add(key,amount);}
}
class CraftingCpuHelper {
    static Iterable<InputTemplate> getValidItemTemplates(ICraftingInventory inventory,IPatternDetails.IInput input,Level level){
        return Arrays.stream(input.getPossibleInputs()).filter(s->input.isValid(s.what(),level)).map(s->new InputTemplate(s.what(),s.amount())).toList();
    }
    static long extractTemplates(ICraftingInventory inventory,InputTemplate template,long requested){
        long possible=inventory.extract(template.key(),requested*template.amount(),Actionable.SIMULATE)/template.amount();
        inventory.extract(template.key(),possible*template.amount(),Actionable.MODULATE);return possible;
    }
}
class CraftingSimulationState implements ICraftingInventory {
    final LinkedHashMap<AEKey,Long> inventory;
    double bytes;
    CraftingSimulationState(Map<AEKey,Long> inventory){this.inventory=new LinkedHashMap<>(inventory);}
    void addStackBytes(AEKey key,long amount,long multiplier){addBytes((double)amount*multiplier/key.amountPerByte()*8);}
    void addBytes(double amount){bytes+=amount;}
    void addCrafting(IPatternDetails details,long times){}
    void emitItems(AEKey key,long amount){throw new UnsupportedOperationException("emission not modeled");}
    void insert(AEKey key,long amount,Actionable action){if(action==Actionable.MODULATE)inventory.merge(key,amount,Long::sum);}
    public long extract(AEKey key,long amount,Actionable action){
        long available=inventory.getOrDefault(key,0L), taken=Math.min(available,amount);
        if(action==Actionable.MODULATE){if(taken==available)inventory.remove(key);else inventory.put(key,available-taken);}return taken;
    }
}
class ChildCraftingSimulationState extends CraftingSimulationState {
    ChildCraftingSimulationState(CraftingSimulationState parent){super(parent.inventory);}
    void applyDiff(CraftingSimulationState parent){parent.inventory.clear();parent.inventory.putAll(inventory);parent.bytes+=bytes;}
}
public class CraftingTreeOracle {
    static AEKey key(String name){return new AEKey(name,8);}
    static IPatternDetails.IInput in(AEKey key,long count,AEKey remainder){return new PatternInput(key,count,remainder);}
    static Pattern pattern(String name,AEKey output,long count,IPatternDetails.IInput... inputs){return new Pattern(name,inputs,List.of(new GenericStack(output,count)));}
    static void check(String label,long actual,long expected){if(actual!=expected)throw new AssertionError(label+": expected "+expected+", got "+actual);System.out.println(label+"="+actual);}
    static long plan(Service service,AEKey target,long count,Map<AEKey,Long> inventory) throws Exception {
        var job=new CraftingCalculation(service);var root=new CraftingTreeNode(service,job,target,1,null,-1);
        var state=new CraftingSimulationState(inventory);state.inventory.remove(target);root.request(state,count,null);
        state.addBytes(root.getNodeCount()*8);return (long)Math.ceil(state.bytes);
    }
    static void expectShortage(String label,Service service,AEKey target,Map<AEKey,Long> inventory,String missing) throws Exception {
        var job=new CraftingCalculation(service);var root=new CraftingTreeNode(service,job,target,1,null,-1);
        try {
            root.request(new CraftingSimulationState(inventory),1,null);
            throw new AssertionError(label+": expected source shortage");
        } catch(CraftBranchFailure expected) {
            if(!expected.getMessage().equals(missing)) throw new AssertionError(label+": "+expected.getMessage());
            check(label,1,1);
        }
    }
    public static void main(String[] args) throws Exception {
        AEKey milk=key("milk"), bucket=key("bucket"), sugar=key("sugar"), egg=key("egg"), wheat=key("wheat"), cake=key("cake");
        var cakes=new Service();cakes.add(pattern("cake",cake,1,in(milk,3,bucket),in(sugar,2,null),in(egg,1,null),in(wheat,3,null)));
        check("cake",plan(cakes,cake,2,Map.of(milk,6L,sugar,4L,egg,2L,wheat,6L)),68);
        var clay=key("clay");var reuse=new Service();reuse.add(pattern("reuse",clay,1,in(milk,1,bucket),in(bucket,1,null)));
        check("reuse",plan(reuse,clay,2,Map.of(milk,2L,bucket,1L)),34);
        check("reuse_4097",plan(reuse,clay,4097,Map.of(milk,4097L,bucket,1L)),20509);
        var outputAndRemainder=new Service();outputAndRemainder.add(pattern("same",bucket,1,in(milk,1,bucket)));
        check("output_is_also_remainder",plan(outputAndRemainder,bucket,2,Map.of(milk,1L)),21);
        AEKey choiceOak=key("oak"),choiceBirch=key("birch"),rawBirch=key("raw_birch"),missingOak=key("missing_oak"),box=key("box");
        var substitute=new Service();substitute.add(pattern("birch",choiceBirch,1,in(rawBirch,1,null)));
        substitute.add(pattern("box",box,1,new ChoiceInput(new AEKey[]{choiceOak,choiceBirch},1)));
        check("crafted_substitute",plan(substitute,box,1,Map.of(rawBirch,1L)),29);
        var sticks=key("sticks");var vanillaShape=new Service();
        vanillaShape.add(pattern("birch_planks",choiceBirch,4,in(rawBirch,1,null)));
        vanillaShape.add(pattern("sticks",sticks,4,new ChoiceInput(new AEKey[]{choiceOak,choiceBirch},2)));
        check("crafted_substitute_batch",plan(vanillaShape,sticks,4,Map.of(rawBirch,1L)),33);
        var primaryFirst=new Service();primaryFirst.add(pattern("oak",choiceOak,1,in(missingOak,1,null)));
        primaryFirst.add(pattern("birch",choiceBirch,1,in(rawBirch,1,null)));
        primaryFirst.add(pattern("box",box,1,new ChoiceInput(new AEKey[]{choiceOak,choiceBirch},1)));
        expectShortage("primary_pattern_prevents_fallback",primaryFirst,box,Map.of(rawBirch,1L),"missing_oak:1");
        var differentAmount=new Service();differentAmount.add(pattern("birch",choiceBirch,1000,in(rawBirch,1,null)));
        differentAmount.add(pattern("box",box,1,new MixedAmountInput(new GenericStack[]{new GenericStack(choiceOak,1),new GenericStack(choiceBirch,1000)},1)));
        expectShortage("different_amount_prevents_fallback",differentAmount,box,Map.of(rawBirch,1L),"oak:1");
        AEKey rawA=key("raw_a"),rawB=key("raw_b"),a=key("a"),b=key("b"),target=key("target");
        var lazy=new Service();lazy.add(pattern("make_a",a,1,in(rawA,1,null)));lazy.add(pattern("make_b",b,1,in(rawB,1,null)));
        lazy.add(pattern("first",target,1,in(a,1,null)));lazy.add(pattern("second",target,1,in(b,1,null)));
        check("first_success",plan(lazy,target,1,Map.of(rawA,1L,rawB,1L)),37);
        AEKey rawC=key("raw_c"),c=key("c");var failed=new Service();
        failed.add(pattern("make_a",a,1,in(rawA,1,null)));failed.add(pattern("make_b",b,1,in(rawB,1,null)));failed.add(pattern("make_c",c,1,in(rawC,1,null)));
        failed.add(pattern("first",target,1,in(a,1,null),in(b,1,null)));failed.add(pattern("second",target,1,in(c,1,null)));
        check("failed_first",plan(failed,target,1,Map.of(rawA,1L,rawC,1L)),61);
        var recursion=new Service();
        recursion.add(new Pattern("recursive",new IPatternDetails.IInput[]{in(rawA,1,null)},List.of(new GenericStack(a,1),new GenericStack(target,1))));
        recursion.add(pattern("safe",a,1,in(rawA,1,null)));recursion.add(pattern("final",target,1,in(a,1,null)));
        check("pruned_ancestor",plan(recursion,target,1,Map.of(rawA,1L)),29);
        AEKey iron=key("iron"),gold=key("gold"),part=key("part");var simulated=new Service();
        simulated.add(pattern("iron_path",part,1,in(iron,1,null)));simulated.add(pattern("gold_path",part,1,in(gold,1,null)));
        var job=new CraftingCalculation(simulated);var root=new CraftingTreeNode(simulated,job,part,1,null,-1);
        try{root.request(new CraftingSimulationState(Map.of(iron,1L,gold,1L)),3,null);throw new AssertionError("expected first attempt to fail");}catch(CraftBranchFailure expected){}
        job.simulation=true;root.request(new CraftingSimulationState(Map.of(iron,1L,gold,1L)),3,null);
        check("simulated_missing_iron",job.missing.get(iron),2);check("simulated_missing_gold",job.missing.get(gold),0);
        var log=key("log");var board=key("board");var partial=new Service();
        partial.add(pattern("boards",board,4,in(log,1,null)));
        var partialJob=new CraftingCalculation(partial);var partialRoot=new CraftingTreeNode(partial,partialJob,board,1,null,-1);
        long wanted=16,successful=0,bytes=0;
        try{partialRoot.request(new CraftingSimulationState(Map.of(log,3L)),wanted,null);throw new AssertionError("expected full attempt to fail");}catch(CraftBranchFailure expected){}
        for(long increment=Long.highestOneBit(wanted);increment>0;increment/=2){
            long test=successful+increment;
            if(test<wanted){
                var state=new CraftingSimulationState(Map.of(log,3L));
                try {
                    partialRoot.request(state,test,null);state.addBytes(partialRoot.getNodeCount()*8);
                    successful=test;bytes=(long)Math.ceil(state.bytes);
                } catch(CraftBranchFailure expected){}
            }
        }
        check("craft_less_amount",successful,12);check("craft_less_bytes",bytes,34);
        var oak=key("oak");var birch=key("birch");var substituted=new Service();
        var wrappedAmount=new Service();
        wrappedAmount.add(pattern("wrapped_two",board,4,new MixedAmountInput(new GenericStack[]{new GenericStack(oak,2)},2)));
        check("wrapped_two_template_times_multiplier_bytes",plan(wrappedAmount,board,4,Map.of(oak,4L)),25);
        expectShortage("wrapped_two_three_stock_shortage",wrappedAmount,board,Map.of(oak,3L),"oak:2");
        var groupedAmounts=new Service();
        groupedAmounts.add(pattern("wrapped_two_plus_one",board,4,new MixedAmountInput(new GenericStack[]{new GenericStack(oak,2)},3)));
        check("wrapped_grouped_template_times_condensed_multiplier",plan(groupedAmounts,board,4,Map.of(oak,6L)),27);
        var wrappedFallback=new Service();
        wrappedFallback.add(pattern("wrapped_two_with_substitute",board,4,new MixedAmountInput(new GenericStack[]{new GenericStack(oak,2),new GenericStack(birch,1)},2)));
        check("wrapped_two_split_substitute_bytes",plan(wrappedFallback,board,4,Map.of(oak,2L,birch,1L)),25);
        var repeatedKey=new Service();
        repeatedKey.add(pattern("repeated_key",board,4,new MixedAmountInput(new GenericStack[]{new GenericStack(oak,2),new GenericStack(oak,1),new GenericStack(birch,1)},2)));
        check("wrapped_two_same_key_substitute_bytes",plan(repeatedKey,board,4,Map.of(oak,3L)),25);
        check("wrapped_two_same_key_two_runs_bytes",plan(repeatedKey,board,8,Map.of(oak,7L)),34);
        var evenlyMixed=new Service();evenlyMixed.add(pattern("evenly_mixed",board,4,new ChoiceInput(new AEKey[]{oak,birch},3)));
        check("even_totals_distinct_pushes_bytes",plan(evenlyMixed,board,8,Map.of(oak,4L,birch,2L)),32);
        var singleOutput=key("single_output");var evenSingle=new Service();evenSingle.add(pattern("even_single",singleOutput,1,new ChoiceInput(new AEKey[]{oak,birch},3)));
        check("even_single_output_bytes",plan(evenSingle,singleOutput,2,Map.of(oak,4L,birch,2L)),26);
        substituted.add(pattern("substituted",board,4,new ChoiceInput(new AEKey[]{oak,birch},1)));
        check("substitute_split_bytes",plan(substituted,board,8,Map.of(oak,1L,birch,1L)),28);
        check("substitute_split_10000_bytes",plan(substituted,board,40000,Map.of(oak,5001L,birch,4999L)),60016);
        var differingUnits=new Service();
        differingUnits.add(pattern("differing_units",board,4,new MixedAmountInput(new GenericStack[]{new GenericStack(oak,1),new GenericStack(birch,2)},1)));
        check("differing_unit_split_10000_bytes",plan(differingUnits,board,40000,Map.of(oak,4999L,birch,10002L)),60016);
        var condensedSubstituted=new Service();
        condensedSubstituted.add(pattern("condensed_substituted",board,4,new ChoiceInput(new AEKey[]{oak,birch},2)));
        check("condensed_substitute_split_10000_bytes",plan(condensedSubstituted,board,40000,Map.of(oak,5001L,birch,14999L)),70016);
        var returned=new Service();var bottle=key("bottle");
        returned.add(pattern("returned_substitution",board,4,new ChoiceReturnInput(new AEKey[]{oak,birch},new AEKey[]{bucket,bottle},1)));
        check("returned_substitute_split_10000_bytes",plan(returned,board,40000,Map.of(oak,5001L,birch,4999L)),70016);
        var substituteContainerOnly=new Service();
        substituteContainerOnly.add(pattern("substitute_container_only",board,4,new ChoiceReturnInput(new AEKey[]{oak,birch},new AEKey[]{null,bottle},1)));
        check("substitute_container_untracked_bytes",plan(substituteContainerOnly,board,8,Map.of(birch,2L)),28);
        var oneResult=key("single_result");var oneSubstituteContainer=new Service();
        oneSubstituteContainer.add(pattern("single_substitute_container",oneResult,1,new ChoiceReturnInput(new AEKey[]{oak,birch},new AEKey[]{null,bottle},1)));
        check("single_substitute_container_bytes",plan(oneSubstituteContainer,oneResult,1,Map.of(birch,1L)),19);
        var substituteCannotReuse=new Service();
        substituteCannotReuse.add(pattern("substitute_cannot_reuse",board,1,new ChoiceReturnInput(new AEKey[]{oak,birch},new AEKey[]{null,bottle},1),in(bottle,1,null)));
        expectShortage("substitute_container_not_reused",substituteCannotReuse,board,Map.of(birch,1L),"bottle:1");
        var primaryContainerOnly=new Service();
        primaryContainerOnly.add(pattern("primary_container_only",board,4,new ChoiceReturnInput(new AEKey[]{oak,birch},new AEKey[]{bucket,null},1)));
        check("primary_container_alternative_empty",plan(primaryContainerOnly,board,8,Map.of(oak,1L,birch,1L)),29);
        var water=new AEKey("water",8000);var filledBucket=key("water_bucket");var fluidTarget=key("fluid_target");
        var fluidOnly=new Service();fluidOnly.add(pattern("fluid_only",fluidTarget,1,new MixedAmountInput(new GenericStack[]{new GenericStack(water,1000)},1)));
        expectShortage("fluid_only_rejects_bucket",fluidOnly,fluidTarget,Map.of(filledBucket,1L),"water:1000");
        var fluidFirst=new Service();fluidFirst.add(pattern("fluid_first",fluidTarget,1,new MixedAmountReturnInput(new GenericStack[]{new GenericStack(water,1000),new GenericStack(filledBucket,1)},new AEKey[]{null,bucket},1)));
        var fluidJob=new CraftingCalculation(fluidFirst);var fluidRoot=new CraftingTreeNode(fluidFirst,fluidJob,fluidTarget,1,null,-1);
        var fluidStock=new CraftingSimulationState(Map.of(water,1000L,filledBucket,1L));fluidRoot.request(fluidStock,1,null);fluidStock.addBytes(fluidRoot.getNodeCount()*8);
        check("fluid_first_bytes",(long)Math.ceil(fluidStock.bytes),19);check("fluid_first_bucket_preserved",fluidStock.inventory.getOrDefault(filledBucket,0L),1);
        check("fluid_first_water_consumed",fluidStock.inventory.getOrDefault(water,0L),0);
        check("fluid_first_bucket_fallback_bytes",plan(fluidFirst,fluidTarget,1,Map.of(filledBucket,1L)),19);
        check("fluid_first_mixed_batch_bytes",plan(fluidFirst,fluidTarget,2,Map.of(water,1000L,filledBucket,1L)),22);
        var wideTarget=key("wide_target");var wide=new Service();
        for(int i=0;i<4097;i++)wide.add(pattern(String.format("pattern-%04d",i),wideTarget,1,in(key(String.format("input-%04d",i)),1,null)));
        check("wide_4097_candidate_nodes_bytes",plan(wide,wideTarget,1,Map.of(key("input-0000"),1L)),32787);
        var manyKeys=new AEKey[4097];var manyStock=new LinkedHashMap<AEKey,Long>();
        for(int i=0;i<manyKeys.length;i++){manyKeys[i]=key(String.format("input-%04d",i));manyStock.put(manyKeys[i],1L);}
        var manySubstitutes=new Service();manySubstitutes.add(pattern("many_substitutes",wideTarget,1,new ChoiceInput(manyKeys,1)));
        check("many_4097_substitute_steps_bytes",plan(manySubstitutes,wideTarget,4097,manyStock),12307);
        var deep=new Service();var deepInput=key("deep_input");var previous=deepInput;
        for(int i=0;i<256;i++){var out=key(String.format("deep_%03d",i));deep.add(pattern("deep_"+i,out,1,in(previous,1,null)));previous=out;}
        check("deep_256_bytes",plan(deep,previous,1,Map.of(deepInput,1L)),2569);
        // Once the first intermediate branch succeeds, source does not
        // revisit it to save a shared input for the next sibling.
        var rare=key("rare");var common=key("common");var middle=key("middle");var end=key("end");
        var shared=new Service();shared.add(pattern("rare_first",middle,1,in(rare,1,null)));
        shared.add(pattern("common_second",middle,1,in(common,1,null)));
        shared.add(pattern("final",end,1,in(middle,1,null),in(rare,1,null)));
        var greedyJob=new CraftingCalculation(shared);var greedyRoot=new CraftingTreeNode(shared,greedyJob,end,1,null,-1);
        try {
            greedyRoot.request(new CraftingSimulationState(Map.of(rare,1L,common,1L)),1,null);
            throw new AssertionError("expected source to fail without sibling backtracking");
        } catch(CraftBranchFailure expected) {check("source_greedy_shortage",1,1);}
        var chosen=key("chosen");var spare=key("spare");var result=key("result");
        var slots=new Service();
        slots.add(pattern("slots",result,1,new ChoiceInput(new AEKey[]{chosen,spare},1),in(chosen,1,null)));
        expectShortage("choice_sibling_shortage",slots,result,Map.of(chosen,1L,spare,1L),"chosen:1");
        var ironBranch=new Service();
        ironBranch.add(pattern("iron",part,1,in(iron,1,null)));ironBranch.add(pattern("gold",part,1,in(gold,1,null)));
        ironBranch.add(pattern("assembly",result,1,in(part,2,null),in(iron,1,null)));
        expectShortage("mixed_pattern_sibling_shortage",ironBranch,result,Map.of(iron,2L,gold,1L),"iron:1");
        var batch=new Service();
        batch.add(pattern("make",middle,1,new ChoiceInput(new AEKey[]{rare,common},1)));
        batch.add(pattern("assembly",result,1,in(middle,2,null),in(rare,1,null)));
        expectShortage("mixed_batch_sibling_shortage",batch,result,Map.of(rare,2L,common,1L),"rare:1");
    }
}
