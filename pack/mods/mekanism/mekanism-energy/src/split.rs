//! `EmitUtils.sendToAcceptors` over a `Target` with `LongSplitInfo`: an
//! amount split among acceptors as evenly as they take it. An acceptor that
//! needs less than its share takes what it needs and the rest is shared
//! again among the others; the remainder of the division goes one Joule at a
//! time to the first acceptors still in need, and what is left after that to
//! whoever still takes it, in order.

/// `LongSplitInfo`.
struct SplitInfo {
    to_split_among: i64,
    amount_per_changed: bool,
    decrement_targets: bool,
    amount_to_split: i64,
    amount_per_target: i64,
    sent_so_far: i64,
    remainder: i64,
}

impl SplitInfo {
    fn new(amount_to_split: i64, targets: usize) -> Self {
        let to_split_among = targets as i64;
        let (amount_per_target, remainder) = if to_split_among == 0 {
            (0, 0)
        } else {
            (amount_to_split / to_split_among, amount_to_split % to_split_among)
        };
        Self {
            to_split_among,
            amount_per_changed: false,
            decrement_targets: true,
            amount_to_split,
            amount_per_target,
            sent_so_far: 0,
            remainder,
        }
    }

    fn send(&mut self, amount_needed: i64) {
        self.amount_to_split -= amount_needed;
        self.sent_so_far += amount_needed;
        if !self.decrement_targets {
            let difference = amount_needed - self.amount_per_target;
            if difference > 0 {
                self.remainder -= difference;
            }
            return;
        }
        self.to_split_among -= 1;
        if amount_needed != self.amount_per_target && self.to_split_among != 0 {
            let amount_per_last = self.amount_per_target;
            self.amount_per_target = self.amount_to_split / self.to_split_among;
            self.remainder = self.amount_to_split % self.to_split_among;
            if !self.amount_per_changed && self.amount_per_target != amount_per_last {
                self.amount_per_changed = true;
            }
        }
    }

    fn share(&self) -> i64 {
        self.amount_per_target
    }

    fn remainder_amount(&self) -> i64 {
        if self.to_split_among != 0 && self.remainder > 0 {
            self.amount_per_target + 1
        } else {
            self.amount_per_target
        }
    }
}

/// Splits `amount` Joules among `count` acceptors and returns what they took.
/// `insert(i, amount, simulate)` offers acceptor `i` `amount` Joules and
/// returns what it would not take (`IStrictEnergyHandler.insertEnergy`'s
/// remainder); with `simulate` nothing may change.
pub fn send_to_acceptors(
    count: usize,
    amount: u64,
    mut insert: impl FnMut(usize, u64, bool) -> u64,
) -> u64 {
    if count == 0 {
        return 0;
    }
    let amount = amount.min(i64::MAX as u64) as i64;
    // What acceptor `i` takes of `amount`: Target.simulate and acceptAmount.
    let mut take = |i: usize, amount: i64, simulate: bool| -> i64 {
        let rest = insert(i, amount.max(0) as u64, simulate).min(amount.max(0) as u64);
        amount - rest as i64
    };
    let mut split = SplitInfo::new(amount, count);
    let mut needed: Vec<(usize, i64)> = Vec::new();
    // Target.sendPossible.
    for i in 0..count {
        let amount_needed = take(i, split.amount_to_split, true);
        if split.share() == 0 {
            if amount_needed != 0 {
                needed.push((i, amount_needed));
            }
        } else if amount_needed <= split.share() {
            if amount_needed != 0 {
                let taken = take(i, amount_needed, false);
                split.send(taken);
            }
        } else {
            needed.push((i, amount_needed));
        }
    }
    // Target.shiftNeeded while a share changed.
    while split.amount_per_changed {
        split.amount_per_changed = false;
        if split.share() == 0 {
            continue;
        }
        let mut kept = Vec::with_capacity(needed.len());
        for (i, amount_needed) in needed {
            if amount_needed <= split.share() {
                let taken = take(i, amount_needed, false);
                split.send(taken);
            } else {
                kept.push((i, amount_needed));
            }
        }
        needed = kept;
    }
    // Target.sendRemainingSplit.
    if !needed.is_empty() && split.remainder_amount() != 0 {
        let mut kept = Vec::with_capacity(needed.len());
        for (i, amount_needed) in needed {
            let remainder_amount = split.remainder_amount();
            if remainder_amount == 0 {
                // Java returns here, before the unsent pass.
                return split.sent_so_far as u64;
            }
            if amount_needed <= remainder_amount {
                let taken = take(i, amount_needed, false);
                split.send(taken);
            } else {
                split.decrement_targets = false;
                let taken = take(i, remainder_amount, false);
                split.send(taken);
                split.decrement_targets = true;
                kept.push((i, amount_needed));
            }
        }
        if split.amount_to_split != 0 {
            for (i, _) in kept {
                let remaining = split.amount_to_split;
                if remaining == 0 {
                    break;
                }
                let taken = take(i, remaining, false);
                split.send(taken);
            }
        }
    }
    split.sent_so_far as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptors with room for `room[i]` Joules each.
    fn run(room: &mut [u64], amount: u64) -> u64 {
        send_to_acceptors(room.len(), amount, |i, offered, simulate| {
            let taken = offered.min(room[i]);
            if !simulate {
                room[i] -= taken;
            }
            offered - taken
        })
    }

    #[test]
    fn even_shares_and_the_remainder_to_the_first() {
        let mut room = [100, 100, 100];
        assert_eq!(run(&mut room, 10), 10);
        // 10 / 3 = 3 each, the remainder's Joule to the first.
        assert_eq!(room, [96, 97, 97]);
    }

    #[test]
    fn a_small_acceptor_leaves_its_share_to_the_others() {
        let mut room = [2, 100, 100];
        assert_eq!(run(&mut room, 30), 30);
        assert_eq!(room, [0, 86, 86]);
    }

    #[test]
    fn full_acceptors_take_nothing_and_the_total_is_what_was_taken() {
        let mut room = [0, 5, 0];
        assert_eq!(run(&mut room, 100), 5);
        assert_eq!(room, [0, 0, 0]);
        let mut none: [u64; 0] = [];
        assert_eq!(run(&mut none, 100), 0);
    }

    #[test]
    fn less_than_one_joule_each_goes_in_order() {
        let mut room = [10, 10, 10, 10];
        assert_eq!(run(&mut room, 2), 2);
        assert_eq!(room, [9, 9, 10, 10]);
    }
}
