/*
 * Methods are derived from Applied Energistics 2:
 * appeng/crafting/inv/ICraftingSimulationState.java (addStackBytes),
 * appeng/crafting/pattern/AEPatternHelper.java (condenseStacks),
 * appeng/crafting/CraftingTreeNode.java and CraftingTreeProcess.java (getNodeCount),
 * commit db17504a86128fdf3dae31f5fb7a112a646e0b93.
 * Copyright (c) 2013-2014 AlgorithmX2; (c) 2021 TeamAppliedEnergistics.
 * LGPL-3.0-or-later; see the adjacent crate LICENSE.
 *
 * The surrounding key classes and examples are deliberately minimal stubs.
 * They exercise byte arithmetic, not Minecraft or the AE2 crafting tree.
 */
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

public class CraftingByteOracle {
    record KeyType(int amountPerByte) {
        public int getAmountPerByte() { return amountPerByte; }
    }

    record AEKey(KeyType type, String id) {
        public KeyType getType() { return type; }
    }

    record GenericStack(AEKey what, long amount) {}

    // AEPatternHelper.condenseStacks, pinned method body.
    public static List<GenericStack> condenseStacks(List<GenericStack> sparseInput) {
        // Use a linked map to preserve ordering.
        var map = new LinkedHashMap<AEKey, Long>();

        for (var input : sparseInput) {
            if (input != null) {
                map.merge(input.what(), input.amount(), Long::sum);
            }
        }

        if (map.isEmpty()) {
            throw new IllegalStateException("No pattern here!");
        }

        List<GenericStack> out = new ArrayList<>(map.size());
        for (var entry : map.entrySet()) {
            out.add(new GenericStack(entry.getKey(), entry.getValue()));
        }
        return out;
    }

    static class CraftingTreeNode {
        ArrayList<CraftingTreeProcess> nodes = null;

        // CraftingTreeNode.getNodeCount, pinned method body.
        long getNodeCount() {
            long tot = 1;
            if (this.nodes != null) {
                for (CraftingTreeProcess pro : this.nodes) {
                    tot += pro.getNodeCount();
                }
            }
            return tot;
        }
    }

    static class CraftingTreeProcess {
        final Map<CraftingTreeNode, Long> nodes = new LinkedHashMap<>();

        // CraftingTreeProcess.getNodeCount, pinned method body.
        long getNodeCount() {
            long tot = 0;
            for (CraftingTreeNode node : this.nodes.keySet()) {
                tot += node.getNodeCount();
            }
            return tot;
        }
    }

    interface CraftingState {
        void addBytes(double bytes);

        // AE2 ICraftingSimulationState.addStackBytes, pinned method body.
        default void addStackBytes(AEKey key, long amount, long multiplier) {
            // Crafting storage is 8 times bigger than normal storage, this is intentional.
            addBytes((double) amount * multiplier / key.getType().getAmountPerByte() * 8);
        }
    }

    static class State implements CraftingState {
        double bytes;
        public void addBytes(double bytes) { this.bytes += bytes; }
        long result() { return (long) Math.ceil(bytes); }
    }

    static void check(String name, State state, long expected) {
        long actual = state.result();
        if (actual != expected) {
            throw new AssertionError(name + ": expected " + expected + ", got " + actual);
        }
        System.out.println(name + "=" + actual);
    }

    public static void main(String[] args) {
        AEKey item = new AEKey(new KeyType(8), "item");
        AEKey fluid = new AEKey(new KeyType(8000), "fluid");

        State batch = new State();
        batch.addBytes(2 * 8); // root and one input node
        batch.addStackBytes(item, 1, 64); // requested output
        batch.addStackBytes(item, 1, 64); // requested input
        batch.addBytes(64); // pattern executions
        check("item_batch", batch, 208);

        State mixed = new State();
        mixed.addBytes(3 * 8); // root and two input nodes
        mixed.addStackBytes(fluid, 1, 200);
        mixed.addStackBytes(item, 2, 1);
        mixed.addStackBytes(fluid, 750, 1);
        mixed.addBytes(1);
        check("mixed_fluid", mixed, 28);

        State alternatives = new State();
        alternatives.addBytes(3 * 8); // root and both candidate input nodes
        alternatives.addStackBytes(item, 1, 2);
        alternatives.addStackBytes(item, 1, 1);
        alternatives.addStackBytes(item, 1, 1);
        alternatives.addBytes(2);
        check("alternatives", alternatives, 30);

        State nested = new State();
        nested.addBytes(3 * 8); // root and two nested input nodes
        nested.addStackBytes(item, 1, 1);
        nested.addStackBytes(item, 1, 1);
        nested.addStackBytes(item, 1, 1);
        nested.addBytes(2);
        check("nested", nested, 29);

        AEKey milk = new AEKey(item.type(), "milk"), sugar = new AEKey(item.type(), "sugar");
        AEKey egg = new AEKey(item.type(), "egg"), wheat = new AEKey(item.type(), "wheat");
        var condensed = condenseStacks(List.of(
                new GenericStack(milk, 1), new GenericStack(milk, 1), new GenericStack(milk, 1),
                new GenericStack(sugar, 1), new GenericStack(egg, 1), new GenericStack(sugar, 1),
                new GenericStack(wheat, 1), new GenericStack(wheat, 1), new GenericStack(wheat, 1)));
        if (!condensed.equals(List.of(new GenericStack(milk, 3), new GenericStack(sugar, 2),
                new GenericStack(egg, 1), new GenericStack(wheat, 3)))) throw new AssertionError(condensed);
        State cake = new State();
        cake.addBytes((1 + condensed.size()) * 8);
        cake.addStackBytes(item, 1, 2); // two requested cakes
        for (var input : condensed) cake.addStackBytes(input.what(), 1, input.amount() * 2);
        cake.addStackBytes(item, 6, 1); // returned buckets
        cake.addBytes(2);
        check("cake", cake, 68);

        var root = new CraftingTreeNode();
        var first = new CraftingTreeProcess();
        var second = new CraftingTreeProcess();
        var a = new CraftingTreeNode(); var b = new CraftingTreeNode();
        first.nodes.put(a, 1L); second.nodes.put(b, 1L);
        root.nodes = new ArrayList<>(List.of(first, second));
        var aProcess = new CraftingTreeProcess(); aProcess.nodes.put(new CraftingTreeNode(), 1L);
        a.nodes = new ArrayList<>(List.of(aProcess));
        if (root.getNodeCount() != 4) throw new AssertionError("first path tree");
        var bProcess = new CraftingTreeProcess(); bProcess.nodes.put(new CraftingTreeNode(), 1L);
        b.nodes = new ArrayList<>(List.of(bProcess));
        if (root.getNodeCount() != 5) throw new AssertionError("later path tree");
        System.out.println("manual_lazy_tree=4,5");
    }
}
