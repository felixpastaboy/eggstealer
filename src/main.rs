use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::input::mouse::MouseMotion;
use bevy::math::Affine2;
use bevy::pbr::{CascadeShadowConfigBuilder, DistanceFog, FogFalloff};
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::{CursorGrabMode, PrimaryWindow};
use std::f32::consts::{PI, TAU};

// World layout (meters). Safe zone is x < LINE_X, egg zone is x > LINE_X.
const WORLD_X: f32 = 260.0;
const WORLD_Z: f32 = 156.0;
const LINE_X: f32 = 105.0;
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

// Your yard: x 13..43, z 12..37. Treadmill just east of it.
const YARD: (f32, f32, f32, f32) = (13.0, 43.0, 12.0, 37.0);
const TM_CENTER: Vec3 = Vec3::new(46.6, 0.0, 24.5);
const PORTAL_POS: Vec3 = Vec3::new(15.0, 0.0, 24.5);
const LEVEL_COST: [f64; 3] = [1_000_000.0, 2_000_000.0, 3_000_000.0];

fn level_mult(level: usize) -> f64 {
    [1.0, 5.0, 25.0][level - 1]
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
        112.0 + fastrand::f32() * (WORLD_X - 6.0 - 112.0),
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
struct CarriedEgg {
    slot: usize,
}

#[derive(Component)]
struct Hatching {
    kind: usize,
    t: f32,
}

#[derive(Component)]
struct Pet {
    phase: f32,
    age: f32,
}

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
struct WorldButton(usize);

#[derive(Component)]
struct WorldButtonText(usize);

// axis-aligned fence walls you cannot walk through (east sides have a gate gap)
struct FenceSeg {
    vertical: bool,
    line: f32,
    a: f32,
    b: f32,
}

fn fence_segments() -> Vec<FenceSeg> {
    let mut v = Vec::new();
    for yi in 0..4 {
        let (x0, x1) = (13.0, 43.0);
        let z0 = 12.0 + yi as f32 * 35.0;
        let z1 = z0 + 25.0;
        let gate = (z0 + z1) / 2.0;
        v.push(FenceSeg { vertical: false, line: z0, a: x0, b: x1 });
        v.push(FenceSeg { vertical: false, line: z1, a: x0, b: x1 });
        v.push(FenceSeg { vertical: true, line: x0, a: z0, b: z1 });
        v.push(FenceSeg { vertical: true, line: x1, a: z0, b: gate - 2.0 });
        v.push(FenceSeg { vertical: true, line: x1, a: gate + 2.0, b: z1 });
    }
    v
}

#[derive(Component)]
struct Monster {
    home: Vec3,
    speed: f32,
    scale: f32,
    awake: f32,
    phase: f32,
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
}

#[derive(Resource)]
struct Game {
    money: f64,
    training: f32,
    tier: usize,
    inventory: Vec<usize>,
    level: usize,
    unlocked: [bool; 3],
    menu_open: bool,
    won: bool,
    yard_slots: usize,
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
    beak_mesh: Handle<Mesh>,
    beak_mat: Handle<StandardMaterial>,
    black_mat: Handle<StandardMaterial>,
    defeat_sound: Handle<AudioSource>,
    pickup_sound: Handle<AudioSource>,
    on_treadmill: bool,
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Egg Stealer 3D".to_string(),
                resolution: (1280.0, 800.0).into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb(0.54, 0.74, 0.94)))
        .insert_resource(AmbientLight {
            color: Color::srgb(0.75, 0.82, 1.0),
            brightness: 480.0,
            ..default()
        })
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                cursor_grab,
                player_look,
                player_move,
                gameplay,
                monsters_ai,
                hatch_and_pets,
                portal_system,
                portal_menu,
                world_buttons,
                animate,
                hud,
            ),
        )
        .run();
}

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
    if repeat {
        img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            ..ImageSamplerDescriptor::default()
        });
    }
    images.add(img)
}

fn grass_pixel(x: u32, y: u32) -> [u8; 4] {
    let n = (hash2(x, y, 7) % 31) as i32 - 15;
    let patch = (hash2(x / 16, y / 16, 21) % 17) as i32 - 8;
    let blade = hash2(x, y, 99) % 97 < 5;
    let (mut r, mut g, mut b) = (58 + n + patch, 96 + n + patch, 40 + n / 2 + patch);
    if blade {
        r += 22;
        g += 34;
        b += 12;
    }
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
    let grass_tex = make_image(&mut images, 128, 128, true, grass_pixel);
    let stone_tex = make_image(&mut images, 128, 128, true, stone_pixel);
    let belt_tex = make_image(&mut images, 64, 64, true, belt_pixel);

    let safe_grass = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 1.0, 1.0),
        base_color_texture: Some(grass_tex.clone()),
        perceptual_roughness: 0.95,
        uv_transform: Affine2::from_scale(Vec2::new(30.0, 44.0)),
        ..default()
    });
    let zone_grass = materials.add(StandardMaterial {
        base_color: Color::srgb(0.78, 0.82, 0.78),
        base_color_texture: Some(grass_tex.clone()),
        perceptual_roughness: 0.95,
        uv_transform: Affine2::from_scale(Vec2::new(44.0, 44.0)),
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
        base_color: Color::srgb(0.48, 0.33, 0.18),
        perceptual_roughness: 0.9,
        ..default()
    });
    let white_line = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 1.0, 1.0),
        emissive: LinearRgba::rgb(0.6, 0.6, 0.6),
        perceptual_roughness: 0.8,
        ..default()
    });
    let bush_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.16, 0.34, 0.14),
        perceptual_roughness: 1.0,
        ..default()
    });
    let rock_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.45, 0.44, 0.42),
        perceptual_roughness: 0.95,
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
        let z0 = 12.0 + yi as f32 * 35.0;
        let z1 = z0 + 25.0;
        let post = meshes.add(Cuboid::new(0.14, 1.25, 0.14));
        let mut spawn_post = |x: f32, z: f32, commands: &mut Commands| {
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

    // --- treadmills (yours is functional, other yards get dirt ones) ---
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
        let c = Vec3::new(46.6, 0.0, 24.5 + yi as f32 * 35.0);
        // belt
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(2.6, 0.14, 1.3))),
            MeshMaterial3d(belt_mat.clone()),
            Transform::from_xyz(c.x, 0.2, c.z),
        ));
        // side frames
        for dz in [-0.72, 0.72] {
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(2.8, 0.3, 0.12))),
                MeshMaterial3d(frame_mat.clone()),
                Transform::from_xyz(c.x, 0.16, c.z + dz),
            ));
        }
        // console post + panel (west end, facing the yard)
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(0.12, 1.15, 1.3))),
            MeshMaterial3d(frame_mat.clone()),
            Transform::from_xyz(c.x - 1.35, 0.6, c.z),
        ));
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(0.08, 0.5, 0.95))),
            MeshMaterial3d(if yi == 0 {
                panel_mat.clone()
            } else {
                materials.add(StandardMaterial {
                    base_color: Color::srgb(0.45, 0.32, 0.18),
                    ..default()
                })
            }),
            Transform::from_xyz(c.x - 1.42, 1.05, c.z),
        ));
    }

    // --- bushes & rocks ---
    for _ in 0..26 {
        let x = 5.0 + fastrand::f32() * (WORLD_X - 10.0);
        let z = 5.0 + fastrand::f32() * (WORLD_Z - 10.0);
        // keep clear of yards/treadmills/spawn/line
        if x > 8.0 && x < 52.0 && z > 8.0 && z < 146.0 {
            continue;
        }
        if (x - LINE_X).abs() < 4.0 {
            continue;
        }
        let s = 0.8 + fastrand::f32() * 0.9;
        commands.spawn((
            Mesh3d(unit_sphere.clone()),
            MeshMaterial3d(bush_mat.clone()),
            Transform::from_xyz(x, s * 0.45, z).with_scale(Vec3::new(s * 1.3, s * 0.7, s * 1.3)),
        ));
        commands.spawn((
            Mesh3d(unit_sphere.clone()),
            MeshMaterial3d(bush_mat.clone()),
            Transform::from_xyz(x + s * 0.7, s * 0.35, z + s * 0.3)
                .with_scale(Vec3::new(s * 0.8, s * 0.5, s * 0.8)),
        ));
    }
    for _ in 0..10 {
        let x = 8.0 + fastrand::f32() * (WORLD_X - 16.0);
        let z = 8.0 + fastrand::f32() * (WORLD_Z - 16.0);
        if (x - LINE_X).abs() < 4.0 {
            continue;
        }
        let s = 0.35 + fastrand::f32() * 0.6;
        commands.spawn((
            Mesh3d(unit_sphere.clone()),
            MeshMaterial3d(rock_mat.clone()),
            Transform::from_xyz(x, s * 0.4, z)
                .with_scale(Vec3::new(s * 1.4, s * 0.6, s))
                .with_rotation(Quat::from_rotation_y(fastrand::f32() * TAU)),
        ));
    }

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
        (Vec3::new(145.0, 0.0, 35.0), 1.0, 6.5, (0.45, 0.13, 0.13)),
        (Vec3::new(195.0, 0.0, 28.0), 1.2, 6.2, (0.34, 0.13, 0.42)),
        (Vec3::new(232.0, 0.0, 80.0), 1.35, 5.8, (0.13, 0.30, 0.15)),
        (Vec3::new(155.0, 0.0, 115.0), 1.05, 6.8, (0.13, 0.17, 0.34)),
        (Vec3::new(205.0, 0.0, 128.0), 1.1, 6.0, (0.16, 0.16, 0.18)),
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

    // --- portal at the back of your yard ---
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
    commands.spawn((
        Mesh3d(meshes.add(Torus {
            minor_radius: 0.25,
            major_radius: 2.0,
        })),
        MeshMaterial3d(ring_mat),
        Transform::from_xyz(PORTAL_POS.x, 2.3, PORTAL_POS.z)
            .with_rotation(Quat::from_rotation_z(PI / 2.0)),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Circle::new(1.85))),
        MeshMaterial3d(portal_mat.clone()),
        Transform::from_xyz(PORTAL_POS.x, 2.3, PORTAL_POS.z)
            .with_rotation(Quat::from_rotation_y(PI / 2.0)),
        PortalDisc,
    ));

    // --- pet part assets ---
    let beak_mesh = meshes.add(Cone {
        radius: 0.07,
        height: 0.16,
    });
    let beak_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.6, 0.15),
        perceptual_roughness: 0.6,
        ..default()
    });
    let black_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.05, 0.05, 0.05),
        perceptual_roughness: 0.4,
        ..default()
    });

    // --- game resource + initial eggs ---
    let game = Game {
        money: 0.0,
        training: 0.0,
        tier: 0,
        inventory: Vec::new(),
        level: 1,
        unlocked: [true, false, false],
        menu_open: false,
        won: false,
        yard_slots: 0,
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
        beak_mesh,
        beak_mat,
        black_mat,
        defeat_sound: asset_server.load("defeat.wav"),
        pickup_sound: asset_server.load("pickup.wav"),
        on_treadmill: false,
    };
    for _ in 0..60 {
        let (x, z) = random_egg_xz();
        spawn_world_egg(&mut commands, &game, x, z, random_kind());
    }
    commands.insert_resource(game);

    // --- sun, camera, HUD ---
    commands.spawn((
        DirectionalLight {
            illuminance: 9500.0,
            shadows_enabled: true,
            color: Color::srgb(1.0, 0.96, 0.88),
            ..default()
        },
        Transform::from_xyz(60.0, 90.0, 20.0).looking_at(Vec3::new(130.0, 0.0, 78.0), Vec3::Y),
        CascadeShadowConfigBuilder {
            first_cascade_far_bound: 25.0,
            maximum_distance: 160.0,
            ..default()
        }
        .build(),
    ));

    commands.spawn((
        Camera3d::default(),
        Projection::from(PerspectiveProjection {
            fov: 75.0_f32.to_radians(),
            ..default()
        }),
        Transform::from_translation(SPAWN)
            .with_rotation(Quat::from_rotation_y(-PI / 2.0)),
        DistanceFog {
            color: Color::srgb(0.6, 0.75, 0.92),
            falloff: FogFalloff::Linear {
                start: 70.0,
                end: 260.0,
            },
            ..default()
        },
        Player {
            yaw: -PI / 2.0,
            pitch: 0.0,
        },
    ));

    // HUD
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
    for (top, size, hud) in [
        (Val::Px(78.0), 20.0, Hud::Carry),
        (Val::Percent(30.0), 26.0, Hud::Msg),
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
    // inventory hotbar: 5 free slots
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            bottom: Val::Px(58.0),
            justify_content: JustifyContent::Center,
            column_gap: Val::Px(10.0),
            ..default()
        })
        .with_children(|row| {
            for i in 0..5 {
                row.spawn((
                    Node {
                        width: Val::Px(56.0),
                        height: Val::Px(56.0),
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
    // portal world-select bar (hidden until you step into the portal)
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
                        column_gap: Val::Px(14.0),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.05, 0.75)),
                    BorderRadius::all(Val::Px(16.0)),
                ))
                .with_children(|bar| {
                    for w in 1..=3usize {
                        bar.spawn((
                            Button,
                            Node {
                                width: Val::Px(210.0),
                                height: Val::Px(64.0),
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
                                    font_size: 19.0,
                                    ..default()
                                },
                                TextColor(Color::WHITE),
                                WorldButtonText(w),
                            ));
                        });
                    }
                });
        });
}

// ---------- systems ----------

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

fn player_speed(game: &Game) -> f32 {
    let mut s = 4.2 + game.training * 0.06;
    if !game.inventory.is_empty() {
        s *= 0.92;
    }
    s
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
    let old = tf.translation;
    if moving {
        tf.translation += dir.normalize() * player_speed(&game) * dt;
    }
    tf.translation.x = tf.translation.x.clamp(4.0, WORLD_X - 4.0);
    tf.translation.z = tf.translation.z.clamp(4.0, WORLD_Z - 4.0);
    // solid fences: you can only get into a yard through its gate
    let r = 0.4;
    for s in fence_segments() {
        if s.vertical {
            let crossed = (old.x - s.line).signum() != (tf.translation.x - s.line).signum()
                || (tf.translation.x - s.line).abs() < r;
            if crossed && tf.translation.z > s.a - r && tf.translation.z < s.b + r {
                let side = (old.x - s.line).signum();
                if side != 0.0 {
                    tf.translation.x = s.line + side * r;
                }
            }
        } else {
            let crossed = (old.z - s.line).signum() != (tf.translation.z - s.line).signum()
                || (tf.translation.z - s.line).abs() < r;
            if crossed && tf.translation.x > s.a - r && tf.translation.x < s.b + r {
                let side = (old.z - s.line).signum();
                if side != 0.0 {
                    tf.translation.z = s.line + side * r;
                }
            }
        }
    }
    // head bob
    let t = time.elapsed_secs();
    tf.translation.y = EYE
        + if moving {
            (t * (6.0 + player_speed(&game) * 0.8)).sin() * 0.05
        } else {
            0.0
        };

    // treadmill: stand on it to train - no cap, you can always get faster
    let p = tf.translation;
    game.on_treadmill =
        (p.x - TM_CENTER.x).abs() < 1.5 && (p.z - TM_CENTER.z).abs() < 0.95;
    if game.on_treadmill {
        game.training += dt * T_MULT[game.tier] * 0.5;
    }
}

fn gameplay(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut game: ResMut<Game>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    player_q: Query<(Entity, &Transform), With<Player>>,
    eggs_q: Query<(Entity, &WorldEgg, &Transform)>,
    carried_q: Query<Entity, With<CarriedEgg>>,
) {
    let Ok((player_ent, ptf)) = player_q.single() else {
        return;
    };
    let dt = time.delta_secs();
    let p = ptf.translation;

    // pick up eggs into the 5 inventory slots
    if game.inventory.len() < 5 {
        for (ent, egg, etf) in eggs_q.iter() {
            if game.inventory.len() >= 5 {
                break;
            }
            let d = (etf.translation - p).xz().length();
            if d < 1.5 {
                commands.entity(ent).despawn();
                commands.spawn((
                    AudioPlayer::new(game.pickup_sound.clone()),
                    PlaybackSettings::DESPAWN,
                ));
                let slot = game.inventory.len();
                game.inventory.push(egg.kind);
                let (rn, rc) = rarity(egg.kind);
                game.msg = format!(
                    "Stole a {} {}! ({}/5)",
                    rn,
                    egg_name(egg.kind),
                    game.inventory.len()
                );
                game.msg_color = rc;
                game.msg_t = 2.5;
                let mesh = game.egg_mesh.clone();
                let mat = game.egg_mats[egg.kind].clone();
                commands.entity(player_ent).with_children(|c| {
                    c.spawn((
                        Mesh3d(mesh),
                        MeshMaterial3d(mat),
                        Transform::from_xyz(-0.44 + slot as f32 * 0.22, -0.34, -0.8)
                            .with_scale(Vec3::splat(0.32)),
                        CarriedEgg { slot },
                    ));
                });
            }
        }
    } else if game.msg_t <= 0.0 {
        for (_, _, etf) in eggs_q.iter() {
            if (etf.translation - p).xz().length() < 1.5 {
                game.msg = "All 5 slots full - run home and hatch them!".to_string();
                game.msg_color = Color::srgb(1.0, 0.9, 0.4);
                game.msg_t = 2.0;
                break;
            }
        }
    }

    // deposit in your yard -> eggs start hatching into pets
    if !game.inventory.is_empty() && p.x > YARD.0 && p.x < YARD.1 && p.z > YARD.2 && p.z < YARD.3
    {
        for e in carried_q.iter() {
            commands.entity(e).despawn();
        }
        let mult = level_mult(game.level);
        let mut bonus = 0.0;
        let n = game.inventory.len();
        let inv: Vec<usize> = game.inventory.drain(..).collect();
        for k in inv {
            bonus += egg_value(k) * 10.0 * mult;
            game.yard_eggs.push(k);
            let slot = game.yard_slots;
            if slot < 48 {
                game.yard_slots += 1;
                let (col, row) = (slot % 8, slot / 8);
                commands.spawn((
                    Mesh3d(game.egg_mesh.clone()),
                    MeshMaterial3d(game.egg_mats[k].clone()),
                    Transform::from_xyz(15.5 + col as f32 * 3.5, 0.36, 14.5 + row as f32 * 3.6)
                        .with_scale(Vec3::splat(0.55))
                        .with_rotation(Quat::from_rotation_y(fastrand::f32() * TAU)),
                    Hatching {
                        kind: k,
                        t: 2.5 + fastrand::f32() * 3.0,
                    },
                ));
            }
        }
        game.money += bonus;
        game.msg = format!(
            "+${} - {} egg{} hatching in your yard!",
            bonus as u64,
            n,
            if n == 1 { " is" } else { "s are" }
        );
        game.msg_color = Color::srgb(0.4, 1.0, 0.5);
        game.msg_t = 3.0;
    }

    // upgrade treadmill
    if keys.just_pressed(KeyCode::KeyE) && game.tier < 6 {
        if (p - Vec3::new(TM_CENTER.x, p.y, TM_CENTER.z)).length() < 4.5 {
            let cost = T_COSTS[game.tier + 1];
            if game.money >= cost {
                game.money -= cost;
                game.tier += 1;
                let (r, g, b) = T_RGB[game.tier];
                if let Some(m) = materials.get_mut(&game.panel_mat) {
                    m.base_color = Color::srgb(r, g, b);
                    m.emissive = LinearRgba::rgb(r * 2.5, g * 2.5, b * 2.5);
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

    // your pets make money for you
    let income: f64 = game.yard_eggs.iter().map(|&k| egg_value(k) * 1.5).sum::<f64>()
        * level_mult(game.level);
    game.money += income * dt as f64;

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
    mut mq: Query<(&mut Monster, &mut Transform, &Children), Without<Player>>,
    mut eyes: Query<&mut MeshMaterial3d<StandardMaterial>, With<MonsterEye>>,
    mut limbs: Query<(&mut Transform, &MonsterLimb), (Without<Monster>, Without<Player>)>,
    carried_q: Query<Entity, With<CarriedEgg>>,
) {
    let Ok(mut ptf) = player_q.single_mut() else {
        return;
    };
    let dt = time.delta_secs();
    let t = time.elapsed_secs();
    let p = ptf.translation;
    let hunting = !game.inventory.is_empty() && p.x > LINE_X;
    let mut caught = false;

    for (mut m, mut tf, children) in mq.iter_mut() {
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
            m.awake = (m.awake - dt * 1.2).max(0.0);
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

    if caught && !game.inventory.is_empty() {
        for e in carried_q.iter() {
            commands.entity(e).despawn();
        }
        let inv: Vec<usize> = game.inventory.drain(..).collect();
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
        game.msg = format!(
            "CAUGHT! The monster took your {} egg{} back!",
            n,
            if n == 1 { "" } else { "s" }
        );
        game.msg_color = Color::srgb(1.0, 0.25, 0.25);
        game.msg_t = 3.5;
    }
}

fn animate(
    time: Res<Time>,
    game: Res<Game>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut carried: Query<(&mut Transform, &CarriedEgg), Without<PortalDisc>>,
    mut portal: Query<&mut Transform, (With<PortalDisc>, Without<CarriedEgg>)>,
) {
    let t = time.elapsed_secs();
    let dt = time.delta_secs();
    // belt scroll
    let belt_speed = if game.on_treadmill {
        0.6 + player_speed(&game) * 0.25
    } else {
        0.35
    };
    if let Some(m) = materials.get_mut(&game.belt_mat) {
        m.uv_transform.translation.x = (m.uv_transform.translation.x + dt * belt_speed) % 1.0;
    }
    // carried eggs sway in their slots
    for (mut tf, ce) in carried.iter_mut() {
        let s = ce.slot as f32;
        tf.translation = Vec3::new(-0.44 + s * 0.22, -0.34 + (t * 4.0 + s).sin() * 0.012, -0.8);
        tf.rotation = Quat::from_rotation_y(t * 0.8 + s);
    }
    // portal vortex spin
    for mut tf in portal.iter_mut() {
        tf.rotation = Quat::from_rotation_x(t * 1.3) * Quat::from_rotation_y(PI / 2.0);
    }
}

fn hatch_and_pets(
    time: Res<Time>,
    mut commands: Commands,
    game: Res<Game>,
    mut hatching: Query<
        (Entity, &mut Hatching, &mut Transform),
        (Without<Pet>, Without<ShellBit>),
    >,
    mut pets: Query<(&mut Pet, &mut Transform), (Without<Hatching>, Without<ShellBit>)>,
    mut bits: Query<(Entity, &mut ShellBit), (Without<Hatching>, Without<Pet>)>,
) {
    let dt = time.delta_secs();
    let t = time.elapsed_secs();
    for (ent, mut h, mut tf) in hatching.iter_mut() {
        h.t -= dt;
        let urgency = ((3.0 - h.t.max(0.0)) / 3.0).clamp(0.0, 1.0);
        tf.rotation =
            Quat::from_rotation_z((t * 18.0).sin() * 0.14 * urgency) * Quat::from_rotation_y(t * 0.3);
        if h.t <= 0.0 {
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
            // the pet hatches!
            commands
                .spawn((
                    Transform::from_xyz(pos.x, 0.0, pos.z).with_scale(Vec3::splat(0.01)),
                    Visibility::default(),
                    Pet {
                        phase: fastrand::f32() * TAU,
                        age: 0.0,
                    },
                ))
                .with_children(|p| {
                    // body wears the same shell pattern as its egg
                    p.spawn((
                        Mesh3d(game.sphere_mesh.clone()),
                        MeshMaterial3d(game.egg_mats[kind].clone()),
                        Transform::from_xyz(0.0, 0.30, 0.0)
                            .with_scale(Vec3::new(0.30, 0.26, 0.30)),
                    ));
                    p.spawn((
                        Mesh3d(game.sphere_mesh.clone()),
                        MeshMaterial3d(game.egg_mats[kind].clone()),
                        Transform::from_xyz(0.0, 0.62, -0.06).with_scale(Vec3::splat(0.17)),
                    ));
                    for dx in [-0.07f32, 0.07] {
                        p.spawn((
                            Mesh3d(game.sphere_mesh.clone()),
                            MeshMaterial3d(game.black_mat.clone()),
                            Transform::from_xyz(dx, 0.66, -0.20).with_scale(Vec3::splat(0.035)),
                        ));
                    }
                    p.spawn((
                        Mesh3d(game.beak_mesh.clone()),
                        MeshMaterial3d(game.beak_mat.clone()),
                        Transform::from_xyz(0.0, 0.60, -0.24)
                            .with_rotation(Quat::from_rotation_x(-PI / 2.0)),
                    ));
                });
        }
    }
    for (mut pet, mut tf) in pets.iter_mut() {
        pet.age += dt;
        let grow = (pet.age / 0.5).min(1.0);
        tf.scale = Vec3::splat(grow.max(0.01));
        tf.translation.y = (t * 5.0 + pet.phase).sin().abs() * 0.14 * grow;
        tf.rotation = Quat::from_rotation_y((t * 0.6 + pet.phase).sin() * 1.2);
    }
    for (ent, mut b) in bits.iter_mut() {
        b.t -= dt;
        if b.t <= 0.0 {
            commands.entity(ent).despawn();
        }
    }
}

fn apply_level_theme(
    level: usize,
    game: &Game,
    materials: &mut Assets<StandardMaterial>,
    clear: &mut ClearColor,
    fog_q: &mut Query<&mut DistanceFog>,
    sun_q: &mut Query<&mut DirectionalLight>,
) {
    let (safe, zone, sky, fog, lux, sun) = match level {
        1 => (
            (1.0, 1.0, 1.0),
            (0.78, 0.82, 0.78),
            (0.54, 0.74, 0.94),
            (0.6, 0.75, 0.92),
            9500.0,
            (1.0, 0.96, 0.88),
        ),
        2 => (
            (1.0, 0.86, 0.58),
            (0.95, 0.76, 0.48),
            (0.93, 0.66, 0.40),
            (0.92, 0.70, 0.50),
            7500.0,
            (1.0, 0.82, 0.60),
        ),
        _ => (
            (0.62, 0.52, 1.0),
            (0.50, 0.40, 0.92),
            (0.08, 0.05, 0.20),
            (0.16, 0.11, 0.30),
            3200.0,
            (0.72, 0.72, 1.0),
        ),
    };
    if let Some(m) = materials.get_mut(&game.safe_grass_mat) {
        m.base_color = Color::srgb(safe.0, safe.1, safe.2);
    }
    if let Some(m) = materials.get_mut(&game.zone_grass_mat) {
        m.base_color = Color::srgb(zone.0, zone.1, zone.2);
    }
    clear.0 = Color::srgb(sky.0, sky.1, sky.2);
    for mut f in fog_q.iter_mut() {
        f.color = Color::srgb(fog.0, fog.1, fog.2);
    }
    for mut s in sun_q.iter_mut() {
        s.illuminance = lux;
        s.color = Color::srgb(sun.0, sun.1, sun.2);
    }
}

fn portal_system(
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
    } else if !game.unlocked[1] {
        game.money >= LEVEL_COST[0]
    } else if !game.unlocked[2] {
        game.money >= LEVEL_COST[1]
    } else {
        true
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
    // beat World 3 by reaching $3000000 there
    if game.level == 3 && !game.won && game.money >= LEVEL_COST[2] {
        game.won = true;
        if let Some(m) = materials.get_mut(&game.portal_mat) {
            m.emissive = LinearRgba::rgb(3.0, 2.4, 0.6);
        }
        game.msg = "$3000000 IN WORLD 3 - YOU BEAT THE GAME! EGG MASTER!".to_string();
        game.msg_color = Color::srgb(1.0, 0.85, 0.2);
        game.msg_t = 12.0;
    }
    // stepping into the portal opens the world-select bar
    let p = ptf.translation;
    let d = Vec2::new(p.x - PORTAL_POS.x, p.z - PORTAL_POS.z).length();
    if d < 1.7 && !game.menu_open {
        game.menu_open = true;
    } else if d > 3.5 && game.menu_open {
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
    mut clear: ResMut<ClearColor>,
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
            format!("World {} - ${}", w, LEVEL_COST[w - 2] as u64)
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
                } else if w == 3 && !game.unlocked[1] {
                    game.msg = "Unlock World 2 first!".to_string();
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
                            level_mult(w) as u64
                        );
                        game.msg_color = Color::srgb(0.5, 1.0, 1.0);
                        game.msg_t = 6.0;
                    } else {
                        game.msg = format!(
                            "World {} needs ${} (you have ${})",
                            w, cost as u64, game.money as u64
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
        apply_level_theme(w, &game, &mut materials, &mut clear, &mut fog_q, &mut sun_q);
        if let Ok(mut ptf) = player_q.single_mut() {
            ptf.translation = SPAWN;
        }
        game.menu_open = false;
    }
}

fn hud(
    game: Res<Game>,
    mut q: Query<(&mut Text, &mut TextColor, &Hud), Without<SlotText>>,
    mut slots: Query<(&mut BackgroundColor, &SlotUi)>,
    mut slot_texts: Query<(&mut Text, &mut TextColor, &SlotText), Without<Hud>>,
) {
    let income: f64 = game.yard_eggs.iter().map(|&k| egg_value(k) * 1.5).sum::<f64>()
        * level_mult(game.level);
    for (mut text, mut color, hud) in q.iter_mut() {
        match hud {
            Hud::Money => {
                text.0 = format!(
                    "$ {}   (+${}/s)   WORLD {}",
                    game.money.floor() as i64,
                    income as u64,
                    game.level
                );
            }
            Hud::Speed => {
                let kmh = player_speed(&game) * 3.6;
                text.0 = format!(
                    "Speed: {:.1} km/h   Training: {:.0}   Treadmill: {}",
                    kmh, game.training, T_NAMES[game.tier]
                );
            }
            Hud::Stats => {
                let types: std::collections::HashSet<usize> =
                    game.yard_eggs.iter().copied().collect();
                let portal = if game.won {
                    "EGG MASTER!".to_string()
                } else if !game.unlocked[1] {
                    format!("World 2: ${}", LEVEL_COST[0] as u64)
                } else if !game.unlocked[2] {
                    format!("World 3: ${}", LEVEL_COST[1] as u64)
                } else {
                    format!("Win: ${} in World 3", LEVEL_COST[2] as u64)
                };
                text.0 = format!(
                    "Pets: {}   Types: {}/169   {}",
                    game.yard_eggs.len(),
                    types.len(),
                    portal
                );
            }
            Hud::Carry => {
                if game.inventory.is_empty() {
                    text.0.clear();
                } else {
                    let total: f64 = game.inventory.iter().map(|&k| egg_value(k)).sum();
                    let best = game
                        .inventory
                        .iter()
                        .copied()
                        .max_by_key(|&k| egg_value(k) as u64)
                        .unwrap();
                    let (_, rc) = rarity(best);
                    text.0 = format!(
                        "Carrying {}/5 eggs (worth ${}) - take them to YOUR YARD to hatch!",
                        game.inventory.len(),
                        total as u64
                    );
                    color.0 = rc;
                }
            }
            Hud::Msg => {
                if game.msg_t > 0.0 {
                    text.0 = game.msg.clone();
                    color.0 = game.msg_color.with_alpha(game.msg_t.min(1.0));
                } else if game.won {
                    text.0 = "EGG MASTER! All 3 levels complete!".to_string();
                    color.0 = Color::srgb(1.0, 0.85, 0.2);
                } else {
                    text.0.clear();
                }
            }
            Hud::Prompt => {
                let next_cost = if !game.unlocked[1] {
                    Some(LEVEL_COST[0])
                } else if !game.unlocked[2] {
                    Some(LEVEL_COST[1])
                } else {
                    None
                };
                text.0 = if game.menu_open {
                    "Click a world to travel! (Esc or walk away to close)".to_string()
                } else if next_cost.is_some_and(|c| game.money >= c) {
                    "THE PORTAL IS GLOWING! Walk into it at the back of YOUR YARD!".to_string()
                } else if game.tier < 6 {
                    format!(
                        "WASD move | mouse look | click to capture mouse | [E] near treadmill: upgrade to {} (${})",
                        T_NAMES[game.tier + 1],
                        T_COSTS[game.tier + 1] as u64
                    )
                } else {
                    "WASD move | mouse look | HACKER treadmill maxed - run forever!".to_string()
                };
            }
        }
    }
    for (mut bg, slot) in slots.iter_mut() {
        if let Some(&k) = game.inventory.get(slot.0) {
            let (r, g, b) = EGG_RGB[k / 13];
            bg.0 = Color::srgb(r, g, b);
        } else {
            bg.0 = Color::srgba(0.0, 0.0, 0.0, 0.45);
        }
    }
    for (mut text, mut tc, st) in slot_texts.iter_mut() {
        if let Some(&k) = game.inventory.get(st.0) {
            let (r, g, b) = EGG_RGB[k / 13];
            text.0 = format!("${}", egg_value(k) as u64);
            tc.0 = if r * 0.3 + g * 0.6 + b * 0.1 > 0.5 {
                Color::BLACK
            } else {
                Color::WHITE
            };
        } else {
            text.0.clear();
        }
    }
}
