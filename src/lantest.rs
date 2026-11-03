//! `--lan-test <n>`: a server on a generated world with n bots joined over
//! TCP on this machine, walking, mining and placing for 60 s. Prints how
//! long the server's ticks took and how much went over the network, to
//! check a full world keeps up (ticks under 50 ms with 8 bots).

use std::{
    net::TcpStream,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

use anyhow::Result;
use glam::DVec3;

use crate::{
    assets,
    bindings::Action,
    entity::mobs::MobRegistry,
    game::Game,
    inventory::{crafting::Recipes, items, stack::ItemStack, stack::Items},
    net::{
        protocol::{ClientMsg, Input, PROTOCOL_VERSION, PlayerSettings, ServerMsg},
        transport::{Connection, Tcp, Traffic},
    },
    player::{input::MoveKeys, mode::GameMode},
    server::Server,
    world::{
        biome::BiomeColors,
        manager::{ChunkManager, ChunkSource},
        mesh::BlockModels,
        worldgen::terrain::Terrain,
    },
};

/// How long the bots play, and the view distance they ask for.
pub const SECONDS: f32 = 60.0;
const VIEW_DISTANCE: i32 = 6;
const TICK: f32 = 0.05;

/// What `--lan-test` measured.
pub struct Report {
    pub bots: usize,
    pub ticks: usize,
    pub average_ms: f64,
    pub p99_ms: f64,
    pub worst_ms: f64,
    /// Bytes a second the server sent and received, all bots together.
    pub sent_per_second: f64,
    pub received_per_second: f64,
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(
            f,
            "lan test: {} bots for {SECONDS} s, {} ticks",
            self.bots, self.ticks
        )?;
        writeln!(
            f,
            "  server tick: {:.2} ms average, {:.2} ms 99th percentile, {:.2} ms worst (limit 50)",
            self.average_ms, self.p99_ms, self.worst_ms
        )?;
        write!(
            f,
            "  network: {:.1} KiB/s sent, {:.1} KiB/s received ({:.1} KiB/s a bot down)",
            self.sent_per_second / 1024.0,
            self.received_per_second / 1024.0,
            self.sent_per_second / 1024.0 / self.bots.max(1) as f64
        )
    }
}

/// A bot: its connection and how it moves.
struct Bot {
    conn: Tcp<ClientMsg, ServerMsg>,
    seq: u32,
    yaw: f32,
}

pub fn run(bots: usize) -> Result<Report> {
    let dir = assets::dir();
    let recipes = Recipes::load(&dir.join("data/recipes.ron"))?;
    let items = Items::new().with_recipes(Arc::new(recipes));
    let mobs = Arc::new(MobRegistry::load(&dir.join("data/mobs.ron"))?);
    // The server draws nothing, so its block models need no textures.
    let models = Arc::new(BlockModels::new(
        crate::world::block::registry(),
        |_| Some([0.0; 4]),
        [0.0; 4],
        BiomeColors::uniform(),
    ));
    let terrain = Terrain::new(crate::bench::SEED);
    let (x, y, z) = terrain.spawn_point();
    let spawn = DVec3::new(x as f64 + 0.5, y as f64, z as f64 + 0.5);
    let chunks = ChunkManager::new(
        ChunkSource::Terrain(Box::new(terrain)),
        models,
        VIEW_DISTANCE,
        crate::game::chunk_workers(),
    );
    let world = Game::new(chunks, items, spawn, spawn, 0.0, 0.0).with_mobs(mobs);
    let mut server = Server::new(world);
    let port = server.open_to_lan(0, GameMode::Survival, false)?;
    let mut bots: Vec<Bot> = (0..bots)
        .map(|i| {
            let mut conn = Tcp::new(TcpStream::connect(("127.0.0.1", port))?)?;
            conn.send(ClientMsg::Login {
                version: PROTOCOL_VERSION,
                name: format!("Bot_{i}"),
                mods: crate::mods::active().required(),
            });
            conn.send(ClientMsg::Settings(PlayerSettings {
                view_distance: VIEW_DISTANCE,
                auto_jump: true,
                toggle_sneak: false,
                toggle_sprint: false,
                left_handed: false,
                skin_layers: crate::render::player::ALL_LAYERS,
                particles: crate::options::Particles::All,
            }));
            Ok(Bot {
                conn,
                seq: 0,
                yaw: i as f32 * 0.8,
            })
        })
        .collect::<Result<_>>()?;
    let traffic: Vec<Arc<Traffic>> = bots.iter().map(|bot| bot.conn.traffic.clone()).collect();
    // Everyone in before the clock starts.
    let deadline = Instant::now() + Duration::from_secs(30);
    while server.player_count() < bots.len() {
        anyhow::ensure!(Instant::now() < deadline, "the bots never all joined");
        server.advance(0.0);
        std::thread::sleep(Duration::from_millis(5));
    }
    // Each bot carries stone to build with (the first in is the world's
    // own player).
    let world = &mut server.game;
    for player in std::iter::once(&mut world.player).chain(world.others.iter_mut()) {
        player
            .inventory
            .set(0, Some(ItemStack::new(items::STONE, 64)));
    }
    let start = Instant::now();
    let mut times = Vec::new();
    let mut tick = 0u32;
    while start.elapsed().as_secs_f32() < SECONDS {
        let frame = Instant::now();
        for (i, bot) in bots.iter_mut().enumerate() {
            play(bot, tick, i);
            while bot.conn.try_recv().is_some() {}
        }
        let began = Instant::now();
        server.advance(TICK);
        times.push(began.elapsed().as_secs_f64() * 1000.0);
        tick += 1;
        if let Some(rest) = Duration::from_secs_f32(TICK).checked_sub(frame.elapsed()) {
            std::thread::sleep(rest);
        }
    }
    let seconds = start.elapsed().as_secs_f64();
    let sum = |f: fn(&Traffic) -> u64| traffic.iter().map(|t| f(t) as f64).sum::<f64>();
    let received = sum(|t| t.sent.load(Ordering::Relaxed));
    let sent = sum(|t| t.received.load(Ordering::Relaxed));
    times.sort_by(f64::total_cmp);
    let average = times.iter().sum::<f64>() / times.len().max(1) as f64;
    let p99 = times
        .get((times.len() as f64 * 0.99) as usize)
        .copied()
        .unwrap_or(0.0);
    Ok(Report {
        bots: bots.len(),
        ticks: times.len(),
        average_ms: average,
        p99_ms: p99,
        worst_ms: times.last().copied().unwrap_or(0.0),
        sent_per_second: sent / seconds,
        received_per_second: received / seconds,
    })
}

/// A bot's tick: it walks forward, turning now and then, and every few
/// seconds looks down to dig the block underfoot or put stone down.
fn play(bot: &mut Bot, tick: u32, index: usize) {
    let phase = (tick as usize + index * 17) % 100;
    if phase == 0 {
        bot.yaw += 1.3;
    }
    let looking_down = (60..80).contains(&phase);
    bot.seq += 1;
    let keys = MoveKeys {
        forward: !looking_down,
        jump: phase.is_multiple_of(23),
        ..MoveKeys::default()
    };
    bot.conn.send(ClientMsg::Input(Input {
        seq: bot.seq,
        keys,
        yaw: bot.yaw,
        pitch: if looking_down { -1.4 } else { 0.0 },
    }));
    let press = |bot: &mut Bot, action, pressed| {
        bot.conn.send(ClientMsg::Action {
            action,
            pressed,
            repeat: false,
            grabbed: true,
        });
    };
    match phase {
        60 => press(bot, Action::Attack, true),
        70 => {
            press(bot, Action::Attack, false);
            press(bot, Action::Use, true);
        }
        80 => press(bot, Action::Use, false),
        _ => {}
    }
}
