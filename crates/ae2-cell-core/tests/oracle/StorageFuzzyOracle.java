/*
 * The two bound methods below come from AE2 FuzzySearch.java at
 * db17504a86128fdf3dae31f5fb7a112a646e0b93.
 * Copyright (c) TeamAppliedEnergistics; LGPL-3.0-or-later.
 * Run with: java ae2-cell-core/tests/oracle/StorageFuzzyOracle.java
 * Minimal stub keys reproduce the source bounds; this is not a live AE2 game.
 */
public class StorageFuzzyOracle {
    enum FuzzyMode {
        IGNORE_ALL(-1), PERCENT_99(0), PERCENT_75(25), PERCENT_50(50), PERCENT_25(75);
        final float percentage;
        FuzzyMode(float value) { percentage = value; }
        int calculateBreakPoint(int maxDamage) { return (int) (percentage * maxDamage / 100.0f); }
    }
    record AEKey(int value, int max) {
        int getFuzzySearchValue() { return value; }
        int getFuzzySearchMaxValue() { return max; }
    }
    record FuzzyBound(int itemDamage) {}
    static final class Preconditions {
        static void checkState(boolean condition, String message, Object value) {
            if (!condition) throw new IllegalStateException(message.formatted(value));
        }
    }
    private static final int MIN_DAMAGE_VALUE = -1;

    static FuzzyBound makeLowerBound(AEKey key, FuzzyMode fuzzy) {
        var maxValue = key.getFuzzySearchMaxValue();
        Preconditions.checkState(maxValue > 0, "Cannot use fuzzy search on keys that don't have a fuzzy max value: %s",
                key);

        int damage;
        if (fuzzy == FuzzyMode.IGNORE_ALL) {
            damage = maxValue;
        } else {
            var breakpoint = fuzzy.calculateBreakPoint(maxValue);
            damage = key.getFuzzySearchValue() <= breakpoint ? breakpoint : maxValue;
        }

        return new FuzzyBound(damage);
    }

    static FuzzyBound makeUpperBound(AEKey key, FuzzyMode fuzzy) {
        var maxValue = key.getFuzzySearchMaxValue();
        Preconditions.checkState(maxValue > 0, "Cannot use fuzzy search on keys that don't have a fuzzy max value: %s",
                key);

        int damage;
        if (fuzzy == FuzzyMode.IGNORE_ALL) {
            damage = MIN_DAMAGE_VALUE;
        } else {
            final var breakpoint = fuzzy.calculateBreakPoint(maxValue);
            damage = key.getFuzzySearchValue() <= breakpoint ? MIN_DAMAGE_VALUE : breakpoint;
        }

        return new FuzzyBound(damage);
    }

    // FuzzySearch's sorted map is ordered from highest to lowest damage;
    // its lower bound is inclusive and its upper bound exclusive.
    static boolean listed(AEKey configured, AEKey incoming, FuzzyMode mode) {
        return configured.value() <= makeLowerBound(incoming, mode).itemDamage()
                && configured.value() > makeUpperBound(incoming, mode).itemDamage();
    }
    static void check(String label, boolean actual, boolean expected) {
        if (actual != expected) throw new AssertionError(label + ": " + actual);
        System.out.println(label + "=" + actual);
    }
    public static void main(String[] args) {
        check("different_max_percent50", listed(new AEKey(75, 100), new AEKey(120, 200), FuzzyMode.PERCENT_50), false);
        check("same_query_partition", listed(new AEKey(120, 100), new AEKey(120, 200), FuzzyMode.PERCENT_50), true);
        check("beyond_query_max", listed(new AEKey(201, 300), new AEKey(120, 200), FuzzyMode.IGNORE_ALL), false);
    }
}
