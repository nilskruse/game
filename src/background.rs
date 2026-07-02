use bevy::math::DVec2;
use bevy::prelude::*;

use crate::camera::move_camera;

/// Deep-space backdrop: a dark clear color, a parallax starfield (depth layers
/// that scroll at different speeds and wrap around the camera so they're
/// effectively infinite), and a few distant planets.
pub struct BackgroundPlugin;

impl Plugin for BackgroundPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(Color::srgb(0.015, 0.015, 0.045)))
            .add_systems(Startup, spawn_background)
            // Reposition after the camera has moved so there's no one-frame lag.
            .add_systems(Update, apply_parallax.after(move_camera));
    }
}

/// A backdrop element that scrolls with parallax and wraps around the camera.
///
/// `strength` is how much it moves with the world: `1.0` = fixed in the world
/// (full apparent motion, i.e. closest), approaching `0.0` = sticks to the
/// camera (barely moves on screen, i.e. farthest). `base` is its fixed position
/// within a `tile`-sized cell that is tiled infinitely around the camera.
#[derive(Component)]
struct Parallax {
    base: Vec2,
    strength: f32,
    tile: f32,
}

/// Tiny deterministic PRNG (xorshift64*) so the starfield is the same every run
/// without pulling in a rand dependency.
struct Rng(u64);

impl Rng {
    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x.wrapping_mul(0x2545F4914F6CDD1D) >> 32) as u32
    }

    fn unit(&mut self) -> f32 {
        self.next_u32() as f32 / u32::MAX as f32
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }
}

const STAR_TILE: f32 = 2600.;
const STAR_Z: f32 = -100.;
const PLANET_Z: f32 = -90.;

fn spawn_background(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let mut rng = Rng(0x9E3779B97F4A7C15);

    // Distant planets: slight parallax (so they read as far) and a large tile so
    // they don't visibly repeat during normal play.
    let planets = [
        (Vec2::new(-1250., 820.), 260., Color::srgb(0.28, 0.34, 0.55)),
        (Vec2::new(1500., -650.), 180., Color::srgb(0.46, 0.30, 0.24)),
        (Vec2::new(950., 1150.), 120., Color::srgb(0.32, 0.45, 0.40)),
    ];
    for (pos, radius, color) in planets {
        commands.spawn((
            Mesh2d(meshes.add(Circle::new(radius))),
            MeshMaterial2d(materials.add(color)),
            Transform::from_xyz(pos.x, pos.y, PLANET_Z),
            Parallax {
                base: pos,
                strength: 0.3,
                tile: 9000.,
            },
        ));
    }

    // Shared star mesh + a small reusable palette (no material-per-star).
    let star_mesh = meshes.add(Circle::new(1.0));
    let palette: Vec<Handle<ColorMaterial>> = (0..10)
        .map(|i| {
            let b = 0.40 + 0.065 * i as f32;
            materials.add(Color::srgb(b, b, (b + 0.06).min(1.0)))
        })
        .collect();

    // Three depth layers: far (slow, small, dim) -> near (fast, big, bright).
    spawn_star_layer(
        &mut commands,
        &star_mesh,
        &palette,
        &mut rng,
        350,
        0.25,
        (0.7, 1.4),
    );
    spawn_star_layer(
        &mut commands,
        &star_mesh,
        &palette,
        &mut rng,
        300,
        0.5,
        (1.0, 2.0),
    );
    spawn_star_layer(
        &mut commands,
        &star_mesh,
        &palette,
        &mut rng,
        200,
        0.8,
        (1.4, 2.8),
    );
}

fn spawn_star_layer(
    commands: &mut Commands,
    star_mesh: &Handle<Mesh>,
    palette: &[Handle<ColorMaterial>],
    rng: &mut Rng,
    count: usize,
    strength: f32,
    scale: (f32, f32),
) {
    for _ in 0..count {
        let base = Vec2::new(
            rng.range(-STAR_TILE / 2., STAR_TILE / 2.),
            rng.range(-STAR_TILE / 2., STAR_TILE / 2.),
        );
        let s = rng.range(scale.0, scale.1);
        let mat = palette[(rng.next_u32() as usize) % palette.len()].clone();
        commands.spawn((
            Mesh2d(star_mesh.clone()),
            MeshMaterial2d(mat),
            Transform::from_xyz(base.x, base.y, STAR_Z).with_scale(Vec3::splat(s)),
            Parallax {
                base,
                strength,
                tile: STAR_TILE,
            },
        ));
    }
}

/// Each frame, place every parallax element relative to the camera: its on-screen
/// offset scrolls by `(1 - strength)` of the camera's motion, wrapped into a
/// `tile`-sized cell centered on the camera so the field tiles infinitely.
///
/// The scroll phase is computed from the camera's *world* position
/// (`WorldOrigin` + local, in f64) so the starfield is continuous across
/// floating-origin rebases — phase from the local camera position alone would
/// snap by `origin_shift * strength` every rebase. The wrapped offset is small,
/// so converting back to f32 costs nothing.
fn apply_parallax(
    origin: Res<crate::origin::WorldOrigin>,
    camera: Single<&Transform, With<Camera2d>>,
    mut elements: Query<(&mut Transform, &Parallax), Without<Camera2d>>,
) {
    let cam = camera.translation.xy();
    let cam_world = origin.0 + cam.as_dvec2();
    for (mut transform, p) in &mut elements {
        let screen_rel = p.base.as_dvec2() - cam_world * p.strength as f64;
        let tile = p.tile as f64;
        let half = tile / 2.;
        let wrapped = DVec2::new(
            (screen_rel.x + half).rem_euclid(tile) - half,
            (screen_rel.y + half).rem_euclid(tile) - half,
        )
        .as_vec2();
        let world = cam + wrapped;
        transform.translation.x = world.x;
        transform.translation.y = world.y;
    }
}
