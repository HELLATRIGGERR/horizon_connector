//! Streaming with the client's flat generator (AE2's forced chunks).
    use crate::streaming::*;
    use crate::world::World;
    use crate::chunk::Chunk;
    use std::sync::Arc;
    use crate::{assets::Assets, biome::Biomes, flatgen::FlatTerrain, light::LightTables, mesher::MeshTables};
    use horizon_world::nbt::Tag;
    use crate::blocks::BlockFacts;

    #[test]
    #[ignore = "requires prepared models/textures in HORIZON_TEST_RESOURCES"]
    fn forced_flat_halo_finishes_and_lights_with_real_workers() -> crate::Result<()> {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").expect("HORIZON_TEST_RESOURCES"));
        let assets = Arc::new(Assets::load(&root)?);
        let render = Arc::new(assets.reload(&root)?);
        let facts = BlockFacts::load(&root, assets.registry)?;
        let biomes = Arc::new(Biomes::load(&root)?);
        let tables = Arc::new(MeshTables::new(&assets, &render, &facts, crate::models::load_visibility(&root, assets.registry)?)?);
        let context = Arc::new(WorkerContext { light: Arc::new(LightTables::load(&root, assets.registry, &facts)?), tables, assets, render, biomes: biomes.clone() });
        for force_radius in [1, 0] {
        let mut streaming = Streaming::new(Arc::new(FlatTerrain::void(biomes.id("minecraft:plains"))), context.clone(), 2);
        streaming.radius = 0;
        streaming.mesh = false;
        for z in 40-force_radius..=40+force_radius { for x in 40-force_radius..=40+force_radius { streaming.forced.insert((x,z)); } }
        let mut world = World::new(facts.air, 0);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(35);
        while std::time::Instant::now() < deadline {
            streaming.update(&mut world);
            if (38..=42).all(|z| (38..=42).all(|x| world.chunks.contains_key(&(x,z))))
                && (39..=41).all(|z| (39..=41).all(|x| world.chunks.get(&(x,z)).is_some_and(|c| c.lit))) {
                break;
            }
            streaming.wait(std::time::Duration::from_millis(5));
        }
        assert!((38..=42).all(|z| (38..=42).all(|x| world.chunks.contains_key(&(x,z)))) && (39..=41).all(|z| (39..=41).all(|x| world.chunks.get(&(x,z)).is_some_and(|c| c.lit))),
            "forced radius {force_radius} did not finish and light: loaded {:?}, proto {:?}, generating {:?}, finishing {:?}",
            (39..=41).flat_map(|z| (39..=41).map(move |x| (x,z))).filter(|p| world.chunks.contains_key(p)).collect::<Vec<_>>(),
            streaming.test_state().0.keys().copied().filter(|(x,z)| (38..=42).contains(x) && (38..=42).contains(z)).collect::<Vec<_>>(),
            streaming.test_state().1, streaming.test_state().2);
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires prepared models/textures in HORIZON_TEST_RESOURCES"]
    fn authoritative_flat_owns_contents_and_rejects_late_worker_results_after_unload() -> crate::Result<()> {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").expect("HORIZON_TEST_RESOURCES"));
        let assets=Arc::new(Assets::load(&root)?);
        let render=Arc::new(assets.reload(&root)?);
        let facts=BlockFacts::load(&root,assets.registry)?;
        let biomes=Arc::new(Biomes::load(&root)?);
        let tables=Arc::new(MeshTables::new(&assets,&render,&facts,crate::models::load_visibility(&root,assets.registry)?)?);
        let context=Arc::new(WorkerContext{light:Arc::new(LightTables::load(&root,assets.registry,&facts)?),tables,assets:assets.clone(),render,biomes:biomes.clone()});
        let mut streaming=Streaming::new(Arc::new(FlatTerrain::void(biomes.id("minecraft:plains"))),context,1);
        let mut world=World::new(facts.air,0);
        let stone=assets.registry.default_state("minecraft:stone").unwrap();
        let mut authoritative=Chunk::empty(0,0);
        authoritative.set(1,64,1,stone.0);
        authoritative.block_entities.push(((1,64,1),crate::containers::BlockEntity::Chest(vec![None;27])));
        authoritative.block_names.push(((1,64,1),"owned".into()));
        authoritative.entities.push(Tag::compound([("id",Tag::String("minecraft:pig".into()))]));
        streaming.test_generating().insert((0,0));
        streaming.insert_authoritative_flat(&mut world,authoritative).map_err(|e|format!("{e}"))?;
        assert_eq!(world.block(1,64,1),stone);
        assert!(matches!(world.block_entities.get(&(1,64,1)),Some(crate::containers::BlockEntity::Chest(_))));
        assert_eq!(world.block_names.get(&(1,64,1)).map(String::as_str),Some("owned"));
        assert_eq!(world.loaded_entities.len(),1);
        assert!(world.dirty_light.contains(&(0,0)));
        assert!(!world.chunks[&(0,0)].lit);
        assert!(streaming.insert_authoritative_flat(&mut world,Chunk::empty(0,0)).is_err());
        // A worker started before the synchronous install finishes after unload.
        // It must not recreate a stale column, names, BEs or entities.
        world.chunks.remove(&(0,0));
        let mut stale=Chunk::empty(0,0);
        stale.block_names.push(((1,64,1),"stale".into()));
        stale.entities.push(Tag::compound([("id",Tag::String("minecraft:cow".into()))]));
        streaming.test_receive(0,0,Some(stale),&mut world);
        assert!(!world.chunks.contains_key(&(0,0)));
        assert_eq!(world.block_names.get(&(1,64,1)).map(String::as_str),Some("owned"));
        assert_eq!(world.loaded_entities.len(),1);
        // A separate in-flight Generate returns a Proto rather than a Chunk.
        // It too must be discarded after an authoritative install and unload.
        streaming.test_generating().insert((1,0));
        streaming.insert_authoritative_flat(&mut world,Chunk::empty(1,0)).map_err(|e|format!("{e}"))?;
        world.chunks.remove(&(1,0));
        streaming.test_receive(1,0,None,&mut world);
        assert!(!streaming.test_state().0.contains_key(&(1,0)));
        assert!(!world.chunks.contains_key(&(1,0)));
        Ok(())
    }
