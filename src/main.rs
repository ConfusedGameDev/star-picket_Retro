//! Star Picket for the Game Boy Advance: a port of the web game (https://star-picket.vercel.app).
//!
//! Prototype scope: fly a wrapping sector, clear hex stations by shooting their six cannon pods
//! or the core, dodge homing missiles and asteroids. Guns fire fore and aft on their own.
#![no_std]
#![no_main]

extern crate alloc;

use agb::{
    display::{
        AffineMatrix, Palette16, Priority, Rgb15,
        object::{
            AffineMatrixObject, AffineMode, DynamicSprite16, Object, ObjectAffine, PaletteVramSingle, Size, SpriteVram,
            include_aseprite,
        },
        tiled::{RegularBackground, RegularBackgroundSize, TileFormat},
    },
    fixnum::{Num, Vector2D, num, vec2},
    include_background_gfx,
    input::{Button, ButtonController},
};
use alloc::vec::Vec;

mod sound;
use sound::Ev;

include_aseprite!(
    mod sprites,
    32x32 "gfx/ships.png",
    32x32 "gfx/explosion.png",
    "gfx/bolt.png",
    "gfx/missile.png",
    "gfx/hull.png",
    "gfx/pod.png",
    16x16 "gfx/core.png",
    "gfx/rock_big.png",
    "gfx/rock_small.png",
    8x8 "gfx/digits.png",
);
include_background_gfx!(
    mod backdrop,
    "001020",
    FAR => deduplicate "gfx/stars_far.png",
    NEAR => deduplicate "gfx/stars_near.png",
);

type N = Num<i32, 8>;
type V = Vector2D<N>;

const WS: i32 = 2048; // the sector wraps at this size, about 8.5 screens across
const SW: i32 = 240;
const SH: i32 = 160;
const FACINGS: usize = 32; // ships snap to 32 directions, one shared rotation matrix each
const POD_R: i32 = 24; // pods sit on the hull's corners

// ---------- maths ----------
fn wrap(v: N) -> N { v.rem_euclid(N::new(WS)) }
fn wrapv(v: V) -> V { vec2(wrap(v.x), wrap(v.y)) }
/// shortest signed difference across the wrapping sector
fn wd(d: N) -> N { (d + N::new(WS / 2)).rem_euclid(N::new(WS)) - N::new(WS / 2) }
fn wdv(a: V, b: V) -> V { vec2(wd(a.x - b.x), wd(a.y - b.y)) }
fn dist2(a: V, b: V) -> i32 { let d = wdv(a, b); let (x, y) = (d.x.floor(), d.y.floor()); x * x + y * y }
fn near(a: V, b: V, r: i32) -> bool { dist2(a, b) < r * r }
/// angles are in turns: 0 = right, 0.25 = down (screen y points down)
fn dir(a: N) -> V { vec2(a.cos(), a.sin()) }
/// signed angle from a to b, in turns, in [-0.5, 0.5)
fn ang_diff(a: N, b: N) -> N { (b - a + num!(0.5)).rem_euclid(N::new(1)) - num!(0.5) }
fn facing(a: N) -> usize { ((a * FACINGS as i32).round() as usize) % FACINGS }
/// angle from a to b. Within each octant the ratio is close enough to the true angle for steering.
fn ang_to(a: V, b: V) -> N {
    let d = wdv(b, a);
    let (x, y) = (d.x.floor(), d.y.floor());
    if x == 0 && y == 0 { return N::new(0); }
    let (ax, ay) = (x.abs(), y.abs());
    let base = if ax >= ay { N::new(ay) / N::new(ax) / 8 } else { num!(0.25) - N::new(ax) / N::new(ay) / 8 };
    let a = match (x >= 0, y >= 0) {
        (true, true) => base,
        (false, true) => num!(0.5) - base,
        (false, false) => num!(0.5) + base,
        (true, false) => N::new(1) - base,
    };
    a.rem_euclid(N::new(1))
}
fn rnd(lo: i32, hi: i32) -> i32 { lo + (agb::rng::next_i32().unsigned_abs() as i32).rem_euclid((hi - lo).max(1)) }
fn rndn(lo: N, hi: N) -> N { lo + N::from_raw(rnd(0, (hi - lo).to_raw().max(1))) }
fn turn_toward(a: N, target: N, max: N) -> N {
    let d = ang_diff(a, target);
    (a + if d > max { max } else if d < -max { -max } else { d }).rem_euclid(N::new(1))
}
fn pod_off(k: usize) -> V { dir(N::new(k as i32) / 6) * POD_R }
fn zero() -> V { vec2(N::new(0), N::new(0)) }

// ---------- game state ----------
struct Player { pos: V, a: N, v: N, inv: i32, alive: bool, dead_t: i32 }
struct Bullet { pos: V, vel: V, life: i32, f: usize }
struct Station { pos: V, pods: [i8; 6], pod_flash: [u8; 6], core: i8, core_flash: u8, alive: bool, fire_t: i32 }
struct Missile { pos: V, a: N, life: i32 }
struct Rock { pos: V, vel: V, big: bool }
struct Boom { pos: V, t: i32 }

struct Game {
    p: Player,
    cam: V,
    bullets: Vec<Bullet>,
    stations: Vec<Station>,
    miss: Vec<Missile>,
    rocks: Vec<Rock>,
    booms: Vec<Boom>,
    events: Vec<Ev>,
    score: u32,
    lives: i32,
    round: i32,
    fire_t: i32,
    clear_t: i32,
    over_t: i32,
    frame: u32,
}

impl Game {
    fn new() -> Self {
        let mid = vec2(N::new(WS / 2), N::new(WS / 2));
        let mut g = Game {
            p: Player { pos: mid, a: num!(0.75), v: num!(1.75), inv: 120, alive: true, dead_t: 0 },
            cam: mid,
            bullets: Vec::new(), stations: Vec::new(), miss: Vec::new(), rocks: Vec::new(), booms: Vec::new(), events: Vec::new(),
            score: 0, lives: 3, round: 0, fire_t: 0, clear_t: 0, over_t: 0, frame: 0,
        };
        g.next_round();
        g
    }

    fn next_round(&mut self) {
        self.round += 1;
        self.stations.clear(); self.miss.clear(); self.rocks.clear();
        let n = (3 + self.round).min(10) as usize;
        let mut tries = 0;
        while self.stations.len() < n && tries < 2000 {
            tries += 1;
            let pos = vec2(N::new(rnd(0, WS)), N::new(rnd(0, WS)));
            if near(pos, self.p.pos, 360) || self.stations.iter().any(|s| near(s.pos, pos, 400)) { continue; }
            self.stations.push(Station { pos, pods: [2; 6], pod_flash: [0; 6], core: 3, core_flash: 0, alive: true, fire_t: rnd(60, 180) });
        }
        // `cargo build --features showcase`: park the first station just ahead of you, for screenshots and testing
        if cfg!(feature = "showcase") && self.round == 1 {
            if let Some(s) = self.stations.first_mut() { s.pos = wrapv(self.p.pos + vec2(N::new(30), N::new(-260))); }
            let pp = self.p.pos;
            self.rocks.retain(|r| !near(r.pos, pp, 200));
        }
        tries = 0;
        while self.rocks.len() < 30 && tries < 3000 {
            tries += 1;
            let pos = vec2(N::new(rnd(0, WS)), N::new(rnd(0, WS)));
            if near(pos, self.p.pos, 110) || self.stations.iter().any(|s| near(s.pos, pos, 90)) { continue; }
            let vel = vec2(rndn(num!(-0.08), num!(0.08)), rndn(num!(-0.08), num!(0.08)));
            self.rocks.push(Rock { pos, vel, big: self.rocks.len() % 3 == 0 });
        }
    }

    fn kill_player(&mut self) {
        if !self.p.alive || self.p.inv > 0 { return; }
        self.p.alive = false; self.lives -= 1; self.p.dead_t = 150;
        self.booms.push(Boom { pos: self.p.pos, t: 0 });
        self.booms.push(Boom { pos: self.p.pos + vec2(N::new(8), N::new(-5)), t: -8 });
        self.events.push(Ev::Die);
    }

    fn destroy_station(&mut self, i: usize) {
        let s = &mut self.stations[i];
        if !s.alive { return; }
        s.alive = false;
        let pos = s.pos;
        for k in 0..6 {
            if s.pods[k] > 0 { s.pods[k] = 0; }
            self.booms.push(Boom { pos: wrapv(pos + pod_off(k)), t: -(k as i32) * 3 });
        }
        self.booms.push(Boom { pos, t: 0 });
        self.score += 1500;
        self.events.push(Ev::Station);
        if self.stations.iter().all(|s| !s.alive) && self.clear_t == 0 { self.clear_t = 180; }
    }

    fn update(&mut self, input: &ButtonController) {
        self.frame = self.frame.wrapping_add(1);
        if self.over_t > 0 {
            self.over_t -= 1;
            if self.over_t == 0 || (self.over_t < 180 && input.is_just_pressed(Button::Start)) { *self = Game::new(); }
            return;
        }

        // --- player: the D-pad picks a heading, the ship turns toward it and speeds up ---
        let (ix, iy) = (input.x_tri() as i32, input.y_tri() as i32);
        if self.p.alive {
            let steering = ix != 0 || iy != 0;
            if steering {
                let target = ang_to(zero(), vec2(N::new(ix * 100), N::new(iy * 100)));
                self.p.a = turn_toward(self.p.a, target, num!(0.02));
            }
            let target_v = if steering { num!(2.9) } else { num!(1.75) };
            self.p.v += (target_v - self.p.v) / 15;
            self.p.pos = wrapv(self.p.pos + dir(self.p.a) * self.p.v);
            if self.p.inv > 0 { self.p.inv -= 1; }
            self.fire_t -= 1;
            if self.fire_t <= 0 {
                self.fire_t = 8;
                let d = dir(self.p.a);
                let f = facing(self.p.a);
                self.bullets.push(Bullet { pos: self.p.pos + d * 9, vel: d * (num!(7.0) + self.p.v), life: 30, f });
                self.bullets.push(Bullet { pos: self.p.pos - d * 7, vel: -d * (num!(7.0) - self.p.v / 2), life: 24, f: (f + FACINGS / 2) % FACINGS });
            }
        } else if self.p.dead_t > 0 {
            self.p.dead_t -= 1;
            if self.p.dead_t == 0 {
                if self.lives > 0 {
                    self.p.alive = true; self.p.inv = 150; self.p.v = num!(1.75);
                    let pp = self.p.pos;
                    self.miss.retain(|m| !near(m.pos, pp, 280));
                    self.rocks.retain(|r| !near(r.pos, pp, 40));
                } else { self.over_t = 240; }
            }
        }

        // camera leads the ship a little
        let lead = if self.p.alive { dir(self.p.a) * 38 } else { zero() };
        let d = wdv(wrapv(self.p.pos + lead), self.cam);
        self.cam = wrapv(self.cam + d / 12);

        // --- bullets ---
        let mut bullets = core::mem::take(&mut self.bullets);
        for b in bullets.iter_mut() {
            b.pos = wrapv(b.pos + b.vel); b.life -= 1;
            if b.life > 0 && self.bullet_hits(b.pos) { b.life = 0; }
        }
        bullets.retain(|b| b.life > 0);
        self.bullets = bullets;

        // --- stations fire at you when you are close ---
        let round = self.round;
        let mut crash = false;
        for i in 0..self.stations.len() {
            let s = &mut self.stations[i];
            if !s.alive { continue; }
            for f in s.pod_flash.iter_mut() { *f = f.saturating_sub(1); }
            s.core_flash = s.core_flash.saturating_sub(1);
            if !self.p.alive { continue; }
            if near(s.pos, self.p.pos, 310) {
                s.fire_t -= 1;
                if s.fire_t <= 0 {
                    s.fire_t = (102 - round * 5).max(42) + rnd(0, 48);
                    let pp = self.p.pos;
                    let best = (0..6).filter(|&k| s.pods[k] > 0).min_by_key(|&k| dist2(s.pos + pod_off(k), pp));
                    if let Some(k) = best {
                        let from = wrapv(s.pos + pod_off(k));
                        self.miss.push(Missile { pos: from, a: ang_to(from, pp), life: 270 });
                        self.events.push(Ev::Launch);
                    }
                }
            }
            if self.p.inv <= 0 && near(s.pos, self.p.pos, 40)
                && (near(s.pos, self.p.pos, 15) || (0..6).any(|k| s.pods[k] > 0 && near(s.pos + pod_off(k), self.p.pos, 13))) {
                crash = true;
            }
        }
        if crash { self.kill_player(); }

        // --- missiles home in ---
        let pp = self.p.pos;
        let alive = self.p.alive;
        let vulnerable = alive && self.p.inv <= 0;
        let mut hit_player = false;
        for m in self.miss.iter_mut() {
            if alive { m.a = turn_toward(m.a, ang_to(m.pos, pp), num!(0.0035)); }
            m.pos = wrapv(m.pos + dir(m.a) * num!(2.15));
            m.life -= 1;
            if m.life <= 0 { self.booms.push(Boom { pos: m.pos, t: 8 }); }
            if vulnerable && near(m.pos, pp, 7) { m.life = 0; hit_player = true; }
        }
        self.miss.retain(|m| m.life > 0);

        // --- rocks drift ---
        for r in self.rocks.iter_mut() {
            r.pos = wrapv(r.pos + r.vel);
            if vulnerable && near(r.pos, pp, if r.big { 25 } else { 15 }) { hit_player = true; }
        }
        if hit_player { self.kill_player(); }

        for b in self.booms.iter_mut() { b.t += 1; }
        self.booms.retain(|b| b.t < 32);

        if self.clear_t > 0 { self.clear_t -= 1; if self.clear_t == 0 { self.next_round(); } }
    }

    /// true when a bullet at `at` hit something
    fn bullet_hits(&mut self, at: V) -> bool {
        for i in 0..self.stations.len() {
            let s = &mut self.stations[i];
            if !s.alive || !near(s.pos, at, 34) { continue; }
            for k in 0..6 {
                if s.pods[k] > 0 && near(s.pos + pod_off(k), at, 8) {
                    s.pods[k] -= 1; s.pod_flash[k] = 5;
                    if s.pods[k] == 0 {
                        let pos = wrapv(s.pos + pod_off(k));
                        let all_gone = s.pods.iter().all(|&h| h == 0);
                        self.booms.push(Boom { pos, t: 0 });
                        self.score += 200;
                        self.events.push(Ev::Pod);
                        if all_gone { self.destroy_station(i); }
                    } else { self.events.push(Ev::Hit); }
                    return true;
                }
            }
            if near(s.pos, at, 9) {
                s.core -= 1; s.core_flash = 6;
                let dead = s.core <= 0;
                if dead { self.destroy_station(i); } else { self.events.push(Ev::Hit); }
                return true;
            }
        }
        if let Some(i) = self.miss.iter().position(|m| near(m.pos, at, 6)) {
            let m = self.miss.swap_remove(i);
            self.booms.push(Boom { pos: m.pos, t: 6 });
            self.score += 10;
            self.events.push(Ev::Missile);
            return true;
        }
        if let Some(i) = self.rocks.iter().position(|r| near(r.pos, at, if r.big { 20 } else { 10 })) {
            let r = self.rocks.swap_remove(i);
            self.booms.push(Boom { pos: r.pos, t: 0 });
            self.score += 10;
            self.events.push(Ev::Rock);
            return true;
        }
        false
    }

    /// world position to screen position (centre of the object)
    fn to_screen(&self, pos: V) -> (i32, i32) {
        let d = wdv(pos, self.cam);
        (d.x.floor() + SW / 2, d.y.floor() + SH / 2)
    }
}

fn on_screen((x, y): (i32, i32), m: i32) -> bool { x > -m && x < SW + m && y > -m && y < SH + m }

// ---------- radar ----------
const fn rgb(r: u16, g: u16, b: u16) -> Rgb15 { Rgb15::new((r >> 3) | ((g >> 3) << 5) | ((b >> 3) << 10)) }
const BLACK: Rgb15 = rgb(0, 0, 0);
const RADAR_PALETTE: Palette16 = Palette16::new([
    BLACK, rgb(10, 14, 34), rgb(80, 74, 130), rgb(255, 93, 143), rgb(255, 255, 255), rgb(98, 227, 255),
    BLACK, BLACK, BLACK, BLACK, BLACK, BLACK, BLACK, BLACK, BLACK, BLACK,
]);

/// the whole sector on a 32x32 sprite, centred on you
fn draw_radar(g: &Game, palette: &PaletteVramSingle) -> SpriteVram {
    let mut s = DynamicSprite16::new(Size::S32x32);
    s.clear(1);
    for i in 0..32 { s.set_pixel(i, 0, 2); s.set_pixel(i, 31, 2); s.set_pixel(0, i, 2); s.set_pixel(31, i, 2); }
    let plot = |s: &mut DynamicSprite16, pos: V, c: u8, big: bool| {
        let d = wdv(pos, g.p.pos);
        let (x, y) = (16 + d.x.floor() * 30 / WS, 16 + d.y.floor() * 30 / WS);
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)].into_iter().take(if big { 4 } else { 1 }) {
            let (px, py) = (x + dx, y + dy);
            if (1..31).contains(&px) && (1..31).contains(&py) { s.set_pixel(px as usize, py as usize, c); }
        }
    };
    for st in g.stations.iter().filter(|s| s.alive) { plot(&mut s, st.pos, 3, true); }
    for m in &g.miss { plot(&mut s, m.pos, 5, false); }
    plot(&mut s, g.p.pos, 4, true);
    s.to_vram(palette.clone())
}

#[agb::entry]
fn main(mut gba: agb::Gba) -> ! {
    let mut gfx = gba.graphics.get();
    let mut input = ButtonController::new();
    let mut audio = sound::Audio::new(&mut gba.mixer);

    gfx.set_background_palettes(backdrop::PALETTES);
    let mut far = RegularBackground::new(Priority::P3, RegularBackgroundSize::Background32x32, TileFormat::FourBpp);
    far.fill_with(&backdrop::FAR);
    let mut near_bg = RegularBackground::new(Priority::P2, RegularBackgroundSize::Background32x32, TileFormat::FourBpp);
    near_bg.fill_with(&backdrop::NEAR);

    // one rotation matrix per facing, shared by every rotated sprite (the hardware allows 32)
    let matrices: Vec<AffineMatrixObject> = (0..FACINGS)
        .map(|k| AffineMatrixObject::new(AffineMatrix::from_rotation(-N::new(k as i32) / FACINGS as i32)))
        .collect();

    let radar_palette = PaletteVramSingle::try_allocate_new(&RADAR_PALETTE).expect("palette space for the radar");
    let mut g = Game::new();
    let mut radar = draw_radar(&g, &radar_palette);
    let mut paused = false;

    loop {
        input.update();
        if input.is_just_pressed(Button::Start) && g.over_t == 0 { paused = !paused; }
        if !paused { g.update(&input); }
        for ev in g.events.drain(..) { audio.play(ev); }
        // the band drops out while you respawn and gets busier while a station has you in range
        audio.alive = g.p.alive && !paused;
        audio.tense = g.p.alive && g.stations.iter().any(|s| s.alive && near(s.pos, g.p.pos, 310));
        if g.frame % 4 == 0 { radar = draw_radar(&g, &radar_palette); }

        let mut frame = gfx.frame();
        let mut budget = 124usize; // OAM holds 128 objects
        // objects shown first draw on top, so: HUD, effects, ships, stations, rocks
        macro_rules! show {
            ($obj:expr) => { if budget > 0 { budget -= 1; $obj.show(&mut frame); } };
        }

        let mut sc = g.score;
        for i in (0..7).rev() {
            let mut o = Object::new(sprites::DIGITS.sprite((sc % 10) as usize));
            o.set_pos((4 + i * 7, 4));
            show!(o);
            sc /= 10;
        }
        for i in 0..g.lives.clamp(0, 5) {
            let mut o = Object::new(sprites::DIGITS.sprite(10));
            o.set_pos((4 + i * 9, 14));
            show!(o);
        }
        let mut r = Object::new(radar.clone());
        r.set_pos((SW - 36, 4));
        show!(r);

        for b in g.booms.iter().filter(|b| b.t >= 0) {
            let s = g.to_screen(b.pos);
            if !on_screen(s, 20) { continue; }
            let mut o = Object::new(sprites::EXPLOSION.sprite((b.t / 4).min(7) as usize));
            o.set_pos((s.0 - 16, s.1 - 16));
            show!(o);
        }
        if g.p.alive && (g.p.inv == 0 || (g.frame / 4) % 2 == 0) {
            let s = g.to_screen(g.p.pos);
            let mut o = ObjectAffine::new(sprites::SHIPS.sprite(0), matrices[facing(g.p.a)].clone(), AffineMode::AffineDouble);
            o.set_pos((s.0 - 32, s.1 - 32));
            show!(o);
        }
        for b in &g.bullets {
            let s = g.to_screen(b.pos);
            if !on_screen(s, 8) { continue; }
            let mut o = ObjectAffine::new(sprites::BOLT.sprite(0), matrices[b.f].clone(), AffineMode::Affine);
            o.set_pos((s.0 - 8, s.1 - 8));
            show!(o);
        }
        for m in &g.miss {
            let s = g.to_screen(m.pos);
            if !on_screen(s, 8) { continue; }
            let mut o = ObjectAffine::new(sprites::MISSILE.sprite(0), matrices[facing(m.a)].clone(), AffineMode::Affine);
            o.set_pos((s.0 - 8, s.1 - 8));
            show!(o);
        }
        for st in g.stations.iter().filter(|s| s.alive) {
            let s = g.to_screen(st.pos);
            if !on_screen(s, 40) { continue; }
            for k in 0..6 {
                if st.pods[k] == 0 || st.pod_flash[k] % 2 == 1 { continue; }
                let off = pod_off(k);
                let mut o = Object::new(sprites::POD.sprite(0));
                o.set_pos((s.0 + off.x.round() - 8, s.1 + off.y.round() - 8));
                show!(o);
            }
            let mut o = Object::new(sprites::CORE.sprite(if st.core_flash > 0 { 1 } else { 0 }));
            o.set_pos((s.0 - 8, s.1 - 8));
            show!(o);
            let mut o = Object::new(sprites::HULL.sprite(0));
            o.set_pos((s.0 - 32, s.1 - 32));
            show!(o);
        }
        for rk in &g.rocks {
            let s = g.to_screen(rk.pos);
            let (spr, h) = if rk.big { (sprites::ROCK_BIG.sprite(0), 32) } else { (sprites::ROCK_SMALL.sprite(0), 16) };
            if !on_screen(s, h) { continue; }
            let mut o = Object::new(spr);
            o.set_pos((s.0 - h, s.1 - h));
            show!(o);
        }

        // parallax: both star layers move slower than the world, the far one slowest
        far.set_scroll_pos((g.cam.x.floor() / 6, g.cam.y.floor() / 6));
        near_bg.set_scroll_pos((g.cam.x.floor() / 2, g.cam.y.floor() / 2));
        far.show(&mut frame);
        near_bg.show(&mut frame);

        audio.frame();
        frame.commit();
    }
}
