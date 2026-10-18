//! M4 "done when" stand-ins: dig a staircase down and wall yourself in on
//! real flat-world chunks, aiming with the raycast and moving with physics,
//! checking every tick that the player never overlaps a solid block.

use glam::{DVec3, IVec3};

use super::{
    collision::{Aabb, Collider, WorldCollider},
    interaction::{break_block, place_block},
    physics::{Body, MoveInput},
    raycast::{Hit, raycast},
};
use crate::{
    config::REACH,
    world::{
        World,
        block::{BlockRegistry, BlockState},
        chunk::ChunkPos,
        worldgen::flat::flat_chunk,
    },
};

const COBBLESTONE: BlockState = BlockState::of(crate::world::blocks::COBBLESTONE);

struct Scene {
    world: World,
    blocks: &'static BlockRegistry,
    body: Body,
}

impl Scene {
    /// Standing on the grass (y 63) of a 5×5-chunk flat world at the origin.
    fn new() -> Self {
        let mut world = World::default();
        for z in -2..=2 {
            for x in -2..=2 {
                world.insert(flat_chunk(ChunkPos::new(x, z)));
            }
        }
        let blocks = crate::world::block::registry();
        let mut body = Body::new(DVec3::new(0.5, 64.0, 0.5));
        body.on_ground = true;
        Self {
            world,
            blocks,
            body,
        }
    }

    fn eye(&self) -> DVec3 {
        self.body.position() + DVec3::Y * self.body.eye_height()
    }

    /// Looks at `point` and returns what the crosshair hits.
    fn aim(&self, point: DVec3) -> Option<Hit> {
        let (world, blocks) = (&self.world, self.blocks);
        raycast(self.eye(), point - self.eye(), REACH, |cell| {
            world
                .block(cell.x, cell.y, cell.z)
                .and_then(|block| blocks.get(block.id()))
                .is_some_and(|def| def.is_targetable())
        })
    }

    fn apply(&mut self, edits: &[(IVec3, BlockState)]) {
        for &(at, block) in edits {
            let chunk = self.world.get_mut(ChunkPos::of_block(at.x, at.z)).unwrap();
            chunk.set(
                at.x.rem_euclid(16) as usize,
                at.y as usize,
                at.z.rem_euclid(16) as usize,
                block,
            );
        }
    }

    /// Aims at the center of `cell` and breaks it; it must be what's hit.
    fn dig(&mut self, cell: IVec3) {
        let hit = self.aim(cell.as_dvec3() + 0.5).expect("block in reach");
        assert_eq!(hit.block, cell, "something else is in the way");
        let edits = break_block(&self.world, self.blocks, hit.block);
        assert!(!edits.is_empty());
        self.apply(&edits);
    }

    /// Places cobblestone into `cell` by clicking the top of the block below it.
    fn build_on_top_of(&mut self, below: IVec3) -> Option<IVec3> {
        let face_center = below.as_dvec3() + DVec3::new(0.5, 0.99, 0.5);
        let hit = self.aim(face_center)?;
        assert_eq!(hit.block, below, "something else is in the way");
        let (at, block) = place_block(
            &self.world,
            self.blocks,
            &hit,
            COBBLESTONE,
            &self.body.aabb(),
        )?;
        self.apply(&[(at, block)]);
        Some(at)
    }

    /// Runs `ticks` ticks of `input` facing `yaw`, checking for clipping.
    fn run(&mut self, input: MoveInput, yaw: f32, ticks: u32) {
        for _ in 0..ticks {
            let collider = WorldCollider {
                world: &self.world,
                blocks: self.blocks,
            };
            self.body.tick(&input, yaw, &collider);
            let bb = self.body.aabb();
            let mut boxes = Vec::new();
            collider.boxes(&bb, &mut boxes);
            let clipping: Vec<&Aabb> = boxes.iter().filter(|b| b.intersects(&bb)).collect();
            assert!(
                clipping.is_empty(),
                "player at {} clips {clipping:?}",
                self.body.position()
            );
        }
    }
}

const FORWARD: MoveInput = MoveInput {
    forward: 1.0,
    strafe: 0.0,
    jump: false,
    sneak: false,
    sprint: false,
};

#[test]
fn dig_a_staircase_down() {
    let mut scene = Scene::new();
    // Facing north (yaw 0), dig three blocks ahead (top first), walk into the
    // gap and drop one step, eight times.
    for step in 0..8 {
        let feet = 64 - step;
        let z = -(step + 1);
        for y in [feet + 1, feet, feet - 1] {
            let cell = IVec3::new(0, y, z);
            if !scene.world.block(cell.x, cell.y, cell.z).unwrap().is_air() {
                scene.dig(cell);
            }
        }
        scene.run(FORWARD, 0.0, 20);
        let p = scene.body.position();
        assert_eq!(p.y, (feet - 1) as f64, "step {step}");
        // Stopped by the next step's wall.
        assert!((p.z - (z as f64 + 0.3)).abs() < 1e-9, "step {step}: {p}");
        assert!(scene.body.on_ground);
    }
    // And climb back out by jumping up the steps.
    let climb = MoveInput {
        jump: true,
        ..FORWARD
    };
    scene.run(climb, std::f32::consts::PI, 80);
    scene.run(MoveInput::default(), 0.0, 20);
    assert_eq!(scene.body.position().y, 64.0);
}

#[test]
fn wall_yourself_in() {
    let mut scene = Scene::new();
    // Two-high walls on the eight cells around the player, corners first so
    // the sides don't block the view of them.
    let mut ring: Vec<IVec3> = (-1..=1)
        .flat_map(|z| (-1..=1).map(move |x| IVec3::new(x, 0, z)))
        .filter(|cell| *cell != IVec3::ZERO)
        .collect();
    ring.sort_by_key(|cell| -(cell.x.abs() + cell.z.abs()));
    for layer in [63, 64] {
        for &cell in &ring {
            let below = cell + IVec3::Y * layer;
            assert_eq!(scene.build_on_top_of(below), Some(below + IVec3::Y));
        }
    }
    // The player's own cell can't be filled.
    assert_eq!(scene.build_on_top_of(IVec3::new(0, 63, 0)), None);

    // Walking and jumping in every direction stays inside.
    for quarter in 0..4 {
        let yaw = quarter as f32 * std::f32::consts::FRAC_PI_2;
        let input = MoveInput {
            jump: true,
            sprint: true,
            ..FORWARD
        };
        scene.run(input, yaw, 40);
        let p = scene.body.position();
        assert!(p.x.abs() < 1.0 && p.z.abs() < 1.0, "escaped to {p}");
    }
    scene.run(MoveInput::default(), 0.0, 20);
    assert_eq!(scene.body.position().y, 64.0);
}
