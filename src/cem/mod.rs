//! OptiFine custom entity models (M58): what Fresh Animations is made of.
//! `model` reads `.jem` and `.jpm` files, `expr` the animation expressions
//! (parsed once into bytecode), `vars` the entity variables, and `pose`
//! runs the animations and draws the parts. The models in the active
//! resource packs are kept in a `Registry` the mob renderer looks in.

pub mod expr;
pub mod model;
pub mod pose;
pub mod vars;

use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{Arc, RwLock},
};

use log::{info, warn};

pub use model::CemModel;
use pose::Runtime;

/// The models of the active packs, by mob name (`pig`, `baby_pig`...).
#[derive(Debug, Default)]
pub struct Registry {
    pub models: HashMap<String, Arc<CemModel>>,
}

impl Registry {
    /// The model for a mob called `name`, a baby's if `baby` and the pack
    /// has one.
    pub fn find(&self, name: &str, baby: bool) -> Option<&Arc<CemModel>> {
        if baby {
            let baby = self
                .models
                .get(&format!("{name}_baby"))
                .or_else(|| self.models.get(&format!("baby_{name}")));
            if baby.is_some() {
                return baby;
            }
        }
        self.models.get(name)
    }

    /// Loads the model of each mob name from the packs' `optifine/cem/`
    /// folder (top pack first), logging what each file skipped.
    pub fn load(bytes: &dyn Fn(&str) -> Option<Vec<u8>>, mobs: &[&str]) -> Self {
        let mut models = HashMap::new();
        let read = |path: &str| bytes(path).and_then(|b| String::from_utf8(b).ok());
        let mut unsupported: Vec<String> = Vec::new();
        for name in mobs {
            for file in [
                name.to_string(),
                format!("{name}_baby"),
                format!("baby_{name}"),
            ] {
                let path = format!("optifine/cem/{file}.jem");
                let Some(text) = read(&path) else {
                    continue;
                };
                let relative = |p: &str| read(&format!("optifine/cem/{p}"));
                let parsed = model::parse(&text, &relative, &mut |line| {
                    info!("{path}: {line}");
                });
                match parsed {
                    Ok(mut model) => {
                        model.baby = file != *name;
                        for n in &model.unknown {
                            if !unsupported.contains(n) {
                                unsupported.push(n.clone());
                            }
                        }
                        models.insert(file, Arc::new(model));
                    }
                    Err(err) => warn!("{path}: {err}"),
                }
            }
        }
        if !unsupported.is_empty() {
            unsupported.sort();
            info!(
                "custom entity models: names with no value here (they read 0): {}",
                unsupported.join(", ")
            );
        }
        if !models.is_empty() {
            info!("custom entity models: {} loaded", models.len());
        }
        Self { models }
    }
}

static ACTIVE: RwLock<Option<Arc<Registry>>> = RwLock::new(None);

/// Makes `registry` the models mobs are drawn with.
pub fn install(registry: Registry) {
    if let Ok(mut active) = ACTIVE.write() {
        *active = Some(Arc::new(registry));
    }
    STATES.with(|s| s.borrow_mut().clear());
}

/// The models in use, if any packs have some.
pub fn active() -> Option<Arc<Registry>> {
    ACTIVE.read().ok()?.clone().filter(|r| !r.models.is_empty())
}

thread_local! {
    /// The frame being drawn: game time, the player, how long the last
    /// frame took.
    static FRAME: RefCell<(vars::Frame, u64, Option<std::time::Instant>)> = const { RefCell::new((
        vars::Frame {
            time: 0.0,
            frame_time: 0.0,
            frame_counter: 0,
            player: glam::DVec3::ZERO,
            alpha: 0.0,
        },
        0,
        None,
    )) };
    /// What each mob keeps of its model, by mob id and model name.
    static STATES: RefCell<HashMap<(u64, String), Runtime>> = RefCell::new(HashMap::new());
}

/// Runs `f` on `mob_id`'s state for `model`, made if it's new, after
/// animating it for `entity`. `frame` counts frames (to forget mobs that
/// are gone).
pub fn with_state<T>(
    mob_id: u64,
    name: &str,
    model: &CemModel,
    entity: &vars::EntityVars,
    frame: u64,
    f: impl FnOnce(&Runtime) -> T,
) -> T {
    STATES.with(|states| {
        let mut states = states.borrow_mut();
        if frame.is_multiple_of(600) {
            states.retain(|_, r| frame.saturating_sub(r.last_used) < 600);
        }
        let runtime = states
            .entry((mob_id, name.to_owned()))
            .or_insert_with(|| Runtime::new(model, mob_id));
        runtime.last_used = frame;
        runtime.animate(model, entity);
        f(runtime)
    })
}

/// Starts a frame of drawing mobs: `time` in game ticks with the part of
/// the tick, `alpha` of the way between ticks, the player's position.
pub fn begin_frame(time: f32, alpha: f32, player: glam::DVec3) {
    FRAME.with(|f| {
        let mut f = f.borrow_mut();
        let now = std::time::Instant::now();
        let dt =
            f.2.map_or(0.0, |last| now.duration_since(last).as_secs_f32().min(0.25));
        f.2 = Some(now);
        f.1 += 1;
        f.0 = vars::Frame {
            time,
            frame_time: dt,
            frame_counter: f.1 as u32,
            player,
            alpha,
        };
    });
}

/// The frame being drawn and its number.
pub fn frame() -> (vars::Frame, u64) {
    FRAME.with(|f| {
        let f = f.borrow();
        (f.0, f.1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::visuals::packs::{TrailerPack, identify};

    const PIG: &str = r#"{"texture": "pig.png", "textureSize": [64, 32], "models": [
        {"part": "head", "id": "head", "boxes": [{"coordinates": [-4, -4, -8, 8, 8, 8], "textureOffset": [0, 0]}],
         "animations": [{"head.rx": "torad(head_pitch)", "var.x": "is_swimming + mystery"}]}]}"#;

    #[test]
    fn a_registry_finds_each_mobs_model_and_its_baby_one() {
        let bytes = |path: &str| -> Option<Vec<u8>> {
            match path {
                "optifine/cem/pig.jem"
                | "optifine/cem/cow_baby.jem"
                | "optifine/cem/baby_sheep.jem" => Some(PIG.as_bytes().to_vec()),
                "optifine/cem/zombie.jem" => Some(b"not json".to_vec()),
                _ => None,
            }
        };
        let registry = Registry::load(&bytes, &["pig", "cow", "sheep", "zombie", "creeper"]);
        assert!(registry.find("pig", false).is_some());
        assert!(!registry.find("pig", false).unwrap().baby);
        // A baby without its own file wears the adult's; with one, its own.
        assert!(!registry.find("pig", true).unwrap().baby);
        assert!(
            registry.find("cow", false).is_none(),
            "a baby file isn't the adult's"
        );
        assert!(registry.find("cow", true).unwrap().baby);
        assert!(registry.find("sheep", true).unwrap().baby);
        // A broken file and a mob with none are simply left out.
        assert!(registry.find("zombie", false).is_none());
        assert!(registry.find("creeper", false).is_none());
        // What the animations name that has no value here is kept.
        let unknown = &registry.find("pig", false).unwrap().unknown;
        assert!(unknown.contains(&"mystery".to_owned()), "{unknown:?}");
    }

    /// Every file of the Fresh Animations pack in resourcepacks/ loads
    /// with no errors. Skipped, with a message, where the pack isn't:
    /// its license forbids bundling it, so it isn't in the repository.
    #[test]
    fn every_fresh_animations_file_loads() {
        let dir = crate::pack::packs_dir();
        let Some(path) = std::fs::read_dir(&dir).ok().and_then(|entries| {
            entries.filter_map(|e| e.ok().map(|e| e.path())).find(|p| {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                identify(name, "") == Some(TrailerPack::FreshAnimations)
            })
        }) else {
            eprintln!("skipping: no Fresh Animations pack in {}", dir.display());
            return;
        };
        let Ok(file) = std::fs::File::open(&path) else {
            eprintln!("skipping: can't open {}", path.display());
            return;
        };
        let mut zip = zip::ZipArchive::new(file).expect("a zip");
        let names: Vec<String> = zip
            .file_names()
            .filter_map(Result::ok)
            .map(|n| n.into_owned())
            .collect();
        let mut text_of = |name: &str| -> Option<String> {
            let mut text = String::new();
            std::io::Read::read_to_string(&mut zip.by_name(name).ok()?, &mut text).ok()?;
            Some(text)
        };
        let jems: Vec<&String> = names
            .iter()
            .filter(|n| n.contains("optifine/cem/") && n.ends_with(".jem"))
            .collect();
        assert!(!jems.is_empty(), "no .jem files in {}", path.display());
        let mut failures = Vec::new();
        for jem in &jems {
            let Some(text) = text_of(jem) else {
                failures.push(format!("{jem}: can't be read"));
                continue;
            };
            let folder = jem[..jem.rfind('/').unwrap() + 1].to_owned();
            // Part files, read from a second handle on the same pack.
            let mut parts = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
            let relative = |p: &str| {
                let mut text = String::new();
                std::io::Read::read_to_string(
                    &mut parts.by_name(&format!("{folder}{p}")).ok()?,
                    &mut text,
                )
                .ok()?;
                Some(text)
            };
            let relative = std::cell::RefCell::new(relative);
            if let Err(err) = model::parse(&text, &|p| (relative.borrow_mut())(p), &mut |_| {}) {
                failures.push(format!("{jem}: {err}"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        println!("{} Fresh Animations models load", jems.len());
    }
}
