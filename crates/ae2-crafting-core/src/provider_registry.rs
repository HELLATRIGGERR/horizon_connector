//! Source-shaped crafting-provider registry, independent of world ownership.
//!
//! AE2 19.2.18's `NetworkCraftingProviders` snapshots each provider's
//! priority and definitions at mount, orders *definitions* by priority, and
//! independently cycles the media for each definition in registration order.
//! The host must supply actual grid membership and refresh events; rebuilding
//! this registry from a sorted world snapshot does not preserve that order.
use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;

/// `key` represents the decoded pattern class and AE item definition, while
/// `primary_output` is the source's craftable-item lookup key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Definition<D, K> {
    pub key: D,
    pub primary_output: K,
}

#[derive(Clone, Debug)]
struct ProviderState<D, K> {
    priority: i32,
    patterns: Vec<Definition<D, K>>,
}

#[derive(Clone, Debug)]
struct Media<P> {
    providers: Vec<P>,
    cursor: usize,
    generation: u64,
}

/// Java's equal-priority ordering comes from a HashSet and is not established
/// by the registry oracle. Consumers must not treat order within a group as a
/// source-backed tie breaker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PriorityGroup<D> {
    pub priority: i32,
    pub definitions: Vec<D>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryError {
    DuplicateProvider,
    InconsistentPrimaryOutput,
    StaleMediaPass,
    GenerationExhausted,
}

/// One limited iteration over the shared cycle. Only `next_medium` consumes a
/// provider; beginning or dropping a pass does not advance the shared cursor.
#[derive(Clone, Debug)]
pub struct MediaPass<D> {
    definition: D,
    generation: u64,
    remaining: usize,
}

#[derive(Clone, Debug)]
pub struct ProviderRegistry<P, D, K> {
    providers: HashMap<P, ProviderState<D, K>>,
    media: HashMap<D, Media<P>>,
    generation: u64,
}

impl<P, D, K> Default for ProviderRegistry<P, D, K> {
    fn default() -> Self {
        Self { providers: HashMap::new(), media: HashMap::new(), generation: 0 }
    }
}

impl<P: Clone + Eq + Hash, D: Clone + Eq + Hash, K: Clone + Eq> ProviderRegistry<P, D, K> {
    pub fn contains_provider(&self, provider: &P) -> bool { self.providers.contains_key(provider) }
    fn advance_generation(&mut self) -> Result<u64, RegistryError> {
        self.generation = self.generation.checked_add(1).ok_or(RegistryError::GenerationExhausted)?;
        Ok(self.generation)
    }

    /// Mount a provider at its actual service-add callback. Repeated equal
    /// definitions within one provider stay repeated in the media list, as in
    /// `ProviderState.mount` in the reference implementation.
    pub fn add(&mut self, provider: P, priority: i32, patterns: Vec<Definition<D, K>>) -> Result<(), RegistryError> {
        if self.providers.contains_key(&provider) { return Err(RegistryError::DuplicateProvider); }
        for pattern in &patterns {
            if self.providers.values().flat_map(|state| &state.patterns)
                .chain(patterns.iter())
                .any(|other| other.key == pattern.key && other.primary_output != pattern.primary_output) {
                return Err(RegistryError::InconsistentPrimaryOutput);
            }
        }
        let generation = self.advance_generation()?;
        for pattern in &patterns {
            let media = self.media.entry(pattern.key.clone()).or_insert_with(|| Media {
                providers: Vec::new(), cursor: 0, generation,
            });
            media.providers.push(provider.clone());
            media.cursor = 0;
            media.generation = generation;
        }
        self.providers.insert(provider, ProviderState { priority, patterns });
        Ok(())
    }

    /// Unmount at the actual service-remove callback. Missing nodes are a
    /// no-op in AE2's node-provider removal path.
    pub fn remove(&mut self, provider: &P) -> Result<bool, RegistryError> {
        let Some(state) = self.providers.get(provider).cloned() else { return Ok(false); };
        let generation = self.advance_generation()?;
        for pattern in &state.patterns {
            let media = self.media.get_mut(&pattern.key).expect("mounted provider has media");
            let index = media.providers.iter().position(|entry| entry == provider)
                .expect("mounted provider has a matching medium");
            media.providers.remove(index);
            media.cursor = 0;
            media.generation = generation;
            if media.providers.is_empty() { self.media.remove(&pattern.key); }
        }
        self.providers.remove(provider);
        Ok(true)
    }

    /// `requestUpdate` removes and re-adds the provider, resetting all affected
    /// cycles and moving it to the tail. Validate and stage before publishing.
    pub fn refresh(&mut self, provider: &P, priority: i32, patterns: Vec<Definition<D, K>>) -> Result<(), RegistryError> {
        let mut staged = self.clone();
        staged.remove(provider)?;
        staged.add(provider.clone(), priority, patterns)?;
        *self = staged;
        Ok(())
    }

    /// Distinct definitions ranked by their highest captured provider priority.
    /// A group with multiple entries needs a source-backed tie policy before a
    /// planner may claim a specific relative ordering.
    pub fn ranked_definitions(&self, output: &K) -> Vec<PriorityGroup<D>> {
        let mut priority_by_definition: HashMap<D, i32> = HashMap::new();
        for state in self.providers.values() {
            for pattern in &state.patterns {
                if &pattern.primary_output == output {
                    priority_by_definition.entry(pattern.key.clone())
                        .and_modify(|priority| *priority = (*priority).max(state.priority))
                        .or_insert(state.priority);
                }
            }
        }
        let mut groups: BTreeMap<Reverse<i32>, Vec<D>> = BTreeMap::new();
        for (definition, priority) in priority_by_definition {
            groups.entry(Reverse(priority)).or_default().push(definition);
        }
        groups.into_iter().map(|(Reverse(priority), definitions)| PriorityGroup { priority, definitions }).collect()
    }

    /// Snapshot every distinct definition for planning. Priority is the
    /// maximum captured on any medium. Within an equal-priority group this
    /// runtime uses definition order; Java's HashSet tie order is not portable.
    pub fn all_ranked(&self) -> Vec<(i32, Definition<D, K>)> where D: Ord {
        let mut ranked: HashMap<D, (i32, K)> = HashMap::new();
        for state in self.providers.values() {
            for pattern in &state.patterns {
                ranked.entry(pattern.key.clone())
                    .and_modify(|(priority, _)| *priority = (*priority).max(state.priority))
                    .or_insert_with(|| (state.priority, pattern.primary_output.clone()));
            }
        }
        let mut entries: Vec<_> = ranked.into_iter().map(|(key, (priority, primary_output))|
            (priority, Definition { key, primary_output })).collect();
        entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.key.cmp(&b.1.key)));
        entries
    }

    pub fn media_pass(&self, definition: &D) -> Option<MediaPass<D>> {
        let media = self.media.get(definition)?;
        Some(MediaPass { definition: definition.clone(), generation: media.generation, remaining: media.providers.len() })
    }
    /// Validate the exact limited pass before a selected medium commits.
    /// An intervening refresh invalidates the pass even if the same node is
    /// immediately re-registered with the same definition.
    pub fn pass_is_current_for(&self, pass: &MediaPass<D>, definition: &D) -> bool {
        &pass.definition == definition && self.media.get(definition).is_some_and(|media| media.generation == pass.generation)
    }
    /// Inspect the registered media without consuming their shared cycle.
    pub fn media_snapshot(&self, definition: &D) -> Vec<P> {
        self.media.get(definition).map_or_else(Vec::new, |media| media.providers.clone())
    }

    /// Advance the shared cycle once, even when the caller later rejects or
    /// skips the returned provider. A pass yields at most its initial length.
    pub fn next_medium(&mut self, pass: &mut MediaPass<D>) -> Result<Option<P>, RegistryError> {
        if pass.remaining == 0 { return Ok(None); }
        let media = self.media.get_mut(&pass.definition).ok_or(RegistryError::StaleMediaPass)?;
        if media.generation != pass.generation { return Err(RegistryError::StaleMediaPass); }
        let provider = media.providers[media.cursor].clone();
        media.cursor = (media.cursor + 1) % media.providers.len();
        pass.remaining -= 1;
        Ok(Some(provider))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern(key: &'static str) -> Definition<&'static str, &'static str> {
        Definition { key, primary_output: "diamond" }
    }
    fn one<'a>(registry: &mut ProviderRegistry<&str, &'a str, &str>, definition: &'a str) -> String {
        let mut pass = registry.media_pass(&definition).unwrap();
        registry.next_medium(&mut pass).unwrap().unwrap().to_owned()
    }

    // Mirrors evidence/java-ae2-provider-priority-oracle/result.json; this is
    // a registry-only proof with stub owners, not a physical-world claim.
    #[test]
    fn captured_priority_orders_distinct_definitions_and_refresh_reorders() {
        let mut registry = ProviderRegistry::default();
        registry.add("first-A", 1, vec![pattern("A")]).unwrap();
        registry.add("second-B", 10, vec![pattern("B")]).unwrap();
        registry.add("third-A", 100, vec![pattern("A")]).unwrap();
        assert_eq!(registry.ranked_definitions(&"diamond"), vec![
            PriorityGroup { priority: 100, definitions: vec!["A"] },
            PriorityGroup { priority: 10, definitions: vec!["B"] },
        ]);
        assert_eq!(registry.all_ranked(), vec![(100, pattern("A")), (10, pattern("B"))]);
        // An external priority change does nothing until an explicit refresh.
        assert_eq!(registry.ranked_definitions(&"diamond")[0].priority, 100);
        registry.refresh(&"third-A", -5, vec![pattern("A")]).unwrap();
        assert_eq!(registry.ranked_definitions(&"diamond"), vec![
            PriorityGroup { priority: 10, definitions: vec!["B"] },
            PriorityGroup { priority: 1, definitions: vec!["A"] },
        ]);
    }

    #[test]
    fn shared_media_cycles_lazily_and_add_remove_refresh_reset_it() {
        let mut registry = ProviderRegistry::default();
        registry.add("first-A", 1, vec![pattern("A")]).unwrap();
        registry.add("third-A", 100, vec![pattern("A")]).unwrap();
        assert_eq!(registry.media_snapshot(&"A"), vec!["first-A", "third-A"]);
        assert_eq!((0..5).map(|_| one(&mut registry, "A")).collect::<Vec<_>>(),
            ["first-A", "third-A", "first-A", "third-A", "first-A"]);
        let mut pass = registry.media_pass(&"A").unwrap();
        assert_eq!(registry.next_medium(&mut pass).unwrap(), Some("third-A")); // busy still consumes
        assert_eq!(registry.next_medium(&mut pass).unwrap(), Some("first-A"));
        assert_eq!(registry.next_medium(&mut pass).unwrap(), None); // one limited pass
        registry.refresh(&"third-A", -5, vec![pattern("A")]).unwrap();
        assert_eq!((0..4).map(|_| one(&mut registry, "A")).collect::<Vec<_>>(),
            ["first-A", "third-A", "first-A", "third-A"]);
        registry.remove(&"first-A").unwrap();
        assert_eq!(one(&mut registry, "A"), "third-A");
        registry.add("first-A", 1, vec![pattern("A")]).unwrap();
        assert_eq!((0..4).map(|_| one(&mut registry, "A")).collect::<Vec<_>>(),
            ["third-A", "first-A", "third-A", "first-A"]);
    }

    #[test]
    fn duplicate_slots_and_stale_passes_preserve_source_list_semantics() {
        let mut registry = ProviderRegistry::default();
        registry.add("p", 2, vec![pattern("A"), pattern("A")]).unwrap();
        registry.add("q", 1, vec![pattern("A")]).unwrap();
        let mut pass = registry.media_pass(&"A").unwrap();
        assert!(registry.pass_is_current_for(&pass, &"A"));
        assert!(!registry.pass_is_current_for(&pass, &"other"));
        assert_eq!(registry.next_medium(&mut pass).unwrap(), Some("p"));
        assert_eq!(registry.next_medium(&mut pass).unwrap(), Some("p"));
        registry.remove(&"p").unwrap();
        assert!(!registry.pass_is_current_for(&pass, &"A"));
        assert_eq!(registry.next_medium(&mut pass), Err(RegistryError::StaleMediaPass));
        assert_eq!(one(&mut registry, "A"), "q");
    }

    #[test]
    fn ties_remain_an_unordered_group_and_invalid_refresh_is_atomic() {
        let mut registry = ProviderRegistry::default();
        registry.add("p", 1, vec![pattern("A")]).unwrap();
        registry.add("q", 1, vec![pattern("B")]).unwrap();
        let group = &registry.ranked_definitions(&"diamond")[0];
        assert_eq!(group.priority, 1);
        assert_eq!(group.definitions.len(), 2);
        assert!(group.definitions.contains(&"A") && group.definitions.contains(&"B"));
        assert_eq!(registry.refresh(&"q", 7, vec![Definition { key: "A", primary_output: "other" }]),
            Err(RegistryError::InconsistentPrimaryOutput));
        assert_eq!(one(&mut registry, "B"), "q");
        assert!(registry.ranked_definitions(&"other").is_empty());
    }
}
