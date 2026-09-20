use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::input::mouse::MouseMotion;
use bevy::math::Affine2;
use bevy::core_pipeline::bloom::Bloom;
use bevy::core_pipeline::fxaa::Fxaa;
use bevy::pbr::{
    CascadeShadowConfigBuilder, DirectionalLightShadowMap, DistanceFog, FogFalloff,
    NotShadowCaster, NotShadowReceiver, ScreenSpaceAmbientOcclusion,
};
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::{CursorGrabMode, PrimaryWindow};
use serde::{Deserialize, Serialize};
use std::f32::consts::{PI, TAU};

// World layout (meters). Safe zone is x < LINE_X, egg zone is x > LINE_X.
const WORLD_X: f32 = 150.0;
const WORLD_Z: f32 = 124.0;
const LINE_X: f32 = 65.0;
const SPAWN: Vec3 = Vec3::new(56.0, 1.7, 24.5);
const EYE: f32 = 1.7;

const COLOR_NAMES: [&str; 13] = [
    "White", "Brown", "Mint", "Sky", "Rose", "Lemon", "Lava", "Ocean", "Violet", "Slime",
    "Midnight", "Golden", "Rainbow",
];
const PATTERN_NAMES: [&str; 13] = [
    "Plain", "Spotted", "Striped", "Starry", "Cracked", "Swirly", "Checker", "Glossy", "Fuzzy",
    "Crystal", "Camo", "Royal", "Hacker",
];
const EGG_RGB: [(f32, f32, f32); 13] = [
    (0.96, 0.96, 0.92),
    (0.72, 0.52, 0.34),
    (0.62, 0.93, 0.75),
    (0.55, 0.80, 0.98),
    (0.98, 0.66, 0.76),
    (0.99, 0.95, 0.55),
    (0.95, 0.42, 0.22),
    (0.15, 0.45, 0.75),
    (0.65, 0.45, 0.90),
    (0.55, 0.85, 0.25),
    (0.24, 0.24, 0.42),
    (1.00, 0.82, 0.20),
    (0.95, 0.35, 0.85),
];

const T_NAMES: [&str; 7] = ["Dirt", "Copper", "Iron", "Gold", "Diamond", "Pro", "Hacker"];
const T_COSTS: [f64; 7] = [0.0, 100.0, 400.0, 1500.0, 6000.0, 25000.0, 100000.0];
const T_MULT: [f32; 7] = [1.0, 2.0, 4.0, 8.0, 16.0, 40.0, 150.0];
const T_RGB: [(f32, f32, f32); 7] = [
    (0.45, 0.32, 0.18),
    (0.80, 0.45, 0.20),
    (0.62, 0.64, 0.68),
    (0.95, 0.78, 0.15),
    (0.45, 0.90, 0.95),
    (0.60, 0.25, 0.90),
    (0.10, 0.90, 0.30),
];

// Your yard: x 13..43, z 12..37. Treadmill just east of it, off the gate path.
const YARD: (f32, f32, f32, f32) = (13.0, 43.0, 12.0, 37.0);
const YARD_GAP: f32 = 27.0; // z spacing between the four yards
const INV_SLOTS: usize = 10;

// day/night cycle on the wall clock, so it keeps running while the game is closed
const DAY_SECS: f64 = 30.0 * 60.0;
const NIGHT_SECS: f64 = 5.0 * 60.0;
// hatch times: a plain white egg takes 5 minutes, a Hacker Rainbow egg an hour
const HATCH_MIN_SECS: f64 = 5.0 * 60.0;
const HATCH_MAX_SECS: f64 = 60.0 * 60.0;
const YARD_MAX_SLOTS: usize = 48;

// Gear Station / Upgrader, north of the gate path (the treadmill is south of it)
const GEAR_POS: Vec3 = Vec3::new(48.6, 0.0, 18.0);
const GEAR_NAMES: [&str; 3] = ["Freeze Gun", "Ice Sword", "Blizzard Orb"];
const GEAR_COSTS: [f64; 3] = [1500.0, 6000.0, 25000.0];
const GEAR_FREEZE: [f32; 3] = [10.0, 20.0, 30.0];
const GEAR_RANGE: [f32; 3] = [30.0, 8.0, f32::MAX];
const GEAR_COOLDOWN: [f32; 3] = [12.0, 20.0, 60.0];
const GEAR_KEYS: [KeyCode; 3] = [KeyCode::KeyF, KeyCode::KeyG, KeyCode::KeyH];
const GEAR_KEY_NAMES: [&str; 3] = ["F", "G", "H"];

fn now_unix() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn is_night_at(now: f64) -> bool {
    // EGG_NIGHT=1 / EGG_NIGHT=0 forces the phase (handy for previewing the night wall)
    static FORCE: std::sync::OnceLock<Option<bool>> = std::sync::OnceLock::new();
    let force = FORCE.get_or_init(|| match std::env::var("EGG_NIGHT").as_deref() {
        Ok("1") => Some(true),
        Ok("0") => Some(false),
        _ => None,
    });
    force.unwrap_or_else(|| now.rem_euclid(DAY_SECS + NIGHT_SECS) >= DAY_SECS)
}

// seconds until the current phase (day or night) ends
fn phase_left(now: f64) -> f64 {
    let c = now.rem_euclid(DAY_SECS + NIGHT_SECS);
    if c < DAY_SECS {
        DAY_SECS - c
    } else {
        DAY_SECS + NIGHT_SECS - c
    }
}

// what an egg pays out when it finishes hatching
fn hatch_payout(kind: usize, level: usize) -> f64 {
    egg_value(kind) * 200.0 * level_mult(level)
}

fn hatch_secs(kind: usize) -> f64 {
    HATCH_MIN_SECS + (HATCH_MAX_SECS - HATCH_MIN_SECS) * (egg_value(kind) - 1.0) / 168.0
}

fn fmt_dur(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

// every yard has its own Gear Station at the same relative spot
fn gear_pos(yi: usize) -> Vec3 {
    Vec3::new(GEAR_POS.x, 0.0, GEAR_POS.z + yi as f32 * YARD_GAP)
}

fn near_gear(p: Vec3) -> bool {
    (0..4).any(|yi| {
        let g = gear_pos(yi);
        Vec2::new(p.x - g.x, p.z - g.z).length() < 4.0
    })
}

// where the n-th egg/pet stands in your yard (8 per row)
fn slot_pos(slot: usize) -> Vec3 {
    let (col, row) = (slot % 8, slot / 8);
    Vec3::new(15.5 + col as f32 * 3.5, 0.0, 14.5 + row as f32 * 3.6)
}
const TM_CENTER: Vec3 = Vec3::new(48.6, 0.0, 31.0);
// every yard has a portal at its back (west) fence, centred in z
fn portal_pos(yi: usize) -> Vec3 {
    Vec3::new(
        YARD.0 + 2.0,
        0.0,
        YARD.2 + yi as f32 * YARD_GAP + (YARD.3 - YARD.2) / 2.0,
    )
}
const N_WORLDS: usize = 10;
// cost to unlock world 2..=10 (index = world - 2)
const LEVEL_COST: [f64; N_WORLDS - 1] = [
    1_000_000.0,
    2_000_000.0,
    5_000_000.0,
    10_000_000.0,
    25_000_000.0,
    50_000_000.0,
    100_000_000.0,
    250_000_000.0,
    500_000_000.0,
];
// reach this much money in World 10 to beat the game
const WIN_MONEY: f64 = 1_000_000_000.0;

fn level_mult(level: usize) -> f64 {
    [
        1.0, 5.0, 25.0, 100.0, 500.0, 2_000.0, 8_000.0, 30_000.0, 100_000.0, 500_000.0,
    ][level - 1]
}

// the next world you have not unlocked yet, if any
fn next_locked(unlocked: &[bool; N_WORLDS]) -> Option<usize> {
    (2..=N_WORLDS).find(|&w| !unlocked[w - 1])
}

fn fmt_money(v: f64) -> String {
    let n = v.max(0.0).floor() as u64;
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn near_portal(p: Vec3) -> bool {
    (0..4).any(|yi| {
        let pp = portal_pos(yi);
        Vec2::new(p.x - pp.x, p.z - pp.z).length() < 3.0
    })
}

fn egg_value(kind: usize) -> f64 {
    ((kind / 13 + 1) * (kind % 13 + 1)) as f64
}
fn egg_name(kind: usize) -> String {
    format!("{} {} Egg", PATTERN_NAMES[kind % 13], COLOR_NAMES[kind / 13])
}
fn rarity(kind: usize) -> (&'static str, Color) {
    let v = egg_value(kind);
    if v < 10.0 {
        ("Common", Color::srgb(0.85, 0.85, 0.85))
    } else if v < 30.0 {
        ("Uncommon", Color::srgb(0.4, 0.95, 0.4))
    } else if v < 60.0 {
        ("Rare", Color::srgb(0.4, 0.75, 1.0))
    } else if v < 100.0 {
        ("Epic", Color::srgb(0.8, 0.5, 1.0))
    } else if v < 150.0 {
        ("Legendary", Color::srgb(1.0, 0.65, 0.2))
    } else {
        ("MYTHIC", Color::srgb(1.0, 0.3, 0.3))
    }
}
fn random_kind() -> usize {
    let r = fastrand::f32();
    ((r * r * r) * 168.9) as usize
}
fn random_egg_xz() -> (f32, f32) {
    (
        LINE_X + 7.0 + fastrand::f32() * (WORLD_X - 6.0 - LINE_X - 7.0),
        6.0 + fastrand::f32() * (WORLD_Z - 12.0),
    )
}

#[derive(Component)]
struct Player {
    yaw: f32,
    pitch: f32,
}

#[derive(Component)]
struct WorldEgg {
    kind: usize,
}

#[derive(Component)]
struct CarriedEgg;

#[derive(Component)]
struct Hatching {
    kind: usize,
    slot: usize,
    hatch_at: f64, // unix seconds
}

#[derive(Component)]
struct IceBlock;

#[derive(Component)]
struct NightWall;

#[derive(Component)]
struct ShellBit {
    t: f32,
}

#[derive(Component)]
struct PortalDisc;

#[derive(Component)]
struct SlotUi(usize);

#[derive(Component)]
struct SlotText(usize);

#[derive(Component)]
struct MonsterLimb {
    base_z: f32,
    phase: f32,
}

#[derive(Component)]
struct PortalMenu;

#[derive(Component)]
struct MainMenu;

#[derive(Component)]
struct MenuButton(usize); // 0 online, 1 multiplayer, 2 single player

#[derive(Component)]
struct MenuStatus;

#[derive(Component)]
struct PauseMenu;

#[derive(Component)]
struct PauseButton(usize); // 0 continue, 1 quit and save

#[derive(Component)]
struct WorldButton(usize);

#[derive(Component)]
struct WorldButtonText(usize);

// Solid obstacles in the XZ plane (axis-aligned boxes): fence rails and treadmill frames.
#[derive(Clone, Copy)]
struct Blocker {
    x0: f32,
    x1: f32,
    z0: f32,
    z1: f32,
}

const PLAYER_RADIUS: f32 = 0.32;
const FENCE_HALF: f32 = 0.08;
const TM_BELT_L: f32 = 2.8; // belt length (x)
const TM_BELT_W: f32 = 1.6; // belt width (z)
const TM_DECK_H: f32 = 0.27; // top of the belt

fn treadmill_center(yi: usize) -> Vec3 {
    Vec3::new(TM_CENTER.x, 0.0, TM_CENTER.z + yi as f32 * YARD_GAP)
}

fn blockers() -> Vec<Blocker> {
    let mut v = Vec::new();
    let mut wall = |vertical: bool, line: f32, a: f32, b: f32| {
        if vertical {
            v.push(Blocker { x0: line - FENCE_HALF, x1: line + FENCE_HALF, z0: a, z1: b });
        } else {
            v.push(Blocker { x0: a, x1: b, z0: line - FENCE_HALF, z1: line + FENCE_HALF });
        }
    };
    for yi in 0..4 {
        let (x0, x1) = (YARD.0, YARD.1);
        let z0 = YARD.2 + yi as f32 * YARD_GAP;
        let z1 = z0 + (YARD.3 - YARD.2);
        let gate = (z0 + z1) / 2.0;
        wall(false, z0, x0, x1);
        wall(false, z1, x0, x1);
        wall(true, x0, z0, z1);
        // east side has a 4 m gate gap in the middle
        wall(true, x1, z0, gate - 2.0);
        wall(true, x1, gate + 2.0, z1);
    }
    for yi in 0..4 {
        // treadmill: side frames and the console at the west end; you step on from the east
        let c = treadmill_center(yi);
        let hl = TM_BELT_L / 2.0 + 0.1;
        let hw = TM_BELT_W / 2.0;
        for side in [-1.0, 1.0] {
            let zc = c.z + side * (hw + 0.06);
            v.push(Blocker { x0: c.x - hl, x1: c.x + hl, z0: zc - 0.06, z1: zc + 0.06 });
        }
        v.push(Blocker {
            x0: c.x - hl - 0.16,
            x1: c.x - hl + 0.06,
            z0: c.z - hw - 0.12,
            z1: c.z + hw + 0.12,
        });
    }
    // gear station kiosks
    for yi in 0..4 {
        let g = gear_pos(yi);
        v.push(Blocker {
            x0: g.x - 1.1,
            x1: g.x + 1.1,
            z0: g.z - 0.9,
            z1: g.z + 0.9,
        });
    }
    v
}

// Push a circle of radius `r` out of every blocker it overlaps.
fn resolve_blockers(p: &mut Vec3, r: f32, blockers: &[Blocker]) {
    for b in blockers {
        let cx = p.x.clamp(b.x0, b.x1);
        let cz = p.z.clamp(b.z0, b.z1);
        let dx = p.x - cx;
        let dz = p.z - cz;
        let d2 = dx * dx + dz * dz;
        if d2 >= r * r {
            continue;
        }
        if d2 > 1e-6 {
            let d = d2.sqrt();
            let push = r - d;
            p.x += dx / d * push;
            p.z += dz / d * push;
        } else {
            // centre is inside the box: leave through the nearest face
            let lx = p.x - b.x0;
            let rx = b.x1 - p.x;
            let lz = p.z - b.z0;
            let rz = b.z1 - p.z;
            let m = lx.min(rx).min(lz).min(rz);
            if m == lx {
                p.x = b.x0 - r;
            } else if m == rx {
                p.x = b.x1 + r;
            } else if m == lz {
                p.z = b.z0 - r;
            } else {
                p.z = b.z1 + r;
            }
        }
    }
}

// Move `p` by `delta`, sub-stepping so a fast runner can never tunnel through a rail.
fn move_blocked(p: &mut Vec3, delta: Vec3, blockers: &[Blocker]) {
    let total = delta.length();
    let steps = (total / 0.25).ceil().max(1.0) as usize;
    let step = delta / steps as f32;
    for _ in 0..steps {
        *p += step;
        resolve_blockers(p, PLAYER_RADIUS, blockers);
    }
}

fn on_deck(p: Vec3) -> bool {
    (0..4).any(|yi| {
        let c = treadmill_center(yi);
        (p.x - c.x).abs() < TM_BELT_L / 2.0 && (p.z - c.z).abs() < TM_BELT_W / 2.0
    })
}

#[derive(Component)]
struct Monster {
    home: Vec3,
    speed: f32,
    scale: f32,
    awake: f32,
    phase: f32,
    frozen: f32,
}

#[derive(Component)]
struct MonsterEye;

#[derive(Component)]
enum Hud {
    Money,
    Speed,
    Stats,
    Carry,
    Prompt,
    Msg,
    Clock,
    Gear,
    Night,
}

#[derive(Resource)]
struct Game {
    in_menu: bool,
    paused: bool,
    money: f64,
    training: f32,
    tier: usize,
    inventory: [Option<usize>; INV_SLOTS],
    selected: usize,
    held_kind: Option<usize>,
    level: usize,
    unlocked: [bool; N_WORLDS],
    menu_open: bool,
    won: bool,
    portal_lit: bool,
    yard_eggs: Vec<usize>,
    msg: String,
    msg_color: Color,
    msg_t: f32,
    spawn_t: f32,
    egg_mesh: Handle<Mesh>,
    egg_mats: Vec<Handle<StandardMaterial>>,
    eye_open: Handle<StandardMaterial>,
    eye_closed: Handle<StandardMaterial>,
    belt_mat: Handle<StandardMaterial>,
    panel_mat: Handle<StandardMaterial>,
    portal_mat: Handle<StandardMaterial>,
    safe_grass_mat: Handle<StandardMaterial>,
    zone_grass_mat: Handle<StandardMaterial>,
    sphere_mesh: Handle<Mesh>,
    defeat_sound: Handle<AudioSource>,
    pickup_sound: Handle<AudioSource>,
    on_treadmill: bool,
    gear: usize,
    gear_cd: [f32; 3],
    night: bool,
    night_applied: Option<bool>,
    ice_mat: Handle<StandardMaterial>,
    cube_mesh: Handle<Mesh>,
    eye_h: f32,
    sky_mesh: Handle<Mesh>,
    sun_mat: Handle<StandardMaterial>,
}

impl Game {
    fn carrying(&self) -> usize {
        self.inventory.iter().flatten().count()
    }
    fn has_eggs(&self) -> bool {
        self.inventory.iter().any(|s| s.is_some())
    }
    fn carried(&self) -> impl Iterator<Item = usize> + '_ {
        self.inventory.iter().flatten().copied()
    }
    fn take_all(&mut self) -> Vec<usize> {
        let v: Vec<usize> = self.carried().collect();
        self.inventory = [None; INV_SLOTS];
        v
    }
    // the selected slot if it is free, otherwise the first free slot
    fn free_slot(&self) -> Option<usize> {
        if self.inventory[self.selected].is_none() {
            return Some(self.selected);
        }
        self.inventory.iter().position(|s| s.is_none())
    }
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct SaveData {
    money: f64,
    training: f32,
    tier: usize,
    level: usize,
    unlocked: Vec<bool>,
    won: bool,
    inventory: Vec<Option<usize>>,
    gear: usize,
    yard_eggs: Vec<usize>,              // kinds hatched so far (for the Types count)
    hatching: Vec<(usize, usize, f64)>, // (kind, slot, hatch_at)
}

fn save_path() -> std::path::PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        let dir = std::path::Path::new(&home).join("Library/Application Support/EggStealer");
        if std::fs::create_dir_all(&dir).is_ok() {
            return dir.join("save.json");
        }
    }
    std::path::PathBuf::from("eggstealer_save.json")
}

fn load_save() -> Option<SaveData> {
    let text = std::fs::read_to_string(save_path()).ok()?;
    serde_json::from_str(&text).ok()
}

fn spawn_hatching(commands: &mut Commands, game: &Game, kind: usize, slot: usize, hatch_at: f64) {
    let pos = slot_pos(slot);
    commands.spawn((
        Mesh3d(game.egg_mesh.clone()),
        MeshMaterial3d(game.egg_mats[kind].clone()),
        Transform::from_xyz(pos.x, 0.36, pos.z)
            .with_scale(Vec3::splat(0.55))
            .with_rotation(Quat::from_rotation_y(fastrand::f32() * TAU)),
        Hatching {
            kind,
            slot,
            hatch_at,
        },
    ));
}

fn panel_colors(tier: usize) -> (Color, LinearRgba) {
    let (r, g, b) = T_RGB[tier];
    (Color::srgb(r, g, b), LinearRgba::rgb(r * 2.5, g * 2.5, b * 2.5))
}

// ---------- tiny 5x7 pixel font for in-world signs ----------

fn glyph(c: char) -> [&'static str; 7] {
    match c.to_ascii_uppercase() {
        'A' => ["01110", "10001", "10001", "11111", "10001", "10001", "10001"],
        'B' => ["11110", "10001", "10001", "11110", "10001", "10001", "11110"],
        'C' => ["01110", "10001", "10000", "10000", "10000", "10001", "01110"],
        'D' => ["11110", "10001", "10001", "10001", "10001", "10001", "11110"],
        'E' => ["11111", "10000", "10000", "11110", "10000", "10000", "11111"],
        'F' => ["11111", "10000", "10000", "11110", "10000", "10000", "10000"],
        'G' => ["01110", "10001", "10000", "10111", "10001", "10001", "01111"],
        'H' => ["10001", "10001", "10001", "11111", "10001", "10001", "10001"],
        'I' => ["01110", "00100", "00100", "00100", "00100", "00100", "01110"],
        'J' => ["00111", "00010", "00010", "00010", "00010", "10010", "01100"],
        'K' => ["10001", "10010", "10100", "11000", "10100", "10010", "10001"],
        'L' => ["10000", "10000", "10000", "10000", "10000", "10000", "11111"],
        'M' => ["10001", "11011", "10101", "10101", "10001", "10001", "10001"],
        'N' => ["10001", "10001", "11001", "10101", "10011", "10001", "10001"],
        'O' => ["01110", "10001", "10001", "10001", "10001", "10001", "01110"],
        'P' => ["11110", "10001", "10001", "11110", "10000", "10000", "10000"],
        'Q' => ["01110", "10001", "10001", "10001", "10101", "10010", "01101"],
        'R' => ["11110", "10001", "10001", "11110", "10100", "10010", "10001"],
        'S' => ["01111", "10000", "10000", "01110", "00001", "00001", "11110"],
        'T' => ["11111", "00100", "00100", "00100", "00100", "00100", "00100"],
        'U' => ["10001", "10001", "10001", "10001", "10001", "10001", "01110"],
        'V' => ["10001", "10001", "10001", "10001", "10001", "01010", "00100"],
        'W' => ["10001", "10001", "10001", "10101", "10101", "10101", "01010"],
        'X' => ["10001", "10001", "01010", "00100", "01010", "10001", "10001"],
        'Y' => ["10001", "10001", "01010", "00100", "00100", "00100", "00100"],
        'Z' => ["11111", "00001", "00010", "00100", "01000", "10000", "11111"],
        '0' => ["01110", "10001", "10011", "10101", "11001", "10001", "01110"],
        '1' => ["00100", "01100", "00100", "00100", "00100", "00100", "01110"],
        '2' => ["01110", "10001", "00001", "00010", "00100", "01000", "11111"],
        '3' => ["11110", "00001", "00001", "01110", "00001", "00001", "11110"],
        '4' => ["00010", "00110", "01010", "10010", "11111", "00010", "00010"],
        '5' => ["11111", "10000", "11110", "00001", "00001", "10001", "01110"],
        '6' => ["00110", "01000", "10000", "11110", "10001", "10001", "01110"],
        '7' => ["11111", "00001", "00010", "00100", "01000", "01000", "01000"],
        '8' => ["01110", "10001", "10001", "01110", "10001", "10001", "01110"],
        '9' => ["01110", "10001", "10001", "01111", "00001", "00010", "01100"],
        '.' => ["00000", "00000", "00000", "00000", "00000", "01100", "01100"],
        ',' => ["00000", "00000", "00000", "00000", "01100", "00100", "01000"],
        '!' => ["00100", "00100", "00100", "00100", "00100", "00000", "00100"],
        '-' => ["00000", "00000", "00000", "11111", "00000", "00000", "00000"],
        '/' => ["00001", "00010", "00010", "00100", "01000", "01000", "10000"],
        '\'' => ["00100", "00100", "01000", "00000", "00000", "00000", "00000"],
        ':' => ["00000", "01100", "01100", "00000", "01100", "01100", "00000"],
        _ => ["00000", "00000", "00000", "00000", "00000", "00000", "00000"],
    }
}

// Render centred lines of text into an RGBA image. Returns the handle and its pixel size.
fn text_image(
    images: &mut Assets<Image>,
    lines: &[&str],
    scale: u32,
    pad: u32,
    fg: [u8; 4],
    bg: [u8; 4],
) -> (Handle<Image>, u32, u32) {
    let cols = lines.iter().map(|l| l.chars().count() as u32).max().unwrap_or(1);
    let w = cols * 6 * scale + 2 * pad;
    let h = lines.len() as u32 * 9 * scale + 2 * pad;
    let mut px = vec![bg; (w * h) as usize];
    for (li, line) in lines.iter().enumerate() {
        let n = line.chars().count() as u32;
        let x_off = pad + (cols - n) * 3 * scale;
        let y_off = pad + li as u32 * 9 * scale + scale;
        for (ci, c) in line.chars().enumerate() {
            let g = glyph(c);
            for (gy, row) in g.iter().enumerate() {
                for (gx, bit) in row.bytes().enumerate() {
                    if bit != b'1' {
                        continue;
                    }
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let x = x_off + (ci as u32 * 6 + gx as u32) * scale + dx;
                            let y = y_off + gy as u32 * scale + dy;
                            px[(y * w + x) as usize] = fg;
                        }
                    }
                }
            }
        }
    }
    let handle = make_image(images, w, h, false, |x, y| px[(y * w + x) as usize]);
    (handle, w, h)
}

// EGG_SCREENSHOT=1 starts straight into the world with no HUD or menu (for taking the menu picture)
#[derive(Resource)]
struct PhotoMode(bool);

fn main() {
    App::new()
        .insert_resource(PhotoMode(std::env::var("EGG_SCREENSHOT").is_ok()))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Egg Stealer 3D".to_string(),
                resolution: Vec2::new(1280.0, 800.0).into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb(0.54, 0.74, 0.94)))
        .insert_resource(AmbientLight {
            color: Color::srgb(0.86, 0.88, 0.94),
            brightness: 320.0,
            ..default()
        })
        .insert_resource(DirectionalLightShadowMap { size: 4096 })
        .add_systems(Startup, (setup, set_dock_icon))
        .add_systems(
            Update,
            (
                (
                    cursor_grab,
                    player_look,
                    player_move,
                    select_slot,
                    gameplay,
                    held_egg,
                    gear_system,
                    portal_system,
                    portal_menu,
                    world_buttons,
                )
                    .run_if(playing),
                main_menu,
                pause_menu,
                day_night,
                monsters_ai,
                hatching,
                animate,
                hud,
            ),
        )
        .add_systems(Last, save_system)
        .run();
}

// Dock icon (macOS). Running through `cargo run` gives a generic executable icon,
// so hand AppKit our own image once the window exists. The NonSend parameter pins
// this system to the main thread, which AppKit requires.
#[cfg(target_os = "macos")]
fn set_dock_icon(_windows: NonSend<bevy::winit::WinitWindows>) {
    use objc2::ClassType;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::{MainThreadMarker, NSData};
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(include_bytes!("../assets/icon.png"));
    if let Some(img) = NSImage::initWithData(NSImage::alloc(), &data) {
        unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&img)) };
    }
}

#[cfg(not(target_os = "macos"))]
fn set_dock_icon() {}

// ---------- procedural textures ----------

fn hash2(x: u32, y: u32, s: u32) -> u32 {
    let mut n = x
        .wrapping_mul(374761393)
        .wrapping_add(y.wrapping_mul(668265263))
        .wrapping_add(s.wrapping_mul(2246822519));
    n ^= n >> 13;
    n = n.wrapping_mul(1274126177);
    n ^ (n >> 16)
}

fn make_image(
    images: &mut Assets<Image>,
    w: u32,
    h: u32,
    repeat: bool,
    f: impl Fn(u32, u32) -> [u8; 4],
) -> Handle<Image> {
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            data.extend_from_slice(&f(x, y));
        }
    }
    let mut img = Image::new(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    let mode = if repeat {
        ImageAddressMode::Repeat
    } else {
        ImageAddressMode::ClampToEdge
    };
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: mode,
        address_mode_v: mode,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..ImageSamplerDescriptor::default()
    });
    images.add(img)
}

// Smooth value noise on a `cell`-pixel lattice that tiles at 256 px.
fn vnoise(x: u32, y: u32, cell: u32, seed: u32) -> f32 {
    let n = 256 / cell;
    let fx = (x % cell) as f32 / cell as f32;
    let fy = (y % cell) as f32 / cell as f32;
    let (cx, cy) = (x / cell, y / cell);
    let v = |i: u32, j: u32| (hash2(i % n, j % n, seed) % 1000) as f32 / 1000.0;
    let (a, b, c, d) = (v(cx, cy), v(cx + 1, cy), v(cx, cy + 1), v(cx + 1, cy + 1));
    let sx = fx * fx * (3.0 - 2.0 * fx);
    let sy = fy * fy * (3.0 - 2.0 * fy);
    let top = a + (b - a) * sx;
    let bot = c + (d - c) * sx;
    top + (bot - top) * sy
}

fn grass_pixel(x: u32, y: u32) -> [u8; 4] {
    let big = vnoise(x, y, 64, 21) - 0.5;
    let mid = vnoise(x, y, 16, 23) - 0.5;
    let fine = (hash2(x, y, 7) % 41) as f32 / 41.0 - 0.5;
    let shade = big * 0.30 + mid * 0.22 + fine * 0.16;
    // worn, yellowish patches
    let dry = (vnoise(x, y, 32, 29) - 0.55).max(0.0) * 1.6;
    let blade = hash2(x, y, 99) % 53 < 2;
    let mut r = 72.0 + shade * 90.0 + dry * 60.0;
    let mut g = 102.0 + shade * 110.0 + dry * 34.0;
    let mut b = 38.0 + shade * 50.0;
    if blade {
        r += 24.0;
        g += 38.0;
        b += 10.0;
    }
    [
        r.clamp(0.0, 255.0) as u8,
        g.clamp(0.0, 255.0) as u8,
        b.clamp(0.0, 255.0) as u8,
        255,
    ]
}

fn wood_pixel(x: u32, y: u32) -> [u8; 4] {
    let wobble = vnoise(x, y, 32, 31) * 6.0 + (y as f32 * 0.07).sin() * 2.5;
    let ring = ((x as f32 * 0.42 + wobble).sin() * 0.5 + 0.5) * 34.0;
    let grain = (hash2(x, y, 17) % 13) as i32 - 6;
    let r = (120.0 + ring) as i32 + grain;
    let g = (86.0 + ring * 0.8) as i32 + grain;
    let b = (52.0 + ring * 0.5) as i32 + grain / 2;
    [
        r.clamp(0, 255) as u8,
        g.clamp(0, 255) as u8,
        b.clamp(0, 255) as u8,
        255,
    ]
}

fn stone_pixel(x: u32, y: u32) -> [u8; 4] {
    let row = y / 32;
    let xoff = if row % 2 == 0 { 0 } else { 32 };
    let bx = (x + xoff) % 64;
    let mortar = bx < 3 || y % 32 < 3;
    if mortar {
        let n = (hash2(x, y, 3) % 15) as i32;
        let v = (88 + n) as u8;
        return [v, v, v + 4, 255];
    }
    let brick = hash2((x + xoff) / 64, row, 11) % 40;
    let n = (hash2(x, y, 5) % 21) as i32 - 10;
    let v = (125 + brick as i32 + n).clamp(0, 255) as u8;
    [v, v, (v as i32 + 6).clamp(0, 255) as u8, 255]
}

fn belt_pixel(x: u32, _y: u32) -> [u8; 4] {
    if x % 16 < 3 {
        [82, 82, 88, 255]
    } else {
        [34, 34, 38, 255]
    }
}

fn pattern_pixel(pi: usize, x: u32, y: u32) -> [u8; 4] {
    let speck = (hash2(x, y, pi as u32 + 40) % 25) as i32 - 12;
    let base = (235 + speck).clamp(0, 255) as u8;
    let mut px = [base, base, base, 255];
    let dark = (120 + speck).clamp(0, 255) as u8;
    let fx = x as f32;
    let fy = y as f32;
    match pi {
        1 => {
            // spotted
            for k in 0..14u32 {
                let sx = (hash2(k, 1, 61) % 128) as f32;
                let sy = (hash2(k, 2, 61) % 128) as f32;
                let r = 5.0 + (hash2(k, 3, 61) % 6) as f32;
                for wrap in [-128.0, 0.0, 128.0] {
                    let d = ((fx - sx + wrap).powi(2) + (fy - sy).powi(2)).sqrt();
                    if d < r {
                        px = [dark, dark, dark, 255];
                    }
                }
            }
        }
        2 => {
            if y % 26 < 8 {
                px = [dark, dark, dark, 255];
            }
        }
        3 => {
            // starry: gold diamonds
            for k in 0..12u32 {
                let sx = (hash2(k, 5, 77) % 128) as i32;
                let sy = (hash2(k, 6, 77) % 128) as i32;
                for wrap in [-128i32, 0, 128] {
                    if (x as i32 - sx + wrap).abs() + (y as i32 - sy).abs() < 6 {
                        px = [225, 190, 70, 255];
                    }
                }
            }
        }
        4 => {
            // cracked
            for c in [38i32, 86] {
                let zig = c + ((y / 9) % 2) as i32 * 7 + (hash2(y / 9, c as u32, 13) % 5) as i32;
                if (x as i32 - zig).abs() < 2 {
                    px = [70, 65, 60, 255];
                }
            }
        }
        5 => {
            // swirly
            let dx = fx - 64.0;
            let dy = fy - 64.0;
            let r = (dx * dx + dy * dy).sqrt();
            let a = dy.atan2(dx);
            if (a * 2.0 + r * 0.22).sin() > 0.55 {
                px = [dark, dark, dark, 255];
            }
        }
        6 => {
            if (x / 16 + y / 16) % 2 == 0 {
                let v = (200 + speck).clamp(0, 255) as u8;
                px = [v, v, v, 255];
            }
        }
        8 => {
            // fuzzy: heavy noise
            let n = (hash2(x, y, 500) % 90) as i32 - 45;
            let v = (215 + n).clamp(0, 255) as u8;
            px = [v, v, v, 255];
        }
        9 => {
            // crystal facets
            let facet = hash2(x / 22, y / 17, 31) % 70;
            let v = (185 + facet as i32).clamp(0, 255) as u8;
            px = [v, v, 255, 255];
            if x % 22 < 2 || y % 17 < 2 {
                px = [130, 140, 190, 255];
            }
        }
        10 => {
            // camo blobs
            let n = hash2(x / 18, y / 18, 42) % 3;
            px = match n {
                0 => [150, 160, 120, 255],
                1 => [190, 180, 150, 255],
                _ => [base, base, base, 255],
            };
        }
        11 => {
            // royal: gold band + dots
            if (y as i32 - 64).abs() < 10 {
                px = [220, 185, 70, 255];
            } else if (y == 30 || y == 98) && x % 16 < 5 {
                px = [220, 185, 70, 255];
            }
        }
        12 => {
            // hacker: dark with green code columns
            px = [28, 38, 28, 255];
            if hash2(x / 8, y / 5, 314) % 11 < 3 && y % 5 < 3 {
                px = [50, 230, 100, 255];
            }
        }
        _ => {}
    }
    px
}

// ---------- egg mesh (real egg profile, not a sphere) ----------

fn build_egg_mesh() -> Mesh {
    const LAT: usize = 26;
    const LON: usize = 36;
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    for i in 0..=LAT {
        let v = i as f32 / LAT as f32;
        let theta = v * PI; // 0 = top
        let y = theta.cos();
        let taper = 1.0 - 0.28 * ((y + 1.0) * 0.5).powf(1.8);
        let r = theta.sin() * 0.5 * taper;
        for j in 0..=LON {
            let u = j as f32 / LON as f32;
            let phi = u * TAU;
            positions.push([r * phi.cos(), y * 0.65, r * phi.sin()]);
            uvs.push([u, v]);
        }
    }
    let idx = |i: usize, j: usize| i * (LON + 1) + j;
    let pget = |i: i32, j: i32| -> Vec3 {
        let ii = i.clamp(0, LAT as i32) as usize;
        let jj = (((j % LON as i32) + LON as i32) % LON as i32) as usize;
        Vec3::from(positions[idx(ii, jj)])
    };
    let mut normals: Vec<[f32; 3]> = Vec::new();
    for i in 0..=LAT {
        for j in 0..=LON {
            let du = pget(i as i32, j as i32 + 1) - pget(i as i32, j as i32 - 1);
            let dv = pget(i as i32 + 1, j as i32) - pget(i as i32 - 1, j as i32);
            let n = du.cross(dv);
            if n.length_squared() > 1e-8 {
                normals.push(n.normalize().to_array());
            } else {
                normals.push([0.0, if i == 0 { 1.0 } else { -1.0 }, 0.0]);
            }
        }
    }
    let mut indices: Vec<u32> = Vec::new();
    for i in 0..LAT {
        for j in 0..LON {
            let a = idx(i, j) as u32;
            let b = idx(i + 1, j) as u32;
            let c = idx(i, j + 1) as u32;
            let d = idx(i + 1, j + 1) as u32;
            indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

// ---------- setup ----------

fn spawn_world_egg(commands: &mut Commands, game: &Game, x: f32, z: f32, kind: usize) {
    commands.spawn((
        Mesh3d(game.egg_mesh.clone()),
        MeshMaterial3d(game.egg_mats[kind].clone()),
        Transform::from_xyz(x, 0.36, z)
            .with_scale(Vec3::splat(0.55))
            .with_rotation(
                Quat::from_rotation_y(fastrand::f32() * TAU)
                    * Quat::from_rotation_z((fastrand::f32() - 0.5) * 0.12),
            ),
        WorldEgg { kind },
    ));
}

fn setup(
    mut commands: Commands,
    photo_mode: Res<PhotoMode>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    asset_server: Res<AssetServer>,
) {
    fastrand::seed(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(12345),
    );

    // --- textures & shared materials ---
    let grass_tex = make_image(&mut images, 256, 256, true, grass_pixel);
    let wood_tex = make_image(&mut images, 128, 128, true, wood_pixel);
    let stone_tex = make_image(&mut images, 128, 128, true, stone_pixel);
    let belt_tex = make_image(&mut images, 64, 64, true, belt_pixel);

    let safe_grass = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 1.0, 1.0),
        base_color_texture: Some(grass_tex.clone()),
        perceptual_roughness: 0.95,
        uv_transform: Affine2::from_scale(Vec2::new(16.0, 31.0)),
        ..default()
    });
    let zone_grass = materials.add(StandardMaterial {
        base_color: Color::srgb(0.78, 0.82, 0.78),
        base_color_texture: Some(grass_tex.clone()),
        perceptual_roughness: 0.95,
        uv_transform: Affine2::from_scale(Vec2::new(21.0, 31.0)),
        ..default()
    });
    let stone_x = materials.add(StandardMaterial {
        base_color_texture: Some(stone_tex.clone()),
        perceptual_roughness: 0.9,
        uv_transform: Affine2::from_scale(Vec2::new(34.0, 1.6)),
        ..default()
    });
    let stone_z = materials.add(StandardMaterial {
        base_color_texture: Some(stone_tex),
        perceptual_roughness: 0.9,
        uv_transform: Affine2::from_scale(Vec2::new(20.0, 1.6)),
        ..default()
    });
    let wood = materials.add(StandardMaterial {
        base_color: Color::srgb(0.85, 0.78, 0.7),
        base_color_texture: Some(wood_tex),
        perceptual_roughness: 0.85,
        ..default()
    });
    let white_line = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 1.0, 1.0),
        emissive: LinearRgba::rgb(0.6, 0.6, 0.6),
        perceptual_roughness: 0.8,
        ..default()
    });

    // egg materials: 13 patterns x 13 tints
    let mut pattern_tex = Vec::new();
    for pi in 0..13 {
        pattern_tex.push(make_image(&mut images, 128, 128, true, move |x, y| {
            pattern_pixel(pi, x, y)
        }));
    }
    let mut egg_mats = Vec::with_capacity(169);
    for kind in 0..169 {
        let (r, g, b) = EGG_RGB[kind / 13];
        let pi = kind % 13;
        egg_mats.push(materials.add(StandardMaterial {
            base_color: Color::srgb(r, g, b),
            base_color_texture: Some(pattern_tex[pi].clone()),
            perceptual_roughness: match pi {
                7 => 0.07,
                9 => 0.2,
                8 => 0.98,
                _ => 0.45,
            },
            metallic: if pi == 9 { 0.7 } else { 0.0 },
            ..default()
        }));
    }

    let egg_mesh = meshes.add(build_egg_mesh());
    let unit_sphere = meshes.add(Sphere::new(1.0).mesh().uv(28, 18));
    let unit_cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));

    // --- ground ---
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(LINE_X, WORLD_Z))),
        MeshMaterial3d(safe_grass.clone()),
        Transform::from_xyz(LINE_X / 2.0, 0.0, WORLD_Z / 2.0),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(WORLD_X - LINE_X, WORLD_Z))),
        MeshMaterial3d(zone_grass.clone()),
        Transform::from_xyz((WORLD_X + LINE_X) / 2.0, 0.0, WORLD_Z / 2.0),
    ));
    // white line
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.2, 0.04, WORLD_Z))),
        MeshMaterial3d(white_line),
        Transform::from_xyz(LINE_X, 0.02, WORLD_Z / 2.0),
    ));

    // --- sky dome + sun disc ---
    let sky_mesh = meshes.add(sky_dome_mesh(1, false));
    commands.spawn((
        Mesh3d(sky_mesh.clone()),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::WHITE,
            unlit: true,
            fog_enabled: false,
            cull_mode: None,
            double_sided: true,
            ..default()
        })),
        Transform::from_translation(SKY_CENTER),
        NotShadowCaster,
        NotShadowReceiver,
    ));
    let t1 = theme(1, false);
    let sun_mat = materials.add(StandardMaterial {
        base_color: Color::BLACK,
        emissive: LinearRgba::rgb(t1.disc.0, t1.disc.1, t1.disc.2),
        fog_enabled: false,
        ..default()
    });
    commands.spawn((
        Mesh3d(unit_sphere.clone()),
        MeshMaterial3d(sun_mat.clone()),
        Transform::from_translation(SKY_CENTER - sun_dir() * 700.0).with_scale(Vec3::splat(24.0)),
        NotShadowCaster,
        NotShadowReceiver,
    ));

    // --- stone walls ---
    let wall_h = 4.5;
    for (size, pos, mat) in [
        (
            Vec3::new(WORLD_X + 6.0, wall_h, 3.0),
            Vec3::new(WORLD_X / 2.0, wall_h / 2.0, -1.5),
            stone_x.clone(),
        ),
        (
            Vec3::new(WORLD_X + 6.0, wall_h, 3.0),
            Vec3::new(WORLD_X / 2.0, wall_h / 2.0, WORLD_Z + 1.5),
            stone_x.clone(),
        ),
        (
            Vec3::new(3.0, wall_h, WORLD_Z + 6.0),
            Vec3::new(-1.5, wall_h / 2.0, WORLD_Z / 2.0),
            stone_z.clone(),
        ),
        (
            Vec3::new(3.0, wall_h, WORLD_Z + 6.0),
            Vec3::new(WORLD_X + 1.5, wall_h / 2.0, WORLD_Z / 2.0),
            stone_z.clone(),
        ),
    ] {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(size.x, size.y, size.z))),
            MeshMaterial3d(mat),
            Transform::from_translation(pos),
        ));
    }

    // --- four yards with wooden fences ---
    for yi in 0..4 {
        let x0 = 13.0;
        let x1 = 43.0;
        let z0 = YARD.2 + yi as f32 * YARD_GAP;
        let z1 = z0 + 25.0;
        let post = meshes.add(Cuboid::new(0.14, 1.25, 0.14));
        let spawn_post = |x: f32, z: f32, commands: &mut Commands| {
            commands.spawn((
                Mesh3d(post.clone()),
                MeshMaterial3d(wood.clone()),
                Transform::from_xyz(x, 0.62, z),
            ));
        };
        let nx = 12;
        let nz = 10;
        for k in 0..=nx {
            let x = x0 + (x1 - x0) * k as f32 / nx as f32;
            spawn_post(x, z0, &mut commands);
            spawn_post(x, z1, &mut commands);
        }
        for k in 1..nz {
            let z = z0 + (z1 - z0) * k as f32 / nz as f32;
            spawn_post(x0, z, &mut commands);
            // east side has a 4 m gate gap in the middle
            if !((z - (z0 + z1) / 2.0).abs() < 2.0) {
                spawn_post(x1, z, &mut commands);
            }
        }
        // rails
        for ry in [0.45, 0.95] {
            for (len, cx, cz, along_x) in [
                (x1 - x0, (x0 + x1) / 2.0, z0, true),
                (x1 - x0, (x0 + x1) / 2.0, z1, true),
                (z1 - z0, x0, (z0 + z1) / 2.0, false),
            ] {
                let size = if along_x {
                    Vec3::new(len, 0.09, 0.06)
                } else {
                    Vec3::new(0.06, 0.09, len)
                };
                commands.spawn((
                    Mesh3d(meshes.add(Cuboid::new(size.x, size.y, size.z))),
                    MeshMaterial3d(wood.clone()),
                    Transform::from_xyz(cx, ry, cz),
                ));
            }
            // east rails, split around the gate
            let seg = (z1 - z0) / 2.0 - 2.0;
            for cz in [z0 + seg / 2.0, z1 - seg / 2.0] {
                commands.spawn((
                    Mesh3d(meshes.add(Cuboid::new(0.06, 0.09, seg))),
                    MeshMaterial3d(wood.clone()),
                    Transform::from_xyz(x1, ry, cz),
                ));
            }
        }
    }

    // --- treadmills, one per yard, all functional ---
    let belt_mat = materials.add(StandardMaterial {
        base_color_texture: Some(belt_tex.clone()),
        perceptual_roughness: 0.6,
        uv_transform: Affine2::from_scale(Vec2::new(3.0, 1.0)),
        ..default()
    });
    let (tr, tg, tb) = T_RGB[0];
    let panel_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(tr, tg, tb),
        emissive: LinearRgba::rgb(tr * 2.0, tg * 2.0, tb * 2.0),
        ..default()
    });
    let frame_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.2, 0.2, 0.22),
        perceptual_roughness: 0.4,
        metallic: 0.6,
        ..default()
    });
    for yi in 0..4 {
        let c = treadmill_center(yi);
        let hl = TM_BELT_L / 2.0 + 0.1;
        let hw = TM_BELT_W / 2.0;
        // belt
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(TM_BELT_L, 0.14, TM_BELT_W))),
            MeshMaterial3d(belt_mat.clone()),
            Transform::from_xyz(c.x, TM_DECK_H - 0.07, c.z),
        ));
        // side frames
        for side in [-1.0, 1.0] {
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(hl * 2.0, 0.3, 0.12))),
                MeshMaterial3d(frame_mat.clone()),
                Transform::from_xyz(c.x, 0.16, c.z + side * (hw + 0.06)),
            ));
        }
        // console post + panel (west end, facing the yard)
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(0.22, 1.15, TM_BELT_W + 0.24))),
            MeshMaterial3d(frame_mat.clone()),
            Transform::from_xyz(c.x - hl - 0.05, 0.6, c.z),
        ));
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(0.08, 0.5, 1.2))),
            MeshMaterial3d(panel_mat.clone()),
            Transform::from_xyz(c.x - hl - 0.2, 1.05, c.z),
        ));
    }

    // --- gear station / upgrader, one per yard ---
    let kiosk_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.16, 0.18, 0.24),
        perceptual_roughness: 0.45,
        metallic: 0.5,
        ..default()
    });
    let kiosk_top = materials.add(StandardMaterial {
        base_color: Color::srgb(0.30, 0.65, 0.95),
        emissive: LinearRgba::rgb(0.3, 0.9, 1.6),
        perceptual_roughness: 0.3,
        ..default()
    });
    let (sign_tex, sw, sh) = text_image(
        &mut images,
        &["GEAR STATION / UPGRADER"],
        6,
        24,
        [255, 255, 255, 255],
        [18, 30, 70, 255],
    );
    let sign_w = 4.4;
    let sign_h = sign_w * sh as f32 / sw as f32;
    let sign_mesh = meshes.add(Rectangle::new(sign_w, sign_h));
    let sign_mat = materials.add(StandardMaterial {
        base_color_texture: Some(sign_tex),
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    for yi in 0..4 {
        let g = gear_pos(yi);
        commands.spawn((
            Mesh3d(unit_cube.clone()),
            MeshMaterial3d(kiosk_mat.clone()),
            Transform::from_xyz(g.x, 0.55, g.z).with_scale(Vec3::new(2.2, 1.1, 1.8)),
        ));
        commands.spawn((
            Mesh3d(unit_cube.clone()),
            MeshMaterial3d(kiosk_top.clone()),
            Transform::from_xyz(g.x, 1.14, g.z).with_scale(Vec3::new(2.3, 0.08, 1.9)),
        ));
        // sign post + sign on top
        commands.spawn((
            Mesh3d(unit_cube.clone()),
            MeshMaterial3d(kiosk_mat.clone()),
            Transform::from_xyz(g.x, 1.9, g.z).with_scale(Vec3::new(0.12, 1.6, 0.12)),
        ));
        commands.spawn((
            Mesh3d(sign_mesh.clone()),
            MeshMaterial3d(sign_mat.clone()),
            Transform::from_xyz(g.x, 2.7 + sign_h / 2.0, g.z)
                .with_rotation(Quat::from_rotation_y(PI / 2.0)),
            NotShadowCaster,
        ));
    }

    // --- night wall along the line (hidden by day) ---
    let (wall_tex, ww, wh) = text_image(
        &mut images,
        &["IT IS NIGHT TIME.", "YOU CANNOT GO OUT THERE."],
        5,
        16,
        [255, 235, 235, 255],
        [120, 0, 0, 255],
    );
    let wall_text_mat = materials.add(StandardMaterial {
        base_color_texture: Some(wall_tex),
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    let panel_w = 12.0;
    let panel_h = panel_w * wh as f32 / ww as f32;
    let panel_mesh = meshes.add(Rectangle::new(panel_w, panel_h));
    commands
        .spawn((
            Mesh3d(meshes.add(Cuboid::new(0.3, 6.0, WORLD_Z))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgba(0.9, 0.05, 0.05, 0.45),
                emissive: LinearRgba::rgb(1.6, 0.05, 0.05),
                alpha_mode: AlphaMode::Blend,
                double_sided: true,
                cull_mode: None,
                ..default()
            })),
            Transform::from_xyz(LINE_X, 3.0, WORLD_Z / 2.0),
            Visibility::Hidden,
            NotShadowCaster,
            NightWall,
        ))
        .with_children(|w| {
            // warning panels every 20 m along the west face of the wall (local space)
            let mut z = -WORLD_Z / 2.0 + 10.0;
            while z < WORLD_Z / 2.0 - 5.0 {
                w.spawn((
                    Mesh3d(panel_mesh.clone()),
                    MeshMaterial3d(wall_text_mat.clone()),
                    Transform::from_xyz(-0.2, 0.4, z).with_rotation(Quat::from_rotation_y(-PI / 2.0)),
                    NotShadowCaster,
                ));
                z += 20.0;
            }
        });
    let ice_mat = materials.add(StandardMaterial {
        base_color: Color::srgba(0.65, 0.85, 1.0, 0.55),
        emissive: LinearRgba::rgb(0.1, 0.25, 0.4),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.1,
        ..default()
    });

    // --- monsters ---
    let eye_open = materials.add(StandardMaterial {
        base_color: Color::srgb(0.6, 0.05, 0.05),
        emissive: LinearRgba::rgb(9.0, 0.6, 0.6),
        ..default()
    });
    let eye_closed = materials.add(StandardMaterial {
        base_color: Color::srgb(0.12, 0.1, 0.1),
        perceptual_roughness: 0.9,
        ..default()
    });
    let horn_mesh = meshes.add(Cone {
        radius: 0.28,
        height: 0.95,
    });
    let arm_mesh = meshes.add(Capsule3d::new(0.22, 1.0));
    let leg_mesh = meshes.add(Capsule3d::new(0.30, 0.8));
    let tooth_mesh = meshes.add(Cone {
        radius: 0.08,
        height: 0.2,
    });
    let tooth_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.95, 0.95, 0.88),
        perceptual_roughness: 0.35,
        ..default()
    });
    // shaggy fur texture shared by all monsters, tinted per monster
    let fur_tex = make_image(&mut images, 64, 64, true, |x, y| {
        let streak = (hash2(x / 2, y / 7, 91) % 60) as i32;
        let n = (hash2(x, y, 17) % 25) as i32;
        let v = (150 + streak - n).clamp(90, 255) as u8;
        [v, v, v, 255]
    });
    let monster_defs: [(Vec3, f32, f32, (f32, f32, f32)); 5] = [
        (Vec3::new(88.0, 0.0, 28.0), 1.0, 6.5, (0.45, 0.13, 0.13)),
        (Vec3::new(114.0, 0.0, 23.0), 1.2, 6.2, (0.34, 0.13, 0.42)),
        (Vec3::new(135.0, 0.0, 64.0), 1.35, 5.8, (0.13, 0.30, 0.15)),
        (Vec3::new(92.0, 0.0, 92.0), 1.05, 6.8, (0.13, 0.17, 0.34)),
        (Vec3::new(120.0, 0.0, 103.0), 1.1, 6.0, (0.16, 0.16, 0.18)),
    ];
    for (home, scale, speed, (r, g, b)) in monster_defs {
        let fur_mat = materials.add(StandardMaterial {
            base_color: Color::srgb((r * 1.6).min(1.0), (g * 1.6).min(1.0), (b * 1.6).min(1.0)),
            base_color_texture: Some(fur_tex.clone()),
            perceptual_roughness: 0.95,
            uv_transform: Affine2::from_scale(Vec2::splat(3.0)),
            ..default()
        });
        let belly_mat = materials.add(StandardMaterial {
            base_color: Color::srgb((r * 2.3).min(1.0), (g * 2.3).min(1.0), (b * 2.3).min(1.0)),
            base_color_texture: Some(fur_tex.clone()),
            perceptual_roughness: 0.95,
            uv_transform: Affine2::from_scale(Vec2::splat(3.0)),
            ..default()
        });
        let dark_mat = materials.add(StandardMaterial {
            base_color: Color::srgb(r * 0.5, g * 0.5, b * 0.5),
            perceptual_roughness: 0.9,
            ..default()
        });
        let mouth_mat = materials.add(StandardMaterial {
            base_color: Color::srgb(0.22, 0.03, 0.03),
            perceptual_roughness: 0.7,
            ..default()
        });
        commands
            .spawn((
                Transform::from_translation(home).with_scale(Vec3::splat(scale)),
                Visibility::default(),
                Monster {
                    home,
                    speed,
                    scale,
                    awake: 0.0,
                    phase: fastrand::f32() * TAU,
                    frozen: 0.0,
                },
            ))
            .with_children(|p| {
                // torso (front = -Z)
                p.spawn((
                    Mesh3d(unit_sphere.clone()),
                    MeshMaterial3d(fur_mat.clone()),
                    Transform::from_xyz(0.0, 2.1, 0.0).with_scale(Vec3::new(1.5, 1.9, 1.35)),
                ));
                // lighter belly patch
                p.spawn((
                    Mesh3d(unit_sphere.clone()),
                    MeshMaterial3d(belly_mat.clone()),
                    Transform::from_xyz(0.0, 1.9, -0.9).with_scale(Vec3::new(0.95, 1.25, 0.5)),
                ));
                // head
                p.spawn((
                    Mesh3d(unit_sphere.clone()),
                    MeshMaterial3d(fur_mat.clone()),
                    Transform::from_xyz(0.0, 4.15, -0.1).with_scale(Vec3::splat(0.85)),
                ));
                // snout
                p.spawn((
                    Mesh3d(unit_sphere.clone()),
                    MeshMaterial3d(belly_mat.clone()),
                    Transform::from_xyz(0.0, 3.95, -0.8).with_scale(Vec3::new(0.42, 0.32, 0.45)),
                ));
                // mouth line + fangs
                p.spawn((
                    Mesh3d(unit_cube.clone()),
                    MeshMaterial3d(mouth_mat),
                    Transform::from_xyz(0.0, 3.72, -1.0).with_scale(Vec3::new(0.55, 0.1, 0.08)),
                ));
                for dx in [-0.17f32, 0.0, 0.17] {
                    p.spawn((
                        Mesh3d(tooth_mesh.clone()),
                        MeshMaterial3d(tooth_mat.clone()),
                        Transform::from_xyz(dx, 3.62, -1.0)
                            .with_rotation(Quat::from_rotation_x(PI)),
                    ));
                }
                // eyes on the head
                for dx in [-0.36, 0.36] {
                    p.spawn((
                        Mesh3d(unit_sphere.clone()),
                        MeshMaterial3d(eye_closed.clone()),
                        Transform::from_xyz(dx, 4.35, -0.72).with_scale(Vec3::splat(0.17)),
                        MonsterEye,
                    ));
                }
                // horns
                for dx in [-0.5f32, 0.5] {
                    p.spawn((
                        Mesh3d(horn_mesh.clone()),
                        MeshMaterial3d(dark_mat.clone()),
                        Transform::from_xyz(dx, 4.95, 0.0)
                            .with_rotation(Quat::from_rotation_z(-dx.signum() * 0.4)),
                    ));
                }
                // swinging arms
                for dx in [-1.35f32, 1.35] {
                    p.spawn((
                        Mesh3d(arm_mesh.clone()),
                        MeshMaterial3d(fur_mat.clone()),
                        Transform::from_xyz(dx, 2.35, 0.0),
                        MonsterLimb {
                            base_z: -dx.signum() * 0.25,
                            phase: if dx < 0.0 { 0.0 } else { PI },
                        },
                    ));
                }
                // walking legs + feet
                for dx in [-0.62f32, 0.62] {
                    p.spawn((
                        Mesh3d(leg_mesh.clone()),
                        MeshMaterial3d(fur_mat.clone()),
                        Transform::from_xyz(dx, 0.75, 0.0),
                        MonsterLimb {
                            base_z: 0.0,
                            phase: if dx < 0.0 { PI } else { 0.0 },
                        },
                    ));
                    p.spawn((
                        Mesh3d(unit_sphere.clone()),
                        MeshMaterial3d(dark_mat.clone()),
                        Transform::from_xyz(dx, 0.22, -0.18).with_scale(Vec3::new(0.4, 0.2, 0.55)),
                    ));
                }
            });
    }

    // --- a portal at the back of every yard ---
    let portal_tex = make_image(&mut images, 128, 128, true, |x, y| {
        let dx = x as f32 - 64.0;
        let dy = y as f32 - 64.0;
        let r = (dx * dx + dy * dy).sqrt();
        let a = dy.atan2(dx);
        if (a * 3.0 + r * 0.28).sin() > 0.45 {
            [120, 230, 255, 255]
        } else {
            [25, 20, 70, 255]
        }
    });
    let portal_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.25, 0.25, 0.45),
        base_color_texture: Some(portal_tex.clone()),
        emissive: LinearRgba::rgb(0.12, 0.25, 0.4),
        emissive_texture: Some(portal_tex),
        ..default()
    });
    let ring_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.35, 0.33, 0.42),
        perceptual_roughness: 0.7,
        metallic: 0.3,
        ..default()
    });
    let ring_mesh = meshes.add(Torus {
        minor_radius: 0.25,
        major_radius: 2.0,
    });
    let disc_mesh = meshes.add(Circle::new(1.85));
    for yi in 0..4 {
        let pp = portal_pos(yi);
        commands.spawn((
            Mesh3d(ring_mesh.clone()),
            MeshMaterial3d(ring_mat.clone()),
            Transform::from_xyz(pp.x, 2.3, pp.z).with_rotation(Quat::from_rotation_z(PI / 2.0)),
        ));
        commands.spawn((
            Mesh3d(disc_mesh.clone()),
            MeshMaterial3d(portal_mat.clone()),
            Transform::from_xyz(pp.x, 2.3, pp.z).with_rotation(Quat::from_rotation_y(PI / 2.0)),
            PortalDisc,
        ));
    }

    // --- game resource + initial eggs ---
    let mut game = Game {
        in_menu: !photo_mode.0,
        paused: false,
        money: 0.0,
        training: 0.0,
        tier: 0,
        inventory: [None; INV_SLOTS],
        selected: 0,
        held_kind: None,
        level: 1,
        unlocked: {
            let mut u = [false; N_WORLDS];
            u[0] = true;
            u
        },
        menu_open: false,
        won: false,
        portal_lit: false,
        yard_eggs: Vec::new(),
        msg: "Steal eggs, hatch them into pets, and reach the portal with $1000000!"
            .to_string(),
        msg_color: Color::srgb(1.0, 1.0, 0.7),
        msg_t: 6.0,
        spawn_t: 1.2,
        egg_mesh: egg_mesh.clone(),
        egg_mats,
        eye_open,
        eye_closed,
        belt_mat,
        panel_mat,
        portal_mat,
        safe_grass_mat: safe_grass.clone(),
        zone_grass_mat: zone_grass.clone(),
        sphere_mesh: unit_sphere.clone(),
        defeat_sound: asset_server.load("defeat.wav"),
        pickup_sound: asset_server.load("pickup.wav"),
        on_treadmill: false,
        gear: 0,
        gear_cd: [0.0; 3],
        night: false,
        night_applied: None,
        ice_mat,
        cube_mesh: unit_cube.clone(),
        eye_h: EYE,
        sky_mesh,
        sun_mat,
    };
    // --- restore the last session; eggs keep hatching on the wall clock while you are away ---
    if let Some(sd) = load_save() {
        game.money = sd.money;
        game.training = sd.training;
        game.tier = sd.tier.min(6);
        game.level = sd.level.clamp(1, N_WORLDS);
        for (i, u) in sd.unlocked.iter().enumerate().take(N_WORLDS) {
            game.unlocked[i] = *u;
        }
        game.unlocked[0] = true;
        game.won = sd.won;
        game.gear = sd.gear.min(3);
        game.yard_eggs = sd.yard_eggs;
        for (i, k) in sd.inventory.iter().enumerate().take(INV_SLOTS) {
            game.inventory[i] = k.filter(|k| *k < 169);
        }
        let now = now_unix();
        let mut hatched_away = 0;
        let mut away_money = 0.0;
        for &(kind, slot, at) in sd.hatching.iter().filter(|(k, _, _)| *k < 169) {
            if at <= now {
                away_money += hatch_payout(kind, game.level);
                game.yard_eggs.push(kind);
                hatched_away += 1;
            } else {
                spawn_hatching(&mut commands, &game, kind, slot, at);
            }
        }
        game.money += away_money;
        if let Some(m) = materials.get_mut(&game.panel_mat) {
            let (c, e) = panel_colors(game.tier);
            m.base_color = c;
            m.emissive = e;
        }
        if hatched_away > 0 {
            game.msg = format!(
                "Welcome back! {} egg{} hatched while you were away: +${}",
                hatched_away,
                if hatched_away == 1 { "" } else { "s" },
                fmt_money(away_money)
            );
            game.msg_color = Color::srgb(0.4, 1.0, 0.5);
            game.msg_t = 6.0;
        }
    }
    for _ in 0..60 {
        let (x, z) = random_egg_xz();
        spawn_world_egg(&mut commands, &game, x, z, random_kind());
    }
    commands.insert_resource(game);

    // --- sun, camera, HUD ---
    commands.spawn((
        DirectionalLight {
            illuminance: t1.lux,
            shadows_enabled: true,
            color: Color::srgb(t1.sun.0, t1.sun.1, t1.sun.2),
            ..default()
        },
        Transform::from_xyz(SUN_FROM.x, SUN_FROM.y, SUN_FROM.z).looking_at(SUN_AT, Vec3::Y),
        CascadeShadowConfigBuilder {
            num_cascades: 4,
            first_cascade_far_bound: 10.0,
            maximum_distance: 200.0,
            ..default()
        }
        .build(),
    ));

    commands.spawn((
        Camera3d::default(),
        Camera {
            hdr: true,
            ..default()
        },
        Msaa::Off,
        Projection::from(PerspectiveProjection {
            fov: 75.0_f32.to_radians(),
            ..default()
        }),
        Transform::from_translation(SPAWN)
            .with_rotation(Quat::from_rotation_y(-PI / 2.0)),
        fog_for(1, false),
        ScreenSpaceAmbientOcclusion::default(),
        Bloom {
            intensity: 0.06,
            ..Bloom::NATURAL
        },
        Fxaa::default(),
        Player {
            yaw: -PI / 2.0,
            pitch: 0.0,
        },
    ));

    if !photo_mode.0 {
        spawn_hud(&mut commands, asset_server.load("menu_bg.png"));
    }
}

fn spawn_hud(commands: &mut Commands, menu_bg: Handle<Image>) {
    let font = |s: f32| TextFont {
        font_size: s,
        ..default()
    };
    commands.spawn((
        Text::new(""),
        font(30.0),
        TextColor(Color::srgb(0.45, 1.0, 0.55)),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(16.0),
            top: Val::Px(10.0),
            ..default()
        },
        Hud::Money,
    ));
    commands.spawn((
        Text::new(""),
        font(18.0),
        TextColor(Color::srgb(0.9, 0.9, 0.9)),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(16.0),
            top: Val::Px(48.0),
            ..default()
        },
        Hud::Speed,
    ));
    commands.spawn((
        Text::new(""),
        font(18.0),
        TextColor(Color::srgb(0.95, 0.95, 0.95)),
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(16.0),
            top: Val::Px(10.0),
            ..default()
        },
        Hud::Stats,
    ));
    commands.spawn((
        Text::new(""),
        font(17.0),
        TextColor(Color::srgb(0.9, 0.9, 0.75)),
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(16.0),
            top: Val::Px(36.0),
            ..default()
        },
        Hud::Clock,
    ));
    commands.spawn((
        Text::new(""),
        font(17.0),
        TextColor(Color::srgb(0.75, 0.9, 1.0)),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(16.0),
            bottom: Val::Px(110.0),
            ..default()
        },
        Hud::Gear,
    ));
    for (top, size, hud) in [
        (Val::Px(78.0), 20.0, Hud::Carry),
        (Val::Percent(30.0), 26.0, Hud::Msg),
        (Val::Percent(16.0), 34.0, Hud::Night),
    ] {
        commands
            .spawn(Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top,
                justify_content: JustifyContent::Center,
                ..default()
            })
            .with_children(|p| {
                p.spawn((Text::new(""), font(size), TextColor(Color::WHITE), hud));
            });
    }
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            bottom: Val::Px(24.0),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|p| {
            p.spawn((
                Text::new(""),
                font(19.0),
                TextColor(Color::srgb(1.0, 1.0, 1.0)),
                Hud::Prompt,
            ));
        });
    // crosshair
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(50.0),
            top: Val::Percent(50.0),
            width: Val::Px(4.0),
            height: Val::Px(4.0),
            margin: UiRect {
                left: Val::Px(-2.0),
                top: Val::Px(-2.0),
                ..default()
            },
            ..default()
        },
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.8)),
    ));
    // inventory hotbar
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            bottom: Val::Px(58.0),
            justify_content: JustifyContent::Center,
            column_gap: Val::Px(8.0),
            ..default()
        })
        .with_children(|row| {
            for i in 0..INV_SLOTS {
                row.spawn((
                    Node {
                        width: Val::Px(52.0),
                        height: Val::Px(52.0),
                        border: UiRect::all(Val::Px(2.0)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BorderColor(Color::srgba(1.0, 1.0, 1.0, 0.55)),
                    BorderRadius::all(Val::Px(10.0)),
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
                    SlotUi(i),
                ))
                .with_children(|slot| {
                    slot.spawn((
                        Text::new(""),
                        TextFont {
                            font_size: 15.0,
                            ..default()
                        },
                        TextColor(Color::WHITE),
                        SlotText(i),
                    ));
                });
            }
        });
    // portal world-select bar (hidden until you press E at a portal)
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: Val::Percent(36.0),
                justify_content: JustifyContent::Center,
                display: Display::None,
                ..default()
            },
            PortalMenu,
        ))
        .with_children(|center| {
            center
                .spawn((
                    Node {
                        padding: UiRect::all(Val::Px(16.0)),
                        column_gap: Val::Px(12.0),
                        row_gap: Val::Px(12.0),
                        flex_wrap: FlexWrap::Wrap,
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        max_width: Val::Px(5.0 * 168.0 + 4.0 * 12.0 + 32.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.05, 0.75)),
                    BorderRadius::all(Val::Px(16.0)),
                ))
                .with_children(|bar| {
                    for w in 1..=N_WORLDS {
                        bar.spawn((
                            Button,
                            Node {
                                width: Val::Px(168.0),
                                height: Val::Px(58.0),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border: UiRect::all(Val::Px(3.0)),
                                ..default()
                            },
                            BorderColor(Color::srgba(1.0, 1.0, 1.0, 0.6)),
                            BorderRadius::all(Val::Px(12.0)),
                            BackgroundColor(Color::srgba(0.05, 0.1, 0.2, 0.92)),
                            WorldButton(w),
                        ))
                        .with_children(|b| {
                            b.spawn((
                                Text::new(format!("World {}", w)),
                                TextFont {
                                    font_size: 16.0,
                                    ..default()
                                },
                                TextColor(Color::WHITE),
                                WorldButtonText(w),
                            ));
                        });
                    }
                });
        });

    // --- pause menu (Escape) ---
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.05, 0.6)),
            PauseMenu,
        ))
        .with_children(|root| {
            root.spawn(Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(18.0),
                padding: UiRect::all(Val::Px(36.0)),
                ..default()
            })
            .with_children(|col| {
                col.spawn((
                    Text::new("PAUSED"),
                    TextFont {
                        font_size: 56.0,
                        ..default()
                    },
                    TextColor(Color::srgb(1.0, 0.92, 0.45)),
                    Node {
                        margin: UiRect::bottom(Val::Px(24.0)),
                        ..default()
                    },
                ));
                for (i, label) in ["CONTINUE PLAYING", "QUIT AND SAVE GAME"].iter().enumerate() {
                    col.spawn((
                        Button,
                        Node {
                            width: Val::Px(400.0),
                            height: Val::Px(72.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border: UiRect::all(Val::Px(3.0)),
                            ..default()
                        },
                        BorderColor(Color::srgba(1.0, 1.0, 1.0, 0.85)),
                        BorderRadius::all(Val::Px(14.0)),
                        BackgroundColor(Color::srgba(0.05, 0.12, 0.25, 0.9)),
                        PauseButton(i),
                    ))
                    .with_children(|b| {
                        b.spawn((
                            Text::new(*label),
                            TextFont {
                                font_size: 26.0,
                                ..default()
                            },
                            TextColor(Color::WHITE),
                        ));
                    });
                }
                col.spawn((
                    Text::new("Your game saves automatically every 15 seconds and when you quit."),
                    TextFont {
                        font_size: 17.0,
                        ..default()
                    },
                    TextColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
                    Node {
                        margin: UiRect::top(Val::Px(10.0)),
                        ..default()
                    },
                ));
            });
        });

    // --- main menu, on top of everything until you pick a mode ---
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            MainMenu,
        ))
        .with_children(|root| {
            root.spawn((
                ImageNode::new(menu_bg),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    right: Val::Px(0.0),
                    top: Val::Px(0.0),
                    bottom: Val::Px(0.0),
                    ..default()
                },
            ));
            root.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    right: Val::Px(0.0),
                    top: Val::Px(0.0),
                    bottom: Val::Px(0.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.05, 0.42)),
            ));
            root.spawn(Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(18.0),
                padding: UiRect::all(Val::Px(36.0)),
                ..default()
            })
            .with_children(|col| {
                col.spawn((
                    Text::new("EGG STEALER 3D"),
                    TextFont {
                        font_size: 72.0,
                        ..default()
                    },
                    TextColor(Color::srgb(1.0, 0.92, 0.45)),
                    Node {
                        margin: UiRect::bottom(Val::Px(30.0)),
                        ..default()
                    },
                ));
                for (i, label) in ["ONLINE PLAY", "MULTIPLAYER PLAY", "SINGLE PLAYER"]
                    .iter()
                    .enumerate()
                {
                    col.spawn((
                        Button,
                        Node {
                            width: Val::Px(360.0),
                            height: Val::Px(72.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border: UiRect::all(Val::Px(3.0)),
                            ..default()
                        },
                        BorderColor(Color::srgba(1.0, 1.0, 1.0, 0.85)),
                        BorderRadius::all(Val::Px(14.0)),
                        BackgroundColor(Color::srgba(0.05, 0.12, 0.25, 0.9)),
                        MenuButton(i),
                    ))
                    .with_children(|b| {
                        b.spawn((
                            Text::new(*label),
                            TextFont {
                                font_size: 28.0,
                                ..default()
                            },
                            TextColor(Color::WHITE),
                        ));
                    });
                }
                col.spawn((
                    Text::new(""),
                    TextFont {
                        font_size: 20.0,
                        ..default()
                    },
                    TextColor(Color::srgb(1.0, 0.8, 0.5)),
                    Node {
                        margin: UiRect::top(Val::Px(12.0)),
                        ..default()
                    },
                    MenuStatus,
                ));
            });
        });
}

// ---------- systems ----------

fn playing(game: Res<Game>) -> bool {
    !game.in_menu && !game.paused
}

// Escape pauses the game; Continue resumes, Quit saves through the normal exit path
fn pause_menu(
    keys: Res<ButtonInput<KeyCode>>,
    mut game: ResMut<Game>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut root_q: Query<&mut Node, With<PauseMenu>>,
    mut buttons: Query<
        (&Interaction, &PauseButton, &mut BackgroundColor),
        (With<Button>, Changed<Interaction>),
    >,
    mut exit: EventWriter<AppExit>,
) {
    if game.in_menu {
        return;
    }
    let mut resume = false;
    let mut open = false;
    if keys.just_pressed(KeyCode::Escape) {
        if game.paused {
            resume = true;
        } else if !game.menu_open {
            open = true;
        }
    }
    if game.paused {
        for (interaction, pb, mut bg) in buttons.iter_mut() {
            match *interaction {
                Interaction::Pressed => {
                    if pb.0 == 0 {
                        resume = true;
                    } else {
                        exit.write(AppExit::Success);
                    }
                }
                Interaction::Hovered => bg.0 = Color::srgba(0.15, 0.32, 0.55, 0.95),
                Interaction::None => bg.0 = Color::srgba(0.05, 0.12, 0.25, 0.9),
            }
        }
    }
    if open {
        game.paused = true;
        for mut node in root_q.iter_mut() {
            node.display = Display::Flex;
        }
        if let Ok(mut window) = windows.single_mut() {
            window.cursor_options.grab_mode = CursorGrabMode::None;
            window.cursor_options.visible = true;
        }
    } else if resume {
        game.paused = false;
        for mut node in root_q.iter_mut() {
            node.display = Display::None;
        }
        if let Ok(mut window) = windows.single_mut() {
            window.cursor_options.grab_mode = CursorGrabMode::Locked;
            window.cursor_options.visible = false;
        }
    }
}

fn main_menu(
    mut game: ResMut<Game>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut root_q: Query<&mut Node, With<MainMenu>>,
    mut buttons: Query<
        (&Interaction, &MenuButton, &mut BackgroundColor),
        (With<Button>, Changed<Interaction>),
    >,
    mut status: Query<&mut Text, With<MenuStatus>>,
) {
    if !game.in_menu {
        return;
    }
    for (interaction, mb, mut bg) in buttons.iter_mut() {
        match *interaction {
            Interaction::Pressed => {
                if mb.0 == 2 {
                    game.in_menu = false;
                    for mut node in root_q.iter_mut() {
                        node.display = Display::None;
                    }
                    if let Ok(mut window) = windows.single_mut() {
                        window.cursor_options.grab_mode = CursorGrabMode::Locked;
                        window.cursor_options.visible = false;
                    }
                    game.msg = "Steal eggs, hatch them for money, and unlock all 10 worlds!"
                        .to_string();
                    game.msg_color = Color::srgb(1.0, 0.95, 0.6);
                    game.msg_t = 6.0;
                } else {
                    let what = if mb.0 == 0 { "Online play" } else { "Multiplayer play" };
                    for mut t in status.iter_mut() {
                        t.0 = format!("{} is coming soon! Pick Single Player for now.", what);
                    }
                }
            }
            Interaction::Hovered => bg.0 = Color::srgba(0.15, 0.32, 0.55, 0.95),
            Interaction::None => bg.0 = Color::srgba(0.05, 0.12, 0.25, 0.9),
        }
    }
}

fn cursor_grab(
    game: Res<Game>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    if mouse.just_pressed(MouseButton::Left) && !game.menu_open {
        window.cursor_options.grab_mode = CursorGrabMode::Locked;
        window.cursor_options.visible = false;
    }
    if keys.just_pressed(KeyCode::Escape) {
        window.cursor_options.grab_mode = CursorGrabMode::None;
        window.cursor_options.visible = true;
    }
}

fn player_look(
    mut motion: EventReader<MouseMotion>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut q: Query<(&mut Transform, &mut Player)>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let grabbed = window.cursor_options.grab_mode == CursorGrabMode::Locked;
    let Ok((mut tf, mut pl)) = q.single_mut() else {
        return;
    };
    for ev in motion.read() {
        if !grabbed {
            continue;
        }
        pl.yaw -= ev.delta.x * 0.0024;
        pl.pitch = (pl.pitch - ev.delta.y * 0.0024).clamp(-1.45, 1.45);
    }
    tf.rotation = Quat::from_euler(EulerRot::YXZ, pl.yaw, pl.pitch, 0.0);
}

// inside the safe zone you always move at exactly this speed
const SAFE_SPEED_KMH: f32 = 27.3;

// how fast training has made you (used past the line and for the belt)
fn trained_speed(game: &Game) -> f32 {
    let mut s = 4.2 + game.training * 0.06;
    if game.has_eggs() {
        s *= 0.92;
    }
    s
}

fn player_speed(game: &Game, p: Vec3) -> f32 {
    if p.x < LINE_X {
        SAFE_SPEED_KMH / 3.6
    } else {
        trained_speed(game)
    }
}

fn player_move(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut game: ResMut<Game>,
    mut q: Query<(&mut Transform, &Player)>,
) {
    let Ok((mut tf, pl)) = q.single_mut() else {
        return;
    };
    let dt = time.delta_secs();
    let fwd = Vec3::new(-pl.yaw.sin(), 0.0, -pl.yaw.cos());
    let right = Vec3::new(pl.yaw.cos(), 0.0, -pl.yaw.sin());
    let mut dir = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) {
        dir += fwd;
    }
    if keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown) {
        dir -= fwd;
    }
    if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
        dir += right;
    }
    if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
        dir -= right;
    }
    let moving = dir.length_squared() > 0.0;
    let delta = if moving {
        dir.normalize() * player_speed(&game, tf.translation) * dt
    } else {
        Vec3::ZERO
    };
    let old_x = tf.translation.x;
    move_blocked(&mut tf.translation, delta, &blockers());
    // at night the line is sealed: you can always come home, but not go out
    let wall_x = LINE_X - PLAYER_RADIUS - 0.25;
    if game.night && old_x < LINE_X && tf.translation.x > wall_x {
        tf.translation.x = wall_x;
    }
    tf.translation.x = tf.translation.x.clamp(0.6, WORLD_X - 0.6);
    tf.translation.z = tf.translation.z.clamp(0.6, WORLD_Z - 0.6);

    // treadmill: stand on it to train - no cap, you can always get faster
    let p = tf.translation;
    game.on_treadmill = on_deck(p);
    if game.on_treadmill {
        game.training += dt * T_MULT[game.tier] * 0.5;
    }

    // eye height: the belt deck is raised, so ease up/down when stepping on or off
    let target = EYE + if on_deck(p) { TM_DECK_H } else { 0.0 };
    game.eye_h += (target - game.eye_h) * (dt * 14.0).min(1.0);
    // head bob
    let t = time.elapsed_secs();
    tf.translation.y = game.eye_h
        + if moving {
            (t * (5.0 + player_speed(&game, tf.translation) * 0.6)).sin() * 0.035
        } else {
            0.0
        };
}

// number keys 1-9 and 0 select inventory slots 1-10
fn select_slot(keys: Res<ButtonInput<KeyCode>>, mut game: ResMut<Game>) {
    const KEYS: [KeyCode; INV_SLOTS] = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
        KeyCode::Digit0,
    ];
    for (i, k) in KEYS.iter().enumerate() {
        if keys.just_pressed(*k) {
            game.selected = i;
        }
    }
}

// keep the egg shown in your hand in sync with the selected slot
fn held_egg(
    mut commands: Commands,
    mut game: ResMut<Game>,
    player_q: Query<Entity, With<Player>>,
    carried_q: Query<Entity, With<CarriedEgg>>,
) {
    let want = game.inventory[game.selected];
    if want == game.held_kind {
        return;
    }
    for e in carried_q.iter() {
        commands.entity(e).despawn();
    }
    if let (Some(kind), Ok(player)) = (want, player_q.single()) {
        let mesh = game.egg_mesh.clone();
        let mat = game.egg_mats[kind].clone();
        commands.entity(player).with_children(|c| {
            c.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(mat),
                Transform::from_xyz(0.40, -0.36, -0.78).with_scale(Vec3::splat(0.36)),
                CarriedEgg,
            ));
        });
    }
    game.held_kind = want;
}

fn gameplay(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut game: ResMut<Game>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    player_q: Query<(Entity, &Transform), With<Player>>,
    eggs_q: Query<(Entity, &WorldEgg, &Transform)>,
    hatching_q: Query<&Hatching>,
) {
    let Ok((_player_ent, ptf)) = player_q.single() else {
        return;
    };
    let dt = time.delta_secs();
    let p = ptf.translation;

    // pick up eggs into the inventory slots (selected slot first)
    if game.free_slot().is_some() {
        for (ent, egg, etf) in eggs_q.iter() {
            let Some(slot) = game.free_slot() else {
                break;
            };
            let d = (etf.translation - p).xz().length();
            if d < 1.5 {
                commands.entity(ent).despawn();
                commands.spawn((
                    AudioPlayer::new(game.pickup_sound.clone()),
                    PlaybackSettings::DESPAWN,
                ));
                game.inventory[slot] = Some(egg.kind);
                let (rn, rc) = rarity(egg.kind);
                game.msg = format!(
                    "Stole a {} {}! ({}/{})",
                    rn,
                    egg_name(egg.kind),
                    game.carrying(),
                    INV_SLOTS
                );
                game.msg_color = rc;
                game.msg_t = 2.5;
            }
        }
    } else if game.msg_t <= 0.0 {
        for (_, _, etf) in eggs_q.iter() {
            if (etf.translation - p).xz().length() < 1.5 {
                game.msg = format!("All {} slots full - run home and hatch them!", INV_SLOTS);
                game.msg_color = Color::srgb(1.0, 0.9, 0.4);
                game.msg_t = 2.0;
                break;
            }
        }
    }
    // deposit in your yard -> eggs start hatching into pets
    if game.has_eggs() && p.x > YARD.0 && p.x < YARD.1 && p.z > YARD.2 && p.z < YARD.3 {
        let mult = level_mult(game.level);
        let mut bonus = 0.0;
        let inv = game.take_all();
        let n = inv.len();
        let now = now_unix();
        let mut soonest = f64::MAX;
        let mut occupied = [false; YARD_MAX_SLOTS];
        for h in hatching_q.iter() {
            if h.slot < YARD_MAX_SLOTS {
                occupied[h.slot] = true;
            }
        }
        for k in inv {
            bonus += egg_value(k) * 10.0 * mult;
            match occupied.iter().position(|o| !o) {
                Some(slot) => {
                    occupied[slot] = true;
                    let wait = hatch_secs(k);
                    soonest = soonest.min(wait);
                    spawn_hatching(&mut commands, &game, k, slot, now + wait);
                }
                None => {
                    // no room left in the yard: cash it in right away
                    bonus += hatch_payout(k, game.level);
                    game.yard_eggs.push(k);
                }
            }
        }
        game.money += bonus;
        game.msg = format!(
            "+${} - {} egg{} hatching in your yard{}",
            bonus as u64,
            n,
            if n == 1 { " is" } else { "s are" },
            if soonest < f64::MAX {
                format!(" (first one in {})", fmt_dur(soonest))
            } else {
                String::new()
            }
        );
        game.msg_color = Color::srgb(0.4, 1.0, 0.5);
        game.msg_t = 3.0;
    }

    // upgrade treadmill
    if keys.just_pressed(KeyCode::KeyE) && game.tier < 6 {
        let near_tm = (0..4).any(|yi| {
            let c = treadmill_center(yi);
            (p - Vec3::new(c.x, p.y, c.z)).length() < 4.5
        });
        if near_tm {
            let cost = T_COSTS[game.tier + 1];
            if game.money >= cost {
                game.money -= cost;
                game.tier += 1;
                let (r, g, b) = T_RGB[game.tier];
                if let Some(m) = materials.get_mut(&game.panel_mat) {
                    let (c, e) = panel_colors(game.tier);
                    m.base_color = c;
                    m.emissive = e;
                }
                game.msg = format!("{} Treadmill unlocked!", T_NAMES[game.tier]);
                game.msg_color = Color::srgb(r, g, b);
                game.msg_t = 3.0;
            } else {
                game.msg = format!(
                    "Need ${} for the {} Treadmill!",
                    cost as u64,
                    T_NAMES[game.tier + 1]
                );
                game.msg_color = Color::srgb(1.0, 0.4, 0.4);
                game.msg_t = 2.0;
            }
        }
    }


    // egg respawn
    game.spawn_t -= dt;
    if game.spawn_t <= 0.0 {
        game.spawn_t = 1.2;
        if eggs_q.iter().count() < 60 {
            let (x, z) = random_egg_xz();
            spawn_world_egg(&mut commands, &game, x, z, random_kind());
        }
    }

    if game.msg_t > 0.0 {
        game.msg_t -= dt;
    }
}

fn monsters_ai(
    time: Res<Time>,
    mut commands: Commands,
    mut game: ResMut<Game>,
    mut player_q: Query<&mut Transform, With<Player>>,
    mut mq: Query<(Entity, &mut Monster, &mut Transform, &Children), Without<Player>>,
    ice_q: Query<Entity, With<IceBlock>>,
    mut eyes: Query<&mut MeshMaterial3d<StandardMaterial>, With<MonsterEye>>,
    mut limbs: Query<(&mut Transform, &MonsterLimb), (Without<Monster>, Without<Player>)>,
) {
    let Ok(mut ptf) = player_q.single_mut() else {
        return;
    };
    let dt = time.delta_secs();
    let t = time.elapsed_secs();
    let p = ptf.translation;
    // monsters hunt anyone carrying eggs, and at night anyone past the line
    let night = game.night;
    let hunting = p.x > LINE_X && (game.has_eggs() || night);
    let mut caught = false;
    for (ment, mut m, mut tf, children) in mq.iter_mut() {
        // frozen solid: no movement, no breathing, no catching
        let has_ice = children.iter().any(|c| ice_q.get(c).is_ok());
        if m.frozen > 0.0 {
            m.frozen -= dt;
            if !has_ice {
                commands.entity(ment).with_children(|p| {
                    p.spawn((
                        Mesh3d(game.cube_mesh.clone()),
                        MeshMaterial3d(game.ice_mat.clone()),
                        Transform::from_xyz(0.0, 1.3, 0.0).with_scale(Vec3::new(2.4, 2.8, 2.4)),
                        IceBlock,
                    ));
                });
            }
            continue;
        } else if has_ice {
            for c in children.iter() {
                if ice_q.get(c).is_ok() {
                    commands.entity(c).despawn();
                }
            }
        }
        let mut walking = 0.0f32;
        if hunting {
            m.awake = (m.awake + dt * 1.5).min(1.0);
            if m.awake > 0.5 {
                let mut to_p = p - tf.translation;
                to_p.y = 0.0;
                let d = to_p.length();
                let mspeed = m.speed * (1.0 + 0.15 * (game.level as f32 - 1.0));
                if d > 0.01 {
                    let step = to_p / d * mspeed * dt;
                    tf.translation += step;
                    let face = tf.translation + Vec3::new(to_p.x, 0.0, to_p.z);
                    tf.look_at(face, Vec3::Y);
                    walking = 1.0;
                }
                if d < 2.8 * m.scale {
                    caught = true;
                }
            }
        } else {
            // at night they stay wide awake, pacing at home
            m.awake = if night {
                (m.awake + dt * 1.5).min(1.0)
            } else {
                (m.awake - dt * 1.2).max(0.0)
            };
            let mut home = m.home - tf.translation;
            home.y = 0.0;
            let d = home.length();
            if d > 0.5 {
                tf.translation += home / d * (3.0 * dt).min(d);
                let face = tf.translation + Vec3::new(home.x, 0.0, home.z);
                tf.look_at(face, Vec3::Y);
                walking = 0.55;
            }
        }
        tf.translation.x = tf.translation.x.clamp(LINE_X + 3.0, WORLD_X - 4.0);
        tf.translation.z = tf.translation.z.clamp(4.0, WORLD_Z - 4.0);
        // breathing / lumbering animation
        let breathe = if m.awake < 0.5 {
            1.0 + ((t * 1.6 + m.phase).sin()) * 0.03
        } else {
            1.0 + ((t * 7.0 + m.phase).sin()) * 0.02
        };
        tf.scale = Vec3::splat(m.scale * breathe);
        // eyes + swinging limbs
        let open = m.awake > 0.5;
        for child in children.iter() {
            if let Ok(mut mat) = eyes.get_mut(child) {
                let want = if open {
                    game.eye_open.clone()
                } else {
                    game.eye_closed.clone()
                };
                if mat.0 != want {
                    mat.0 = want;
                }
            }
            if let Ok((mut ltf, limb)) = limbs.get_mut(child) {
                let swing = (t * 7.0 + limb.phase + m.phase).sin() * 0.7 * walking;
                ltf.rotation =
                    Quat::from_rotation_z(limb.base_z) * Quat::from_rotation_x(swing);
            }
        }
    }

    if caught {
        let inv = game.take_all();
        let n = inv.len();
        for kind in inv {
            let (x, z) = random_egg_xz();
            spawn_world_egg(&mut commands, &game, x, z, kind);
        }
        ptf.translation = SPAWN;
        commands.spawn((
            AudioPlayer::new(game.defeat_sound.clone()),
            PlaybackSettings::DESPAWN,
        ));
        game.msg = if n > 0 {
            format!(
                "CAUGHT! The monster took your {} egg{} back!",
                n,
                if n == 1 { "" } else { "s" }
            )
        } else {
            "CAUGHT! The monster threw you back home!".to_string()
        };
        game.msg_color = Color::srgb(1.0, 0.25, 0.25);
        game.msg_t = 3.5;
    }
}

fn animate(
    time: Res<Time>,
    game: Res<Game>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut carried: Query<&mut Transform, (With<CarriedEgg>, Without<PortalDisc>)>,
    mut portal: Query<&mut Transform, (With<PortalDisc>, Without<CarriedEgg>)>,
) {
    let t = time.elapsed_secs();
    let dt = time.delta_secs();
    // belt scroll
    let belt_speed = if game.on_treadmill {
        0.6 + trained_speed(&game) * 0.25
    } else {
        0.35
    };
    if let Some(m) = materials.get_mut(&game.belt_mat) {
        m.uv_transform.translation.x = (m.uv_transform.translation.x + dt * belt_speed) % 1.0;
    }
    // the held egg bobs gently in your hand
    for mut tf in carried.iter_mut() {
        tf.translation = Vec3::new(0.40, -0.36 + (t * 4.0).sin() * 0.012, -0.78);
        tf.rotation = Quat::from_rotation_y(t * 0.8) * Quat::from_rotation_z(0.25);
    }
    // portal vortex spin
    for mut tf in portal.iter_mut() {
        tf.rotation = Quat::from_rotation_x(t * 1.3) * Quat::from_rotation_y(PI / 2.0);
    }
}

fn hatching(
    time: Res<Time>,
    mut commands: Commands,
    mut game: ResMut<Game>,
    mut hatching: Query<(Entity, &Hatching, &mut Transform), Without<ShellBit>>,
    mut bits: Query<(Entity, &mut ShellBit), Without<Hatching>>,
) {
    let dt = time.delta_secs();
    let t = time.elapsed_secs();
    let now = now_unix();
    for (ent, h, mut tf) in hatching.iter_mut() {
        let rem = (h.hatch_at - now) as f32;
        // wobbles harder during the last ten seconds
        let urgency = ((10.0 - rem.max(0.0)) / 10.0).clamp(0.0, 1.0);
        tf.rotation =
            Quat::from_rotation_z((t * 18.0).sin() * 0.14 * urgency) * Quat::from_rotation_y(t * 0.3);
        if rem <= 0.0 {
            let pos = tf.translation;
            let kind = h.kind;
            commands.entity(ent).despawn();
            // cracked shell pieces
            for _ in 0..3 {
                commands.spawn((
                    Mesh3d(game.sphere_mesh.clone()),
                    MeshMaterial3d(game.egg_mats[kind].clone()),
                    Transform::from_xyz(
                        pos.x + (fastrand::f32() - 0.5) * 0.9,
                        0.07,
                        pos.z + (fastrand::f32() - 0.5) * 0.9,
                    )
                    .with_scale(Vec3::new(0.14, 0.05, 0.14)),
                    ShellBit { t: 12.0 },
                ));
            }
            // the egg pays out
            let payout = hatch_payout(kind, game.level);
            game.money += payout;
            game.yard_eggs.push(kind);
            let (_, rc) = rarity(kind);
            game.msg = format!("+${} - your {} hatched!", fmt_money(payout), egg_name(kind));
            game.msg_color = rc;
            game.msg_t = 4.0;
        }
    }
    for (ent, mut b) in bits.iter_mut() {
        b.t -= dt;
        if b.t <= 0.0 {
            commands.entity(ent).despawn();
        }
    }
}

struct Theme {
    safe: (f32, f32, f32),
    zone: (f32, f32, f32),
    zenith: (f32, f32, f32),
    horizon: (f32, f32, f32),
    visibility: f32,
    lux: f32,
    sun: (f32, f32, f32),
    disc: (f32, f32, f32),
    ambient: f32,
}

fn theme(level: usize, night: bool) -> Theme {
    // (safe grass, egg-zone grass, sky zenith, horizon/fog, visibility, sun lux, sun colour, sun disc, ambient)
    let rows: [(
        (f32, f32, f32),
        (f32, f32, f32),
        (f32, f32, f32),
        (f32, f32, f32),
        f32,
        f32,
        (f32, f32, f32),
        (f32, f32, f32),
        f32,
    ); N_WORLDS] = [
        // 1 meadow
        ((1.0, 1.0, 1.0), (0.80, 0.84, 0.78), (0.22, 0.44, 0.84), (0.70, 0.80, 0.92), 320.0, 11000.0, (1.0, 0.95, 0.86), (40.0, 36.0, 28.0), 320.0),
        // 2 sunset
        ((1.0, 0.86, 0.60), (0.95, 0.76, 0.50), (0.30, 0.32, 0.58), (0.96, 0.62, 0.38), 220.0, 7000.0, (1.0, 0.78, 0.55), (40.0, 16.0, 5.0), 220.0),
        // 3 twilight
        ((0.62, 0.55, 0.95), (0.50, 0.42, 0.88), (0.06, 0.05, 0.16), (0.20, 0.16, 0.36), 160.0, 3200.0, (0.70, 0.74, 1.0), (4.0, 4.4, 6.0), 170.0),
        // 4 autumn
        ((1.0, 0.72, 0.38), (0.92, 0.60, 0.32), (0.40, 0.42, 0.70), (0.95, 0.78, 0.55), 240.0, 8500.0, (1.0, 0.85, 0.65), (38.0, 26.0, 12.0), 260.0),
        // 5 winter
        ((1.0, 1.0, 1.0), (0.94, 0.97, 1.0), (0.55, 0.65, 0.80), (0.88, 0.92, 0.97), 180.0, 7000.0, (0.90, 0.95, 1.0), (30.0, 32.0, 36.0), 380.0),
        // 6 jungle
        ((0.55, 0.95, 0.55), (0.40, 0.80, 0.45), (0.10, 0.45, 0.50), (0.55, 0.85, 0.75), 140.0, 9000.0, (0.95, 1.0, 0.85), (36.0, 40.0, 26.0), 300.0),
        // 7 desert
        ((1.0, 0.92, 0.62), (0.98, 0.85, 0.55), (0.45, 0.62, 0.90), (0.98, 0.90, 0.70), 400.0, 13000.0, (1.0, 0.98, 0.88), (46.0, 42.0, 30.0), 340.0),
        // 8 lava
        ((0.75, 0.30, 0.20), (0.60, 0.20, 0.15), (0.15, 0.03, 0.03), (0.70, 0.18, 0.08), 120.0, 5000.0, (1.0, 0.55, 0.35), (40.0, 10.0, 3.0), 200.0),
        // 9 alien ocean
        ((0.45, 0.75, 1.0), (0.35, 0.60, 0.95), (0.05, 0.25, 0.45), (0.35, 0.85, 0.95), 200.0, 8000.0, (0.75, 0.95, 1.0), (20.0, 40.0, 44.0), 300.0),
        // 10 hacker
        ((0.30, 0.70, 0.35), (0.20, 0.55, 0.28), (0.01, 0.06, 0.02), (0.05, 0.30, 0.10), 150.0, 3600.0, (0.60, 1.0, 0.65), (6.0, 30.0, 8.0), 220.0),
    ];
    let r = rows[level.clamp(1, N_WORLDS) - 1];
    let mut t = Theme {
        safe: r.0,
        zone: r.1,
        zenith: r.2,
        horizon: r.3,
        visibility: r.4,
        lux: r.5,
        sun: r.6,
        disc: r.7,
        ambient: r.8,
    };
    if night {
        t.zenith = (0.02, 0.02, 0.07);
        t.horizon = (0.12, 0.10, 0.24);
        t.visibility = 140.0;
        t.lux = 2600.0;
        t.sun = (0.70, 0.74, 1.0);
        t.disc = (2.2, 2.4, 3.0);
        t.ambient = 140.0;
    }
    t
}

const SUN_FROM: Vec3 = Vec3::new(60.0, 90.0, 150.0);
const SUN_AT: Vec3 = Vec3::new(130.0, 0.0, 50.0);
const SKY_RADIUS: f32 = 900.0;
const SKY_CENTER: Vec3 = Vec3::new(WORLD_X / 2.0, 0.0, WORLD_Z / 2.0);

fn sun_dir() -> Vec3 {
    (SUN_AT - SUN_FROM).normalize()
}

fn fog_for(level: usize, night: bool) -> DistanceFog {
    let t = theme(level, night);
    let horizon = Color::srgb(t.horizon.0, t.horizon.1, t.horizon.2);
    let glow = if night || matches!(level, 3 | 8 | 10) { 0.15 } else { 0.55 };
    DistanceFog {
        color: horizon,
        directional_light_color: Color::srgba(t.sun.0, t.sun.1, t.sun.2, glow),
        directional_light_exponent: 24.0,
        falloff: FogFalloff::from_visibility_colors(
            t.visibility,
            Color::srgb(0.35, 0.5, 0.66),
            horizon,
        ),
    }
}

// Vertex-coloured gradient dome: horizon colour at the rim, zenith colour overhead.
fn set_sky_colors(mesh: &mut Mesh, level: usize, night: bool) {
    let t = theme(level, night);
    let zen = Color::srgb(t.zenith.0, t.zenith.1, t.zenith.2).to_linear();
    let hor = Color::srgb(t.horizon.0, t.horizon.1, t.horizon.2).to_linear();
    let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return;
    };
    let colors: Vec<[f32; 4]> = pos
        .iter()
        .map(|p| {
            let h = (p[1] / SKY_RADIUS).clamp(-1.0, 1.0);
            let k = if h > 0.0 { h.powf(0.6) } else { 0.0 };
            let d = if h < 0.0 { 1.0 + h * 0.5 } else { 1.0 };
            [
                (hor.red + (zen.red - hor.red) * k) * d,
                (hor.green + (zen.green - hor.green) * k) * d,
                (hor.blue + (zen.blue - hor.blue) * k) * d,
                1.0,
            ]
        })
        .collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
}

fn sky_dome_mesh(level: usize, night: bool) -> Mesh {
    let mut mesh = Mesh::from(Sphere::new(SKY_RADIUS).mesh().uv(48, 24));
    set_sky_colors(&mut mesh, level, night);
    mesh
}

fn apply_level_theme(
    level: usize,
    night: bool,
    game: &Game,
    materials: &mut Assets<StandardMaterial>,
    meshes: &mut Assets<Mesh>,
    clear: &mut ClearColor,
    ambient: &mut AmbientLight,
    fog_q: &mut Query<&mut DistanceFog>,
    sun_q: &mut Query<&mut DirectionalLight>,
) {
    let t = theme(level, night);
    if let Some(m) = materials.get_mut(&game.safe_grass_mat) {
        m.base_color = Color::srgb(t.safe.0, t.safe.1, t.safe.2);
    }
    if let Some(m) = materials.get_mut(&game.zone_grass_mat) {
        m.base_color = Color::srgb(t.zone.0, t.zone.1, t.zone.2);
    }
    if let Some(m) = materials.get_mut(&game.sun_mat) {
        m.emissive = LinearRgba::rgb(t.disc.0, t.disc.1, t.disc.2);
    }
    if let Some(mesh) = meshes.get_mut(&game.sky_mesh) {
        set_sky_colors(mesh, level, night);
    }
    clear.0 = Color::srgb(t.horizon.0, t.horizon.1, t.horizon.2);
    ambient.brightness = t.ambient;
    for mut f in fog_q.iter_mut() {
        *f = fog_for(level, night);
    }
    for mut s in sun_q.iter_mut() {
        s.illuminance = t.lux;
        s.color = Color::srgb(t.sun.0, t.sun.1, t.sun.2);
    }
}

fn portal_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut game: ResMut<Game>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    player_q: Query<&Transform, With<Player>>,
) {
    let Ok(ptf) = player_q.single() else {
        return;
    };
    // glow when the next world is affordable; gold once you are EGG MASTER
    let lit = if game.won {
        true
    } else {
        match next_locked(&game.unlocked) {
            Some(w) => game.money >= LEVEL_COST[w - 2],
            None => true,
        }
    };
    if lit != game.portal_lit {
        game.portal_lit = lit;
        if let Some(m) = materials.get_mut(&game.portal_mat) {
            m.emissive = if game.won {
                LinearRgba::rgb(3.0, 2.4, 0.6)
            } else if lit {
                LinearRgba::rgb(1.2, 3.0, 4.0)
            } else {
                LinearRgba::rgb(0.12, 0.25, 0.4)
            };
        }
    }
    // beat the game by reaching WIN_MONEY in the last world
    if game.level == N_WORLDS && !game.won && game.money >= WIN_MONEY {
        game.won = true;
        if let Some(m) = materials.get_mut(&game.portal_mat) {
            m.emissive = LinearRgba::rgb(3.0, 2.4, 0.6);
        }
        game.msg = format!(
            "${} IN WORLD {} - YOU BEAT THE GAME! EGG MASTER!",
            fmt_money(WIN_MONEY),
            N_WORLDS
        );
        game.msg_color = Color::srgb(1.0, 0.85, 0.2);
        game.msg_t = 12.0;
    }
    // press E at any yard's portal to open the world-select bar; walking away closes it
    let p = ptf.translation;
    if near_portal(p) && keys.just_pressed(KeyCode::KeyE) && !game.menu_open {
        game.menu_open = true;
    } else if game.menu_open && !near_portal(p) {
        game.menu_open = false;
    }
}

fn portal_menu(
    keys: Res<ButtonInput<KeyCode>>,
    mut menu_q: Query<&mut Node, With<PortalMenu>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut was_open: Local<bool>,
    mut game: ResMut<Game>,
) {
    if game.menu_open && keys.just_pressed(KeyCode::Escape) {
        game.menu_open = false;
    }
    if game.menu_open == *was_open {
        return;
    }
    *was_open = game.menu_open;
    for mut node in menu_q.iter_mut() {
        node.display = if game.menu_open {
            Display::Flex
        } else {
            Display::None
        };
    }
    if let Ok(mut window) = windows.single_mut() {
        if game.menu_open {
            window.cursor_options.grab_mode = CursorGrabMode::None;
            window.cursor_options.visible = true;
        } else {
            window.cursor_options.grab_mode = CursorGrabMode::Locked;
            window.cursor_options.visible = false;
        }
    }
}

fn world_buttons(
    mut game: ResMut<Game>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut clear: ResMut<ClearColor>,
    mut ambient: ResMut<AmbientLight>,
    mut player_q: Query<&mut Transform, With<Player>>,
    mut fog_q: Query<&mut DistanceFog>,
    mut sun_q: Query<&mut DirectionalLight>,
    mut buttons: Query<(&Interaction, &WorldButton, &mut BackgroundColor), With<Button>>,
    mut borders: Query<(&WorldButton, &mut BorderColor)>,
    mut labels: Query<(&mut Text, &WorldButtonText)>,
) {
    // keep labels + borders current
    for (mut text, wt) in labels.iter_mut() {
        let w = wt.0;
        let s = if game.unlocked[w - 1] {
            if w == game.level {
                format!("World {} (here)", w)
            } else {
                format!("World {}", w)
            }
        } else {
            format!("World {} - ${}", w, fmt_money(LEVEL_COST[w - 2]))
        };
        if text.0 != s {
            text.0 = s;
        }
    }
    for (wb, mut bc) in borders.iter_mut() {
        bc.0 = if wb.0 == game.level {
            Color::srgb(1.0, 0.85, 0.2)
        } else if game.unlocked[wb.0 - 1] {
            Color::srgba(1.0, 1.0, 1.0, 0.8)
        } else {
            Color::srgba(1.0, 1.0, 1.0, 0.3)
        };
    }
    if !game.menu_open {
        return;
    }
    let mut travel_to: Option<usize> = None;
    for (interaction, wb, mut bg) in buttons.iter_mut() {
        match *interaction {
            Interaction::Pressed => {
                let w = wb.0;
                if w == game.level {
                    game.msg = format!("You are already in World {}!", w);
                    game.msg_color = Color::srgb(1.0, 1.0, 0.7);
                    game.msg_t = 2.5;
                    game.menu_open = false;
                } else if game.unlocked[w - 1] {
                    travel_to = Some(w);
                    game.msg = format!("Welcome back to World {}!", w);
                    game.msg_color = Color::srgb(0.5, 1.0, 1.0);
                    game.msg_t = 4.0;
                } else if w > 2 && !game.unlocked[w - 2] {
                    game.msg = format!("Unlock World {} first!", w - 1);
                    game.msg_color = Color::srgb(1.0, 0.5, 0.5);
                    game.msg_t = 2.5;
                } else {
                    let cost = LEVEL_COST[w - 2];
                    if game.money >= cost {
                        game.money -= cost;
                        game.unlocked[w - 1] = true;
                        travel_to = Some(w);
                        game.msg = format!(
                            "WORLD {} UNLOCKED! Eggs are worth {}x here!",
                            w,
                            fmt_money(level_mult(w))
                        );
                        game.msg_color = Color::srgb(0.5, 1.0, 1.0);
                        game.msg_t = 6.0;
                    } else {
                        game.msg = format!(
                            "World {} needs ${} (you have ${})",
                            w,
                            fmt_money(cost),
                            fmt_money(game.money)
                        );
                        game.msg_color = Color::srgb(1.0, 0.5, 0.5);
                        game.msg_t = 2.5;
                    }
                }
            }
            Interaction::Hovered => bg.0 = Color::srgba(0.15, 0.3, 0.5, 0.95),
            Interaction::None => bg.0 = Color::srgba(0.05, 0.1, 0.2, 0.92),
        }
    }
    if let Some(w) = travel_to {
        game.level = w;
        apply_level_theme(
            w,
            game.night,
            &game,
            &mut materials,
            &mut meshes,
            &mut clear,
            &mut ambient,
            &mut fog_q,
            &mut sun_q,
        );
        if let Ok(mut ptf) = player_q.single_mut() {
            ptf.translation = SPAWN;
        }
        game.menu_open = false;
    }
}

fn day_night(
    mut game: ResMut<Game>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut clear: ResMut<ClearColor>,
    mut ambient: ResMut<AmbientLight>,
    mut fog_q: Query<&mut DistanceFog>,
    mut sun_q: Query<&mut DirectionalLight>,
    mut wall_q: Query<&mut Visibility, With<NightWall>>,
) {
    let night = is_night_at(now_unix());
    if game.night_applied == Some(night) {
        return;
    }
    let first = game.night_applied.is_none();
    game.night = night;
    game.night_applied = Some(night);
    apply_level_theme(
        game.level,
        night,
        &game,
        &mut materials,
        &mut meshes,
        &mut clear,
        &mut ambient,
        &mut fog_q,
        &mut sun_q,
    );
    for mut v in wall_q.iter_mut() {
        *v = if night {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    if !first {
        if night {
            game.msg = "NIGHT HAS FALLEN! The monsters are awake and the line is sealed until morning."
                .to_string();
            game.msg_color = Color::srgb(1.0, 0.3, 0.3);
        } else {
            game.msg = "GOOD MORNING! The line is open again - go steal some eggs!".to_string();
            game.msg_color = Color::srgb(1.0, 0.9, 0.4);
        }
        game.msg_t = 6.0;
    }
}

// buy gear with [E] at the Gear Station, use it with F / G / H
fn gear_system(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut game: ResMut<Game>,
    player_q: Query<&Transform, With<Player>>,
    mut monsters: Query<(&mut Monster, &Transform), Without<Player>>,
) {
    let dt = time.delta_secs();
    for cd in game.gear_cd.iter_mut() {
        *cd = (*cd - dt).max(0.0);
    }
    let Ok(ptf) = player_q.single() else {
        return;
    };
    let p = ptf.translation;
    if keys.just_pressed(KeyCode::KeyE) && near_gear(p) && game.gear < 3 {
        let i = game.gear;
        if game.money >= GEAR_COSTS[i] {
            game.money -= GEAR_COSTS[i];
            game.gear += 1;
            game.msg = format!(
                "{} bought! Press [{}] to freeze {} for {} s.",
                GEAR_NAMES[i],
                GEAR_KEY_NAMES[i],
                match i {
                    0 => "the nearest monster",
                    1 => "every monster near you",
                    _ => "every monster in the world",
                },
                GEAR_FREEZE[i] as u32
            );
            game.msg_color = Color::srgb(0.6, 0.9, 1.0);
        } else {
            game.msg = format!("Need ${} for the {}!", GEAR_COSTS[i] as u64, GEAR_NAMES[i]);
            game.msg_color = Color::srgb(1.0, 0.4, 0.4);
        }
        game.msg_t = 3.0;
    }
    for i in 0..3 {
        if i >= game.gear || !keys.just_pressed(GEAR_KEYS[i]) {
            continue;
        }
        if game.gear_cd[i] > 0.0 {
            game.msg = format!("{} recharging: {:.0} s", GEAR_NAMES[i], game.gear_cd[i]);
            game.msg_color = Color::srgb(0.7, 0.8, 1.0);
            game.msg_t = 1.5;
            continue;
        }
        let mut hit = 0;
        if i == 0 {
            // freeze gun: the nearest monster in range
            let mut best: Option<(f32, Mut<Monster>)> = None;
            for (m, tf) in monsters.iter_mut() {
                let d = (tf.translation - p).xz().length();
                if d < GEAR_RANGE[0] && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                    best = Some((d, m));
                }
            }
            if let Some((_, mut m)) = best {
                m.frozen = m.frozen.max(GEAR_FREEZE[0]);
                hit = 1;
            }
        } else {
            for (mut m, tf) in monsters.iter_mut() {
                if (tf.translation - p).xz().length() < GEAR_RANGE[i] {
                    m.frozen = m.frozen.max(GEAR_FREEZE[i]);
                    hit += 1;
                }
            }
        }
        if hit > 0 {
            game.gear_cd[i] = GEAR_COOLDOWN[i];
            game.msg = format!(
                "{}: froze {} monster{} for {} s!",
                GEAR_NAMES[i],
                hit,
                if hit == 1 { "" } else { "s" },
                GEAR_FREEZE[i] as u32
            );
            game.msg_color = Color::srgb(0.6, 0.9, 1.0);
        } else {
            game.msg = format!("{}: no monster in range!", GEAR_NAMES[i]);
            game.msg_color = Color::srgb(1.0, 0.6, 0.4);
        }
        game.msg_t = 2.5;
    }
}

// autosave every 15 s and on exit
fn save_system(
    time: Res<Time>,
    game: Res<Game>,
    hatching: Query<&Hatching>,
    mut exit: EventReader<AppExit>,
    mut timer: Local<f32>,
) {
    *timer += time.delta_secs();
    let exiting = exit.read().next().is_some();
    if !exiting && *timer < 15.0 {
        return;
    }
    *timer = 0.0;
    let data = SaveData {
        money: game.money,
        training: game.training,
        tier: game.tier,
        level: game.level,
        unlocked: game.unlocked.to_vec(),
        won: game.won,
        inventory: game.inventory.to_vec(),
        gear: game.gear,
        yard_eggs: game.yard_eggs.clone(),
        hatching: hatching.iter().map(|h| (h.kind, h.slot, h.hatch_at)).collect(),
    };
    if let Ok(text) = serde_json::to_string(&data) {
        if let Err(e) = std::fs::write(save_path(), text) {
            warn!("could not save game: {e}");
        }
    }
}

fn hud(
    game: Res<Game>,
    player_q: Query<&Transform, With<Player>>,
    hatching_q: Query<&Hatching>,
    mut q: Query<(&mut Text, &mut TextColor, &Hud), Without<SlotText>>,
    mut slots: Query<(&mut BackgroundColor, &mut BorderColor, &SlotUi)>,
    mut slot_texts: Query<(&mut Text, &mut TextColor, &SlotText), Without<Hud>>,
) {
    let p = player_q.single().map(|t| t.translation).unwrap_or(SPAWN);
    let now = now_unix();
    for (mut text, mut color, hud) in q.iter_mut() {
        match hud {
            Hud::Clock => {
                let phase = if game.night {
                    format!("NIGHT - morning in {}", fmt_dur(phase_left(now)))
                } else {
                    format!("DAY - night in {}", fmt_dur(phase_left(now)))
                };
                let n = hatching_q.iter().count();
                let hatch = if n == 0 {
                    "Hatching: none".to_string()
                } else {
                    let next = hatching_q
                        .iter()
                        .map(|h| h.hatch_at - now)
                        .fold(f64::MAX, f64::min);
                    format!("Hatching: {} (next in {})", n, fmt_dur(next))
                };
                text.0 = format!("{}   |   {}", phase, hatch);
                color.0 = if game.night {
                    Color::srgb(1.0, 0.55, 0.55)
                } else {
                    Color::srgb(0.9, 0.9, 0.75)
                };
            }
            Hud::Gear => {
                text.0 = if game.gear == 0 {
                    "Gear: none - buy some at the Gear Station beside your yard".to_string()
                } else {
                    let parts: Vec<String> = (0..game.gear)
                        .map(|i| {
                            if game.gear_cd[i] > 0.0 {
                                format!("[{}] {} {:.0}s", GEAR_KEY_NAMES[i], GEAR_NAMES[i], game.gear_cd[i])
                            } else {
                                format!("[{}] {} READY", GEAR_KEY_NAMES[i], GEAR_NAMES[i])
                            }
                        })
                        .collect();
                    format!("Gear: {}", parts.join("   "))
                };
            }
            Hud::Night => {
                if game.night && p.x > LINE_X - 30.0 && p.x < LINE_X + 2.0 {
                    text.0 = "IT IS NIGHT TIME. You cannot go out there.".to_string();
                    color.0 = Color::srgb(1.0, 0.2, 0.2);
                } else {
                    text.0.clear();
                }
            }
            Hud::Money => {
                text.0 = format!("$ {}   WORLD {}", fmt_money(game.money), game.level);
            }
            Hud::Speed => {
                let kmh = player_speed(&game, p) * 3.6;
                let zone = if p.x < LINE_X {
                    format!(" (egg zone: {:.1})", trained_speed(&game) * 3.6)
                } else {
                    String::new()
                };
                text.0 = format!(
                    "Speed: {:.1} km/h{}   Training: {:.0}   Treadmill: {}",
                    kmh, zone, game.training, T_NAMES[game.tier]
                );
            }
            Hud::Stats => {
                let types: std::collections::HashSet<usize> =
                    game.yard_eggs.iter().copied().collect();
                let portal = if game.won {
                    "EGG MASTER!".to_string()
                } else {
                    match next_locked(&game.unlocked) {
                        Some(w) => format!("World {}: ${}", w, fmt_money(LEVEL_COST[w - 2])),
                        None => format!("Win: ${} in World {}", fmt_money(WIN_MONEY), N_WORLDS),
                    }
                };
                text.0 = format!(
                    "Hatched: {}   Types: {}/169   {}",
                    game.yard_eggs.len(),
                    types.len(),
                    portal
                );
            }
            Hud::Carry => {
                if !game.has_eggs() {
                    text.0.clear();
                } else {
                    let total: f64 = game.carried().map(egg_value).sum();
                    let best = game.carried().max_by_key(|&k| egg_value(k) as u64).unwrap();
                    let (_, rc) = rarity(best);
                    text.0 = format!(
                        "Carrying {}/{} eggs (worth ${}) - take them to YOUR YARD to hatch!",
                        game.carrying(),
                        INV_SLOTS,
                        total as u64
                    );
                    color.0 = rc;
                }
            }
            Hud::Msg => {
                if game.paused {
                    text.0.clear();
                } else if game.msg_t > 0.0 {
                    text.0 = game.msg.clone();
                    color.0 = game.msg_color.with_alpha(game.msg_t.min(1.0));
                } else if game.won {
                    text.0 = "EGG MASTER! All 10 worlds complete!".to_string();
                    color.0 = Color::srgb(1.0, 0.85, 0.2);
                } else {
                    text.0.clear();
                }
            }
            Hud::Prompt => {
                let next_cost = next_locked(&game.unlocked).map(|w| LEVEL_COST[w - 2]);
                text.0 = if game.menu_open {
                    "Click a world to travel! (Esc or walk away to close)".to_string()
                } else if near_portal(p) {
                    "[E] open the portal and pick a world".to_string()
                } else if near_gear(p) {
                    if game.gear < 3 {
                        format!(
                            "GEAR STATION / UPGRADER - [E] buy {} (${})",
                            GEAR_NAMES[game.gear],
                            GEAR_COSTS[game.gear] as u64
                        )
                    } else {
                        "GEAR STATION / UPGRADER - you own every piece of gear!".to_string()
                    }
                } else if next_cost.is_some_and(|c| game.money >= c) {
                    "THE PORTALS ARE GLOWING! Press [E] at the one at the back of any yard!".to_string()
                } else if game.tier < 6 {
                    format!(
                        "WASD move | mouse look | 1-0 select slot | [E] near treadmill: upgrade to {} (${})",
                        T_NAMES[game.tier + 1],
                        T_COSTS[game.tier + 1] as u64
                    )
                } else {
                    "WASD move | mouse look | 1-0 select slot | HACKER treadmill maxed - run forever!".to_string()
                };
            }
        }
    }
    for (mut bg, mut border, slot) in slots.iter_mut() {
        if let Some(k) = game.inventory[slot.0] {
            let (r, g, b) = EGG_RGB[k / 13];
            bg.0 = Color::srgb(r, g, b);
        } else {
            bg.0 = Color::srgba(0.0, 0.0, 0.0, 0.45);
        }
        border.0 = if slot.0 == game.selected {
            Color::srgb(1.0, 0.9, 0.3)
        } else {
            Color::srgba(1.0, 1.0, 1.0, 0.55)
        };
    }
    for (mut text, mut tc, st) in slot_texts.iter_mut() {
        if let Some(k) = game.inventory[st.0] {
            let (r, g, b) = EGG_RGB[k / 13];
            text.0 = format!("${}", egg_value(k) as u64);
            tc.0 = if r * 0.3 + g * 0.6 + b * 0.1 > 0.5 {
                Color::BLACK
            } else {
                Color::WHITE
            };
        } else {
            // empty slot: show its key
            text.0 = ((st.0 + 1) % 10).to_string();
            tc.0 = Color::srgba(1.0, 1.0, 1.0, 0.35);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walk(from: Vec3, to: Vec3) -> Vec3 {
        let mut p = from;
        move_blocked(&mut p, to - from, &blockers());
        p
    }

    #[test]
    fn fence_blocks_walking_through_rails() {
        // approach the east fence of yard 0 away from the gate
        let end = walk(Vec3::new(45.0, EYE, 15.0), Vec3::new(41.0, EYE, 15.0));
        assert!(end.x >= YARD.1 + FENCE_HALF + PLAYER_RADIUS - 1e-3, "went through fence: {end}");
        // and from the inside going out
        let end = walk(Vec3::new(41.0, EYE, 30.0), Vec3::new(45.0, EYE, 30.0));
        assert!(end.x <= YARD.1 - FENCE_HALF - PLAYER_RADIUS + 1e-3, "went through fence: {end}");
        // north fence too
        let end = walk(Vec3::new(30.0, EYE, 9.0), Vec3::new(30.0, EYE, 15.0));
        assert!(end.z <= YARD.2 - FENCE_HALF - PLAYER_RADIUS + 1e-3, "went through fence: {end}");
    }

    #[test]
    fn gate_lets_you_in() {
        let end = walk(Vec3::new(44.5, EYE, 24.5), Vec3::new(41.0, EYE, 24.5));
        assert!((end.x - 41.0).abs() < 1e-3, "gate blocked: {end}");
    }

    #[test]
    fn treadmill_open_from_east_but_solid_on_sides_and_console() {
        // step on from the east end and reach the middle of the belt
        let end = walk(Vec3::new(51.0, EYE, TM_CENTER.z), TM_CENTER.with_y(EYE));
        assert!((end - TM_CENTER.with_y(EYE)).length() < 1e-3, "could not step on: {end}");
        assert!(on_deck(end));
        // cannot walk out through the console at the west end
        let end = walk(TM_CENTER.with_y(EYE), Vec3::new(45.0, EYE, TM_CENTER.z));
        assert!(end.x > TM_CENTER.x - TM_BELT_L / 2.0 - 0.05, "went through console: {end}");
        // cannot walk in through the side frame
        let end = walk(Vec3::new(TM_CENTER.x, EYE, 22.0), TM_CENTER.with_y(EYE));
        assert!(end.z < TM_CENTER.z - TM_BELT_W / 2.0, "went through side frame: {end}");
    }

    #[test]
    fn fast_movement_cannot_tunnel() {
        // one huge step straight through the fence still gets pushed out
        let b = blockers();
        let mut p = Vec3::new(43.2, EYE, 15.0);
        p.x = 42.9; // now inside the rail box
        resolve_blockers(&mut p, PLAYER_RADIUS, &b);
        assert!((p.x - YARD.1).abs() >= FENCE_HALF + PLAYER_RADIUS - 1e-3, "{p}");
    }
}
