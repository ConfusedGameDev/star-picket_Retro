//! Star Picket for the Game Boy Advance: a port of the web game (https://star-picket.vercel.app).
//!
//! Prototype scope: fly a wrapping sector, clear hex stations by shooting their six cannon pods
//! or the core, dodge homing missiles and asteroids. Guns fire fore and aft on their own.
#![no_std]
#![no_main]

extern crate alloc;

use agb::{
    display::{
        AffineMatrix, GraphicsFrame, Palette16, Priority, Rgb15,
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
use alloc::{format, vec::Vec};

mod scores;
mod sound;
mod ui;
use sound::Ev;
use ui::Ink;

include_aseprite!(
    mod sprites,
    32x32 "gfx/ships.png",
    32x32 "gfx/explosion.png",
    "gfx/bolt.png",
    "gfx/missile.png",
    64x64 "gfx/hulls.png",
    "gfx/pod.png",
    16x16 "gfx/cores.png",
    "gfx/rock_big.png",
    "gfx/rock_small.png",
    8x8 "gfx/digits.png",
);
include_background_gfx!(
    mod backdrop,
    "001020",
    FAR => deduplicate "gfx/stars_far.png",
    NEAR => deduplicate "gfx/stars_near.png",
    FONT => "gfx/font.png",
    LOGO => "gfx/logo.png",
);

type N = Num<i32, 8>;
type V = Vector2D<N>;

const WS: i32 = 2048; // the sector wraps at this size, about 8.5 screens across
const SW: i32 = 240;
const SH: i32 = 160;
const FACINGS: usize = 32; // ships snap to 32 directions, one shared rotation matrix each

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
fn zero() -> V { vec2(N::new(0), N::new(0)) }

// ---------- game state ----------
struct Player { pos: V, a: N, v: N, inv: i32, alive: bool, dead_t: i32 }
struct Bullet { pos: V, vel: V, life: i32, f: usize }
/// One of the five station designs. Pod positions are pixels from the centre before rotation, and
/// must match DESIGNS in tools/make_gfx.py, which draws the hull through them.
struct Design { pods: &'static [(i32, i32)], core: i8, core_frame: usize, fire_pct: i32, volley: usize }
const HEX: usize = 0;
const DESIGNS: [Design; 5] = [
    // the original six-pod hexagon
    Design { pods: &[(24, 0), (12, 21), (-12, 21), (-24, 0), (-12, -21), (12, -21)], core: 3, core_frame: 0, fire_pct: 100, volley: 1 },
    // lance: a long hull, a cannon at each tip
    Design { pods: &[(-34, 0), (-17, -15), (17, -15), (34, 0), (17, 15), (-17, 15)], core: 3, core_frame: 0, fire_pct: 100, volley: 1 },
    // trident: three arms, each ending in a fork of two pods
    Design { pods: &[(-10, -30), (10, -30), (31, 6), (21, 24), (-21, 24), (-31, 6)], core: 3, core_frame: 2, fire_pct: 100, volley: 1 },
    // diamond: small, a fragile core, but it fires faster
    Design { pods: &[(30, 0), (0, -20), (-30, 0), (0, 20)], core: 2, core_frame: 4, fire_pct: 70, volley: 1 },
    // crown: eight pods, a tougher core, and it fires missiles in pairs
    Design { pods: &[(30, 12), (12, 30), (-12, 30), (-30, 12), (-30, -12), (-12, -30), (12, -30), (30, -12)], core: 4, core_frame: 6, fire_pct: 115, volley: 2 },
];
/// which designs a round deals stations from; bigger ones join as the rounds go up
fn station_pool(round: i32) -> &'static [usize] {
    match round { 1 => &[0, 0, 3, 1], 2 => &[0, 3, 1, 2], 3 | 4 => &[0, 3, 1, 2, 2, 4], _ => &[0, 3, 1, 2, 4, 4] }
}
/// `alerted` counts down while the base knows you're near (or just shot at it): that's Condition Yellow.
/// `offs` are the pod positions after the station's rotation; `facing` picks the shared rotation matrix.
struct Station {
    pos: V, design: usize, facing: usize, n: usize, offs: [V; 8], pods: [i8; 8], pod_flash: [u8; 8],
    core: i8, core_flash: u8, alive: bool, fire_t: i32, alerted: i32,
}
impl Station {
    fn new(pos: V, design: usize, facing: usize) -> Self {
        let d = &DESIGNS[design];
        let a = N::new(facing as i32) / FACINGS as i32;
        let mut offs = [vec2(N::new(0), N::new(0)); 8];
        let mut pods = [0; 8];
        for (k, &p) in d.pods.iter().enumerate() { offs[k] = rotate(p, a); pods[k] = 2; }
        Station { pos, design, facing, n: d.pods.len(), offs, pods, pod_flash: [0; 8], core: d.core, core_flash: 0, alive: true, fire_t: rnd(60, 180), alerted: 0 }
    }
    /// bounding radius: the furthest pod plus a little
    fn reach(&self) -> i32 { if self.design == HEX { 34 } else { 44 } }
}
struct Missile { pos: V, a: N, life: i32 }
struct Rock { pos: V, vel: V, big: bool }
struct Boom { pos: V, t: i32 }

// ---------- enemy ships ----------
/// Squadron types, after the web version's I / P / E ships
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind { I, P, E }
struct KindStats { name: &'static str, sp: N, turn: N, pts: u32, lead: u32, bonus: u32, frame: usize, lead_frame: usize, radar: u8 }
fn stats(k: Kind) -> KindStats {
    match k {
        // I weaves at you, P swings in from your flank like a boomerang, E is a dart that barely turns.
        // Faster relative to you than on the web: the GBA screen shows far less of the sector, so
        // enemies that trail behind would spend the whole time off-screen. I-types match your cruise.
        Kind::I => KindStats { name: "I", sp: num!(1.7), turn: num!(0.004), pts: 50, lead: 100, bonus: 500, frame: 1, lead_frame: 5, radar: 6 },
        Kind::P => KindStats { name: "P", sp: num!(2.0), turn: num!(0.0064), pts: 60, lead: 120, bonus: 1000, frame: 2, lead_frame: 6, radar: 5 },
        Kind::E => KindStats { name: "E", sp: num!(3.4), turn: num!(0.0009), pts: 70, lead: 140, bonus: 1500, frame: 3, lead_frame: 7, radar: 7 },
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode { Hunt, Form, Flee }
struct Fighter { pos: V, a: N, sp: N, kind: Kind, leader: bool, squad: u32, slot: usize, mode: Mode, wob: N, dead: bool }
/// The spy ship circles you at a distance. Three long looks and it gets away with your position.
struct Spy { pos: V, a: N, sp: N, passes: i32, far: i32, orbit: N, wob: N }
/// why the condition changed, for the banner
#[derive(Clone, Copy, PartialEq, Eq)]
enum Why { Noticed, TooLong, Reported }
pub const GREEN: u8 = 0;
pub const YELLOW: u8 = 1;
pub const RED: u8 = 2;
/// squad 0 means a lone fighter
struct Squad { id: u32, t: i32, done: bool }
/// wingmen hold a V behind the leader, in the leader's frame (x forward, y to the side)
const SLOTS: [(i32, i32); 4] = [(-16, -14), (-16, 14), (-33, -29), (-33, 29)];
fn rotate(v: (i32, i32), a: N) -> V { let d = dir(a); vec2(d.x * v.0 - d.y * v.1, d.y * v.0 + d.x * v.1) }

struct Game {
    p: Player,
    cam: V,
    bullets: Vec<Bullet>,
    stations: Vec<Station>,
    miss: Vec<Missile>,
    rocks: Vec<Rock>,
    booms: Vec<Boom>,
    fighters: Vec<Fighter>,
    squads: Vec<Squad>,
    next_squad: u32,
    spawn_t: i32,
    squad_t: i32,
    spy: Option<Spy>,
    spy_t: i32,
    spy_streak: usize,
    /// alert condition: GREEN, YELLOW or RED. `cond_min` is the floor the clock has raised it to.
    cond: u8,
    cond_min: u8,
    round_t: i32,
    siren_t: i32,
    events: Vec<Ev>,
    score: u32,
    lives: i32,
    round: i32,
    fire_t: i32,
    clear_t: i32,
    /// the last life is gone; the menus take over
    over: bool,
    banner: Option<(Banner, i32)>,
    frame: u32,
}

/// short messages shown mid-screen during play
#[derive(Clone, Copy, PartialEq, Eq)]
enum Banner { Round(i32), Clear, BattleStations(Kind), Broken, Squadron(u32), Yellow(Why), Red(Why), SpySighted, SpyDown(u32) }

impl Game {
    fn new() -> Self {
        let mid = vec2(N::new(WS / 2), N::new(WS / 2));
        let mut g = Game {
            p: Player { pos: mid, a: num!(0.75), v: num!(1.75), inv: 120, alive: true, dead_t: 0 },
            cam: mid,
            bullets: Vec::new(), stations: Vec::new(), miss: Vec::new(), rocks: Vec::new(), booms: Vec::new(), events: Vec::new(),
            fighters: Vec::new(), squads: Vec::new(), next_squad: 1, spawn_t: 0, squad_t: 0,
            spy: None, spy_t: 0, spy_streak: 0, cond: GREEN, cond_min: GREEN, round_t: 0, siren_t: 0,
            score: 0, lives: if cfg!(feature = "quickover") { 1 } else { 3 }, round: 0, fire_t: 0, clear_t: 0, over: false, banner: None, frame: 0,
        };
        g.next_round();
        g
    }

    fn next_round(&mut self) {
        self.round += 1;
        self.banner = Some((Banner::Round(self.round), 120));
        self.stations.clear(); self.miss.clear(); self.rocks.clear(); self.fighters.clear(); self.squads.clear();
        self.spawn_t = 240; // 4 s before the first lone fighter
        self.squad_t = rnd(540, 840); // 9 to 14 s before the first squadron
        self.spy = None;
        self.spy_t = rnd(840, 1320); // 14 to 22 s before the spy ship
        self.cond = GREEN; self.cond_min = GREEN; self.round_t = 0;
        let n = (3 + self.round).min(10) as usize;
        let mut tries = 0;
        let mut deck: Vec<usize> = Vec::new();
        while self.stations.len() < n && tries < 2000 {
            tries += 1;
            let pos = vec2(N::new(rnd(0, WS)), N::new(rnd(0, WS)));
            if near(pos, self.p.pos, 360) || self.stations.iter().any(|s| near(s.pos, pos, 400)) { continue; }
            // deal designs from a shuffled deck so a sector always mixes them
            if deck.is_empty() {
                deck.extend_from_slice(station_pool(self.round));
                for i in (1..deck.len()).rev() { deck.swap(i, rnd(0, i as i32 + 1) as usize); }
            }
            let design = deck.pop().unwrap_or(HEX);
            self.stations.push(Station::new(pos, design, rnd(0, FACINGS as i32) as usize));
        }
        // `--features gallery`: one of each station design around you, frozen, for screenshots
        if cfg!(feature = "gallery") {
            let spots = [(-78, -40, 0), (0, -44, 3), (78, -40, 7), (-46, 42, 12), (46, 42, 20)];
            self.stations = spots.iter().enumerate().map(|(d, &(x, y, f))| Station::new(wrapv(self.p.pos + vec2(N::new(x), N::new(y))), d, f)).collect();
            let pp = self.p.pos;
            self.rocks.retain(|r| !near(r.pos, pp, 200));
        }
        // `cargo build --features showcase`: park the first station just ahead of you, for screenshots and testing
        if cfg!(feature = "showcase") && self.round == 1 {
            if let Some(s) = self.stations.first_mut() { s.pos = wrapv(self.p.pos + vec2(N::new(30), N::new(-260))); }
            let pp = self.p.pos;
            self.rocks.retain(|r| !near(r.pos, pp, 200));
            self.squad_t = 60; // and send a squadron right away
            self.spawn_t = 90;
            self.spy_t = 120; // and the spy ship
            self.round_t = 5100 - 150 - 900; // and Condition Red 15 s in
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
        for k in 0..s.n {
            if s.pods[k] > 0 { s.pods[k] = 0; }
            self.booms.push(Boom { pos: wrapv(pos + s.offs[k]), t: -(k as i32) * 3 });
        }
        self.booms.push(Boom { pos, t: 0 });
        self.score += 1500;
        self.events.push(Ev::Station);
        if self.stations.iter().all(|s| !s.alive) && self.clear_t == 0 {
            self.clear_t = 180;
            self.banner = Some((Banner::Clear, 170));
        }
    }

    fn update(&mut self, input: &ButtonController) {
        self.frame = self.frame.wrapping_add(1);
        if self.over || cfg!(feature = "gallery") { return; }
        // `--features quickover`: the only ship explodes 3 seconds in, for testing the menus
        if cfg!(feature = "quickover") && self.frame == 180 { self.p.inv = 0; self.kill_player(); }
        if let Some((_, t)) = &mut self.banner { *t -= 1; if *t <= 0 { self.banner = None; } }

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
                    for i in 0..self.fighters.len() { if near(self.fighters[i].pos, pp, 250) { self.kill_fighter(i, false); } }
                    self.rocks.retain(|r| !near(r.pos, pp, 40));
                } else { self.over = true; }
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
            if s.alerted > 0 { s.alerted -= 1; }
            if !self.p.alive { continue; }
            if near(s.pos, self.p.pos, 310) {
                s.alerted = s.alerted.max(120);
                s.fire_t -= 1;
                if s.fire_t <= 0 {
                    let d = &DESIGNS[s.design];
                    s.fire_t = ((102 - round * 5).max(42) + rnd(0, 48)) * d.fire_pct / 100;
                    let pp = self.p.pos;
                    // the pods nearest you fire: one missile for most designs, a pair from the crown
                    let mut live: Vec<usize> = (0..s.n).filter(|&k| s.pods[k] > 0).collect();
                    live.sort_by_key(|&k| dist2(s.pos + s.offs[k], pp));
                    for (j, &k) in live.iter().take(d.volley).enumerate() {
                        let from = wrapv(s.pos + s.offs[k]);
                        let spread = if j > 0 { rndn(num!(-0.04), num!(0.04)) } else { num!(0) };
                        self.miss.push(Missile { pos: from, a: (ang_to(from, pp) + spread).rem_euclid(N::new(1)), life: 270 });
                    }
                    if !live.is_empty() { self.events.push(Ev::Launch); }
                }
            }
            if self.p.inv <= 0 && near(s.pos, self.p.pos, s.reach() + 6)
                && (near(s.pos, self.p.pos, 15) || (0..s.n).any(|k| s.pods[k] > 0 && near(s.pos + s.offs[k], self.p.pos, 13))) {
                crash = true;
            }
        }
        if crash { self.kill_player(); }

        if self.p.alive && self.clear_t == 0 { self.update_cond(); }
        if self.cond == RED {
            self.siren_t -= 1;
            if self.siren_t <= 0 { self.siren_t = 144; self.events.push(Ev::Siren); }
        }
        self.update_fighters();
        self.update_spy();

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

        self.fighters.retain(|f| !f.dead);
        self.squads.retain(|s| !s.done);
        for b in self.booms.iter_mut() { b.t += 1; }
        self.booms.retain(|b| b.t < 32);

        if self.clear_t > 0 { self.clear_t -= 1; if self.clear_t == 0 { self.next_round(); } }
    }

    /// true when a bullet at `at` hit something
    fn bullet_hits(&mut self, at: V) -> bool {
        for i in 0..self.stations.len() {
            let s = &mut self.stations[i];
            if !s.alive || !near(s.pos, at, s.reach()) { continue; }
            for k in 0..s.n {
                if s.pods[k] > 0 && near(s.pos + s.offs[k], at, 8) {
                    s.pods[k] -= 1; s.pod_flash[k] = 5; s.alerted = 360;
                    if s.pods[k] == 0 {
                        let pos = wrapv(s.pos + s.offs[k]);
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
                s.core -= 1; s.core_flash = 6; s.alerted = 360;
                let dead = s.core <= 0;
                if dead { self.destroy_station(i); } else { self.events.push(Ev::Hit); }
                return true;
            }
        }
        if self.spy.as_ref().is_some_and(|s| near(s.pos, at, 9)) { self.kill_spy(); return true; }
        if let Some(i) = self.fighters.iter().position(|f| !f.dead && near(f.pos, at, 8)) {
            self.kill_fighter(i, true);
            return true;
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

    fn kill_fighter(&mut self, i: usize, award: bool) {
        let f = &mut self.fighters[i];
        if f.dead { return; }
        f.dead = true;
        let (pos, sid, leader, st) = (f.pos, f.squad, f.leader, stats(f.kind));
        self.booms.push(Boom { pos, t: 0 });
        if award {
            self.score += if leader { st.lead } else { st.pts };
            self.events.push(if leader { Ev::Leader } else { Ev::Fighter });
        }
        if sid == 0 { return; }
        let pp = self.p.pos;
        let mut left = 0;
        for m in self.fighters.iter_mut().filter(|m| m.squad == sid && !m.dead) {
            left += 1;
            if leader {
                // the formation breaks: wingmen peel off and run
                m.mode = Mode::Flee;
                m.a = (ang_to(pp, m.pos) + rndn(num!(-0.1), num!(0.1))).rem_euclid(N::new(1));
                m.sp = m.sp * num!(1.35);
            }
        }
        if leader && left > 0 { self.banner = Some((Banner::Broken, 90)); }
        if left == 0 {
            if let Some(s) = self.squads.iter_mut().find(|s| s.id == sid) { s.done = true; }
            if award {
                // the whole squadron, every ship shot down
                self.score += st.bonus;
                self.events.push(Ev::Squad);
                self.banner = Some((Banner::Squadron(st.bonus), 120));
            }
        }
    }

    /// raise the condition (it never drops within a round once the clock or the spy has raised it)
    fn set_cond(&mut self, c: u8, why: Why) {
        if c <= self.cond { return; }
        self.cond = c;
        if c == RED {
            self.cond_min = RED;
            self.siren_t = 0;
            self.banner = Some((Banner::Red(why), 180));
            self.events.push(Ev::Alert);
            // the bases commit what they have: a squadron right away and singles pour in
            self.squad_t = self.squad_t.min(90);
            self.spawn_t = self.spawn_t.min(30);
        } else {
            self.banner = Some((Banner::Yellow(why), 110));
        }
    }

    fn update_cond(&mut self) {
        self.round_t += 1;
        let (yellow_at, red_at) = (1800, (5100 - self.round * 150).max(3000)); // 30 s; 85 s falling to 50 s
        if self.round_t > red_at && self.cond < RED { self.set_cond(RED, Why::TooLong); }
        else if self.round_t > yellow_at && self.cond_min < YELLOW { self.cond_min = YELLOW; self.set_cond(YELLOW, Why::Noticed); }
        if self.cond == RED { return; }
        // otherwise Yellow is situational: a base that has noticed you, ships near you, a squadron or the spy
        let pp = self.p.pos;
        let threat = self.cond_min == YELLOW
            || self.stations.iter().any(|s| s.alive && s.alerted > 0)
            || self.fighters.iter().any(|f| !f.dead && near(f.pos, pp, 286))
            || !self.squads.is_empty()
            || self.spy.is_some();
        self.cond = if threat { YELLOW } else { GREEN };
    }

    fn update_spy(&mut self) {
        let pp = self.p.pos;
        let Some(s) = self.spy.as_mut() else {
            if self.p.alive && self.clear_t == 0 {
                self.spy_t -= 1;
                if self.spy_t <= 0 {
                    let a = rndn(num!(0), num!(1));
                    self.spy = Some(Spy {
                        pos: wrapv(pp + dir(a) * 286), a: (a + num!(0.5) + rndn(num!(-0.095), num!(0.095))).rem_euclid(N::new(1)),
                        sp: num!(2.3) + N::new(self.round) * num!(0.07), passes: 0, far: 0,
                        orbit: if rnd(0, 2) == 0 { num!(1) } else { num!(-1) }, wob: rndn(num!(0), num!(1)),
                    });
                    self.banner = Some((Banner::SpySighted, 130));
                    self.events.push(Ev::SpySighted);
                }
            }
            return;
        };
        // glide past at a distance, then swing round for another look
        let off = if near(s.pos, pp, 165) { num!(0.215) } else { num!(0.0875) };
        s.wob = (s.wob + num!(0.008)).rem_euclid(N::new(1));
        let want = ang_to(s.pos, pp) + s.orbit * off + s.wob.sin() * num!(0.032);
        s.a = turn_toward(s.a, want.rem_euclid(N::new(1)), num!(0.0042));
        s.pos = wrapv(s.pos + dir(s.a) * s.sp);
        let mut escaped = false;
        if !near(s.pos, pp, 308) {
            s.far += 1;
            if s.far > 72 {
                s.far = 0;
                s.passes += 1;
                if s.passes > 2 { escaped = true; } else { s.a = ang_to(s.pos, pp); s.orbit = -s.orbit; }
            }
        } else { s.far = 0; }
        let crash = self.p.alive && self.p.inv <= 0 && near(s.pos, pp, 9);
        if escaped {
            // it got away with your position
            self.spy = None;
            self.spy_t = rnd(1200, 1920);
            self.spy_streak = 0;
            self.set_cond(RED, Why::Reported);
        } else if crash { self.kill_player(); }
    }

    fn kill_spy(&mut self) {
        let Some(s) = self.spy.take() else { return };
        let v = [200, 400, 800][self.spy_streak.min(2)];
        self.spy_streak += 1;
        self.score += v;
        self.booms.push(Boom { pos: s.pos, t: 0 });
        self.booms.push(Boom { pos: s.pos + vec2(N::new(6), N::new(4)), t: -6 });
        self.banner = Some((Banner::SpyDown(v), 110));
        self.events.push(Ev::Spy);
        self.spy_t = rnd(1080, 1800);
    }

    /// a five-ship V launched from the nearest base that isn't right on top of you
    fn launch_squad(&mut self) {
        let pp = self.p.pos;
        let mut bases: Vec<V> = self.stations.iter().filter(|s| s.alive).map(|s| s.pos).collect();
        if bases.is_empty() { return; }
        bases.sort_by_key(|&b| dist2(b, pp));
        let base = bases.iter().copied().find(|&b| !near(b, pp, 165)).unwrap_or(bases[0]);
        let pool: &[Kind] = match self.round {
            1 => &[Kind::I],
            2 => &[Kind::I, Kind::I, Kind::P],
            3 | 4 => &[Kind::I, Kind::P, Kind::P, Kind::E],
            _ => &[Kind::I, Kind::P, Kind::E, Kind::E],
        };
        let kind = pool[rnd(0, pool.len() as i32) as usize];
        let a = ang_to(base, pp);
        let sp = stats(kind).sp + N::new(self.round) * num!(0.055) + if self.cond == RED { num!(0.23) } else { num!(0) };
        let id = self.next_squad;
        self.next_squad += 1;
        self.fighters.push(Fighter { pos: base, a, sp, kind, leader: true, squad: id, slot: 0, mode: Mode::Hunt, wob: rndn(num!(0), num!(1)), dead: false });
        for (k, &off) in SLOTS.iter().enumerate() {
            self.fighters.push(Fighter { pos: wrapv(base + rotate(off, a)), a, sp, kind, leader: false, squad: id, slot: k, mode: Mode::Form, wob: num!(0), dead: false });
        }
        self.squads.push(Squad { id, t: 0, done: false });
        self.banner = Some((Banner::BattleStations(kind), 130));
        self.events.push(Ev::Alert);
    }

    fn update_fighters(&mut self) {
        let pp = self.p.pos;
        let (alive, pa) = (self.p.alive, self.p.a);
        let round = self.round;

        // spawning: lone fighters arrive from off screen, squadrons from a base
        if alive && self.clear_t == 0 {
            // Condition Red: fighters pour in from every direction until the sector is cleared
            let red = self.cond == RED;
            self.spawn_t -= 1;
            let cap = if red { 26 } else { (3 + round).min(10) as usize };
            if self.spawn_t <= 0 && self.fighters.iter().filter(|f| f.squad == 0).count() < cap {
                self.spawn_t = if red { rnd(30, 66) } else { (270 - round * 15).max(84) + rnd(0, 90) };
                let a = rndn(num!(0), num!(1));
                let pos = wrapv(pp + dir(a) * rnd(200, 260)); // just off-screen
                let sp = num!(1.6) + N::new(round) * num!(0.08) + rndn(num!(-0.09), num!(0.09)) + if red { num!(0.32) } else { num!(0) };
                self.fighters.push(Fighter { pos, a: (a + num!(0.5)).rem_euclid(N::new(1)), sp, kind: Kind::I, leader: false, squad: 0, slot: 0, mode: Mode::Hunt, wob: rndn(num!(0), num!(1)), dead: false });
            }
            self.squad_t -= 1;
            let flying = self.squads.iter().filter(|s| !s.done).count();
            if self.squad_t <= 0 && flying < if red { 3 } else { 1 } {
                self.squad_t = if red { rnd(360, 540) } else { (1200 - round * 60).max(540) + rnd(0, 360) };
                self.launch_squad();
            }
        }

        // leaders this frame, so wingmen can find their slots
        let leaders: Vec<(u32, V, N)> = self.fighters.iter().filter(|f| f.leader && !f.dead).map(|f| (f.squad, f.pos, f.a)).collect();
        let vulnerable = alive && self.p.inv <= 0;
        let mut hits: Vec<(usize, bool)> = Vec::new(); // (fighter, also kills the player)
        for (i, f) in self.fighters.iter_mut().enumerate() {
            if f.dead { continue; }
            let st = stats(f.kind);
            match f.mode {
                Mode::Flee => {
                    f.a = turn_toward(f.a, ang_to(pp, f.pos), num!(0.0032));
                    f.pos = wrapv(f.pos + dir(f.a) * f.sp);
                    if !near(f.pos, pp, 605) { f.dead = true; } // got away
                    continue;
                }
                Mode::Form => {
                    if let Some(&(_, lp, la)) = leaders.iter().find(|l| l.0 == f.squad) {
                        let target = wrapv(lp + rotate(SLOTS[f.slot], la));
                        let d = wdv(target, f.pos);
                        let len = d.fast_magnitude();
                        let step = if len / 10 < f.sp * num!(1.4) { len / 10 } else { f.sp * num!(1.4) };
                        if len > num!(0.5) { f.pos = wrapv(f.pos + d / len * step); }
                        f.a = la;
                    } else { f.mode = Mode::Hunt; }
                }
                Mode::Hunt => {
                    f.wob = (f.wob + num!(0.0053)).rem_euclid(N::new(1));
                    let target = if !alive { f.a + num!(0.064) } else {
                        match f.kind {
                            Kind::P => {
                                // boomerang: come in from the side, never straight down your gun line
                                let side = if ang_diff(pa, ang_to(pp, f.pos)) >= num!(0) { num!(0.25) } else { num!(-0.25) };
                                let flank = wrapv(pp + dir(pa + side) * 82);
                                if near(f.pos, pp, 94) { ang_to(f.pos, pp) } else { ang_to(f.pos, flank) }
                            }
                            Kind::E => ang_to(f.pos, pp),
                            Kind::I => ang_to(f.pos, pp) + f.wob.sin() * num!(0.0557),
                        }
                    };
                    f.a = turn_toward(f.a, target.rem_euclid(N::new(1)), st.turn);
                    f.pos = wrapv(f.pos + dir(f.a) * f.sp);
                }
            }
            if vulnerable && near(f.pos, pp, 10) { hits.push((i, true)); continue; }
            if f.squad == 0 && !near(f.pos, pp, 600) { f.dead = true; continue; } // lost you
            if self.rocks.iter().any(|r| near(r.pos, f.pos, if r.big { 22 } else { 12 })) { hits.push((i, false)); }
        }
        let mut crash = false;
        for (i, player) in hits { self.kill_fighter(i, false); crash |= player; }
        if crash { self.kill_player(); }

        // a squadron that loses you is recalled; one with nobody left is finished
        for s in self.squads.iter_mut() { s.t += 1; }
        for k in 0..self.squads.len() {
            let id = self.squads[k].id;
            let lead = self.fighters.iter().find(|f| f.squad == id && f.leader && !f.dead).map(|f| f.pos);
            if let Some(lp) = lead && self.squads[k].t > 1200 && !near(lp, pp, 825) {
                for f in self.fighters.iter_mut().filter(|f| f.squad == id) { f.dead = true; }
            }
            if self.fighters.iter().all(|f| f.squad != id || f.dead) { self.squads[k].done = true; }
        }
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
    // 0 clear, 1 field, 2 frame, 3 station, 4 you, 5 P-type, 6 I-type, 7 E-type, 8 squadron leader, 9 missile, 10 spy
    BLACK, rgb(10, 14, 34), rgb(80, 74, 130), rgb(255, 93, 143), rgb(255, 255, 255), rgb(98, 227, 255),
    rgb(255, 138, 208), rgb(255, 93, 93), rgb(255, 45, 85), rgb(255, 160, 120), rgb(255, 240, 74), BLACK, BLACK, BLACK, BLACK, BLACK,
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
    for m in &g.miss { plot(&mut s, m.pos, 9, false); }
    for f in g.fighters.iter().filter(|f| !f.dead) { plot(&mut s, f.pos, if f.leader { 8 } else { stats(f.kind).radar }, f.leader); }
    if let Some(sp) = &g.spy && (g.frame / 8) % 2 == 0 { plot(&mut s, sp.pos, 10, true); }
    plot(&mut s, g.p.pos, 4, true);
    s.to_vram(palette.clone())
}

/// Everything in the sector plus the HUD. World sprites sit below the text layer, the HUD above it.
fn draw_world(g: &Game, frame: &mut GraphicsFrame, matrices: &[AffineMatrixObject], radar: &SpriteVram) {
    let mut budget = 124usize; // OAM holds 128 objects
    // objects shown first draw on top, so: HUD, effects, ships, stations, rocks
    macro_rules! show {
        ($obj:expr) => { if budget > 0 { budget -= 1; $obj.show(frame); } };
    }
    macro_rules! world {
        ($obj:expr) => { $obj.set_priority(Priority::P1); show!($obj); };
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
        world!(o);
    }
    if g.p.alive && (g.p.inv == 0 || (g.frame / 4) % 2 == 0) {
        let s = g.to_screen(g.p.pos);
        let mut o = ObjectAffine::new(sprites::SHIPS.sprite(0), matrices[facing(g.p.a)].clone(), AffineMode::AffineDouble);
        o.set_pos((s.0 - 32, s.1 - 32));
        world!(o);
    }
    for b in &g.bullets {
        let s = g.to_screen(b.pos);
        if !on_screen(s, 8) { continue; }
        let mut o = ObjectAffine::new(sprites::BOLT.sprite(0), matrices[b.f].clone(), AffineMode::Affine);
        o.set_pos((s.0 - 8, s.1 - 8));
        world!(o);
    }
    for f in g.fighters.iter().filter(|f| !f.dead) {
        let s = g.to_screen(f.pos);
        if !on_screen(s, 16) { continue; }
        let st = stats(f.kind);
        let mut o = ObjectAffine::new(sprites::SHIPS.sprite(if f.leader { st.lead_frame } else { st.frame }), matrices[facing(f.a)].clone(), AffineMode::AffineDouble);
        o.set_pos((s.0 - 32, s.1 - 32));
        world!(o);
    }
    if let Some(sp) = &g.spy {
        let s = g.to_screen(sp.pos);
        if on_screen(s, 16) {
            let mut o = ObjectAffine::new(sprites::SHIPS.sprite(8), matrices[facing(sp.a)].clone(), AffineMode::AffineDouble);
            o.set_pos((s.0 - 32, s.1 - 32));
            world!(o);
        }
    }
    for m in &g.miss {
        let s = g.to_screen(m.pos);
        if !on_screen(s, 8) { continue; }
        let mut o = ObjectAffine::new(sprites::MISSILE.sprite(0), matrices[facing(m.a)].clone(), AffineMode::Affine);
        o.set_pos((s.0 - 8, s.1 - 8));
        world!(o);
    }
    for st in g.stations.iter().filter(|s| s.alive) {
        let s = g.to_screen(st.pos);
        if !on_screen(s, 48) { continue; }
        for k in 0..st.n {
            if st.pods[k] == 0 || st.pod_flash[k] % 2 == 1 { continue; }
            let off = st.offs[k];
            let mut o = Object::new(sprites::POD.sprite(0));
            o.set_pos((s.0 + off.x.round() - 8, s.1 + off.y.round() - 8));
            world!(o);
        }
        // the core and hull turn with the station, using the same matrices as the ships
        let core = DESIGNS[st.design].core_frame + if st.core_flash > 0 { 1 } else { 0 };
        let mut o = ObjectAffine::new(sprites::CORES.sprite(core), matrices[st.facing].clone(), AffineMode::Affine);
        o.set_pos((s.0 - 8, s.1 - 8));
        world!(o);
        let mut o = ObjectAffine::new(sprites::HULLS.sprite(st.design), matrices[st.facing].clone(), AffineMode::AffineDouble);
        o.set_pos((s.0 - 64, s.1 - 64));
        world!(o);
    }
    for rk in &g.rocks {
        let s = g.to_screen(rk.pos);
        let (spr, h) = if rk.big { (sprites::ROCK_BIG.sprite(0), 32) } else { (sprites::ROCK_SMALL.sprite(0), 16) };
        if !on_screen(s, h) { continue; }
        let mut o = Object::new(spr);
        o.set_pos((s.0 - h, s.1 - h));
        world!(o);
    }
}

// ---------- screens ----------
#[derive(Clone, Copy)]
enum Screen {
    Title { sel: usize },
    Board { highlight: Option<usize> },
    Credits,
    Play,
    Paused { sel: usize },
    GameOver { t: i32 },
    /// arcade-style callsign entry for a new top-ten score
    Entry { name: [u8; 3], pos: usize },
}

/// letters a callsign can use, cycled with Up/Down
const CALLSIGN: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-";

fn cycle(c: u8, step: i32) -> u8 {
    let i = CALLSIGN.iter().position(|&x| x == c).unwrap_or(0) as i32;
    CALLSIGN[(i + step).rem_euclid(CALLSIGN.len() as i32) as usize]
}
fn name_str(n: &[u8; 3]) -> &str { core::str::from_utf8(n).unwrap_or("???") }

/// a menu with a blinking cursor; one item per row in `rows`
fn menu(text: &mut ui::Text, x: i32, rows: &[i32], items: &[&str], sel: usize, t: u32) {
    for k in 0..items.len() {
        let on = k == sel;
        text.put(x - 2, rows[k], if on && (t / 16) % 2 == 0 { ">" } else { " " }, Ink::Amber);
        text.put(x, rows[k], items[k], if on { Ink::Amber } else { Ink::White });
    }
}

/// the parts of each screen that don't change while it's showing
fn draw_static(text: &mut ui::Text, screen: Screen, g: &Game, board: &scores::Board) {
    text.clear();
    match screen {
        Screen::Title { .. } => {
            text.logo(3);
            text.centre(7, "SECTOR PATROL", Ink::Dim);
            text.centre(16, &format!("HI {:07}", board.best()), Ink::Dim);
            text.centre(18, "A CONFUSEDGAMEDEV GAME", Ink::Dim);
        }
        Screen::Board { highlight } => {
            text.centre(1, "TOP PILOTS", Ink::Rose);
            if board.entries.is_empty() {
                text.centre(8, "NO PATROLS LOGGED YET", Ink::Dim);
                text.centre(10, "BE THE FIRST", Ink::Dim);
            } else {
                text.put(3, 3, " #  PILOT RND    SCORE", Ink::Dim);
                for (i, e) in board.entries.iter().enumerate() {
                    let row = format!("{:>2}  {}   {:>3}  {:>7}", i + 1, name_str(&e.name), e.round, e.score);
                    text.put(3, 5 + i as i32, &row, if highlight == Some(i) { Ink::Amber } else { Ink::White });
                }
            }
            text.centre(17, "PRESS A", Ink::Dim);
        }
        Screen::Credits => {
            text.centre(1, "CREDITS", Ink::Rose);
            text.centre(5, "ENGINEERING", Ink::Dim);
            text.centre(7, "JORGE PEDRERO", Ink::White);
            text.centre(10, "ART", Ink::Dim);
            text.centre(12, "MATT WALKDEN", Ink::White);
            text.centre(17, "PRESS A", Ink::Dim);
        }
        Screen::Paused { .. } => {
            text.centre(6, "PAUSED", Ink::Amber);
            text.centre(15, "START: RESUME", Ink::Dim);
        }
        Screen::GameOver { .. } => {
            text.centre(7, "GAME OVER", Ink::Rose);
            text.centre(9, &format!("SCORE {:07}", g.score), Ink::White);
            text.centre(11, &format!("REACHED ROUND {}", g.round), Ink::Dim);
        }
        Screen::Entry { .. } => {
            text.centre(3, "NEW RECORD!", Ink::Amber);
            text.centre(5, &format!("SCORE {:07}", g.score), Ink::White);
            text.centre(8, "ENTER YOUR CALLSIGN", Ink::Dim);
            text.centre(15, "UP/DOWN: LETTER  A: NEXT", Ink::Dim);
            text.centre(17, "B: BACK", Ink::Dim);
        }
        Screen::Play => {}
    }
}

#[agb::entry]
fn main(mut gba: agb::Gba) -> ! {
    let mut gfx = gba.graphics.get();
    let mut input = ButtonController::new();
    let mut audio = sound::Audio::new(&mut gba.mixer);
    let mut store = scores::Store::open(&mut gba.save);
    let mut board = store.load();

    gfx.set_background_palettes(backdrop::PALETTES);
    let mut far = RegularBackground::new(Priority::P3, RegularBackgroundSize::Background32x32, TileFormat::FourBpp);
    far.fill_with(&backdrop::FAR);
    let mut near_bg = RegularBackground::new(Priority::P2, RegularBackgroundSize::Background32x32, TileFormat::FourBpp);
    near_bg.fill_with(&backdrop::NEAR);
    let mut text = ui::Text::new(&backdrop::FONT, &backdrop::LOGO);

    // one rotation matrix per facing, shared by every rotated sprite (the hardware allows 32)
    let matrices: Vec<AffineMatrixObject> = (0..FACINGS)
        .map(|k| AffineMatrixObject::new(AffineMatrix::from_rotation(-N::new(k as i32) / FACINGS as i32)))
        .collect();

    let radar_palette = PaletteVramSingle::try_allocate_new(&RADAR_PALETTE).expect("palette space for the radar");
    let mut g = Game::new();
    let mut radar = draw_radar(&g, &radar_palette);
    let mut screen = Screen::Title { sel: 0 };
    let mut redraw = true;
    let mut shown_banner: Option<Banner> = None;
    let mut shown_cond: Option<(u8, bool)> = None; // condition readout under the radar, and its blink
    let mut demo = vec2(N::new(0), N::new(0)); // starfield drift behind the menus
    let mut t: u32 = 0;

    loop {
        input.update();
        t = t.wrapping_add(1);
        let pressed = |b: Button| input.is_just_pressed(b);
        let ok = pressed(Button::A) || pressed(Button::Start);
        let ud = input.just_pressed_y_tri() as i32;
        let lr = input.just_pressed_x_tri() as i32;
        let mut next = None;

        match &mut screen {
            Screen::Title { sel } => {
                if ud != 0 { *sel = (*sel as i32 + ud).rem_euclid(3) as usize; audio.play(Ev::Move); }
                if ok {
                    audio.play(Ev::Select);
                    next = Some(match *sel {
                        0 => { g = Game::new(); Screen::Play }
                        1 => Screen::Board { highlight: None },
                        _ => Screen::Credits,
                    });
                }
            }
            Screen::Board { .. } => {
                if ok || pressed(Button::B) { audio.play(Ev::Select); next = Some(Screen::Title { sel: 1 }); }
            }
            Screen::Credits => {
                if ok || pressed(Button::B) { audio.play(Ev::Select); next = Some(Screen::Title { sel: 2 }); }
            }
            Screen::Play => {
                if pressed(Button::Start) { audio.play(Ev::Move); next = Some(Screen::Paused { sel: 0 }); }
                else {
                    g.update(&input);
                    if g.over { next = Some(Screen::GameOver { t: 0 }); }
                }
            }
            Screen::Paused { sel } => {
                if ud != 0 { *sel ^= 1; audio.play(Ev::Move); }
                if pressed(Button::Start) || pressed(Button::B) || (pressed(Button::A) && *sel == 0) { audio.play(Ev::Select); next = Some(Screen::Play); }
                else if pressed(Button::A) { audio.play(Ev::Select); next = Some(Screen::Title { sel: 0 }); }
            }
            Screen::GameOver { t: gt } => {
                *gt += 1;
                if *gt > 180 || (*gt > 45 && ok) {
                    next = Some(if board.qualifies(g.score) {
                        Screen::Entry { name: board.last_name.unwrap_or(*b"AAA"), pos: 0 }
                    } else {
                        Screen::Board { highlight: None }
                    });
                }
            }
            Screen::Entry { name, pos } => {
                if ud != 0 { name[*pos] = cycle(name[*pos], -ud); audio.play(Ev::Move); } // up goes A -> B
                if lr > 0 && *pos < 2 { *pos += 1; audio.play(Ev::Move); }
                if (lr < 0 || pressed(Button::B)) && *pos > 0 { *pos -= 1; audio.play(Ev::Move); }
                if ok {
                    audio.play(Ev::Select);
                    if *pos < 2 { *pos += 1; } else {
                        let place = board.insert(scores::Entry { name: *name, score: g.score, round: g.round.clamp(0, 255) as u8 });
                        store.save(&board);
                        next = Some(Screen::Board { highlight: Some(place) });
                    }
                }
            }
        }
        if let Some(s) = next { screen = s; redraw = true; }
        for ev in g.events.drain(..) { audio.play(ev); }

        // the menus get the calm loop; in play the band drops out while you respawn and gets busier
        // while a station has you in range
        let playing = matches!(screen, Screen::Play);
        let in_menus = matches!(screen, Screen::Title { .. } | Screen::Board { .. } | Screen::Credits | Screen::Entry { .. });
        audio.alive = in_menus || (playing && g.p.alive);
        audio.tense = playing && g.p.alive && g.cond >= YELLOW;
        audio.red = playing && g.p.alive && g.cond == RED;

        // ---- text ----
        if redraw { draw_static(&mut text, screen, &g, &board); redraw = false; shown_banner = None; shown_cond = None; }
        match screen {
            Screen::Title { sel } => menu(&mut text, 11, &[10, 12, 14], &["LAUNCH", "TOP PILOTS", "CREDITS"], sel, t),
            Screen::Paused { sel } => menu(&mut text, 10, &[9, 11], &["RESUME", "QUIT TO TITLE"], sel, t),
            Screen::Entry { name, pos } => {
                for k in 0..3 {
                    let blink = k == pos && (t / 12) % 2 == 1;
                    let ch = [name[k]];
                    text.put(13 + 2 * k as i32, 11, if blink { " " } else { name_str_1(&ch) }, if k == pos { Ink::Amber } else { Ink::White });
                    text.put(13 + 2 * k as i32, 12, if k == pos { "_" } else { " " }, Ink::Amber);
                }
            }
            Screen::Play => {
                let now = g.banner.map(|b| b.0);
                if now != shown_banner {
                    text.clear_row(7);
                    text.clear_row(8);
                    match now {
                        Some(Banner::Round(n)) => text.centre(7, &format!("ROUND {}", n), Ink::Amber),
                        Some(Banner::Clear) => text.centre(7, "SECTOR CLEAR", Ink::Amber),
                        Some(Banner::BattleStations(k)) => {
                            text.centre(7, "BATTLE STATIONS", Ink::Rose);
                            text.centre(8, &format!("{}-TYPE SQUADRON INBOUND", stats(k).name), Ink::White);
                        }
                        Some(Banner::Broken) => text.centre(7, "FORMATION BROKEN", Ink::Amber),
                        Some(Banner::Squadron(b)) => text.centre(7, &format!("SQUADRON +{}", b), Ink::Amber),
                        Some(Banner::Yellow(_)) => {
                            text.centre(7, "CONDITION YELLOW", Ink::Amber);
                            text.centre(8, "THE SECTOR KNOWS YOU'RE HERE", Ink::White);
                        }
                        Some(Banner::Red(why)) => {
                            text.centre(7, "CONDITION RED", Ink::Rose);
                            text.centre(8, if why == Why::Reported { "THE SPY SHIP REPORTED IN" } else { "YOU STAYED TOO LONG" }, Ink::White);
                        }
                        Some(Banner::SpySighted) => {
                            text.centre(7, "SPY SHIP SIGHTED", Ink::Amber);
                            text.centre(8, "SHOOT IT BEFORE IT REPORTS", Ink::White);
                        }
                        Some(Banner::SpyDown(v)) => text.centre(7, &format!("SPY SHIP DOWN +{}", v), Ink::Amber),
                        None => {}
                    }
                    shown_banner = now;
                }
                // CONDITION GREEN / YELLOW / RED under the radar; red blinks
                let blink = g.cond == RED && (t / 8) % 2 == 1;
                if shown_cond != Some((g.cond, blink)) {
                    let (label, ink) = match g.cond { RED => ("CONDITION RED", if blink { Ink::Dim } else { Ink::Rose }), YELLOW => ("CONDITION YELLOW", Ink::Amber), _ => ("CONDITION GREEN", Ink::Green) };
                    text.clear_row(5);
                    text.put(ui::COLS - label.len() as i32, 5, label, ink);
                    shown_cond = Some((g.cond, blink));
                }
            }
            _ => {}
        }

        // Condition Red: the space behind everything throbs dark red
        let throb = if matches!(screen, Screen::Play) && g.cond == RED {
            let pulse = (N::new((t % 64) as i32) / 64).sin() + num!(1); // 0..2
            (pulse * 36).floor() as u16 + 20
        } else { 0 };
        gfx.set_background_palette_colour(0, 0, rgb(throb, 16u16.saturating_sub(throb / 4), 32u16.saturating_sub(throb / 6)));

        // ---- picture ----
        let with_world = matches!(screen, Screen::Play | Screen::Paused { .. } | Screen::GameOver { .. });
        if with_world && g.frame % 4 == 0 { radar = draw_radar(&g, &radar_palette); }
        let mut frame = gfx.frame();
        let cam = if with_world { g.cam } else { demo += vec2(num!(0.6), num!(0.25)); demo };
        if with_world { draw_world(&g, &mut frame, &matrices, &radar); }
        text.show(&mut frame);
        // parallax: both star layers move slower than the world, the far one slowest
        far.set_scroll_pos((cam.x.floor() / 6, cam.y.floor() / 6));
        near_bg.set_scroll_pos((cam.x.floor() / 2, cam.y.floor() / 2));
        let far_id = far.show(&mut frame);
        let near_id = near_bg.show(&mut frame);
        // behind the pause menu and Game Over, darken everything except the text so it reads clearly
        if matches!(screen, Screen::Paused { .. } | Screen::GameOver { .. }) {
            frame.blend().darken(num!(0.5)).enable_background(far_id).enable_background(near_id).enable_object().enable_backdrop();
        }

        audio.frame();
        frame.commit();
    }
}

fn name_str_1(c: &[u8; 1]) -> &str { core::str::from_utf8(c).unwrap_or(" ") }
