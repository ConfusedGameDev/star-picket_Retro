//! Music and sound, ported from the web version's "your kills play the music" system.
//!
//! A 124 BPM loop runs over Am · F · C · G (two bars each). Every kill is a note: it waits for the
//! next 16th, takes its pitch from the chord playing at that moment, and a run of kills climbs the
//! chord like a melody. Kills that land together spill into the following 16ths as an arpeggio.
//! The drums and bass get busier while a station is shooting at you, and drop out while you respawn.
//!
//! Pitched instruments are single samples (sfx/, made by tools/make_sfx.py) re-pitched with the
//! mixer's playback speed. A dotted-eighth echo is faked by replaying the note quieter 3 steps later.
use agb::{
    fixnum::{Num, num},
    include_wav,
    sound::mixer::{Frequency, Mixer, MixerController, SoundChannel, SoundData},
};
use alloc::vec::Vec;

static PLUCK: SoundData = include_wav!("sfx/pluck.wav");
static BLIP: SoundData = include_wav!("sfx/blip.wav");
static BELL: SoundData = include_wav!("sfx/bell.wav");
static BASS: SoundData = include_wav!("sfx/bass.wav");
static KICK: SoundData = include_wav!("sfx/kick.wav");
static HAT: SoundData = include_wav!("sfx/hat.wav");
static CLAP: SoundData = include_wav!("sfx/clap.wav");
static BOOM: SoundData = include_wav!("sfx/boom.wav");
static BIG: SoundData = include_wav!("sfx/big.wav");
static DIE: SoundData = include_wav!("sfx/die.wav");

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ev { Die, Station, Pod, Hit, Missile, Rock, Launch }

/// MIDI notes of each chord: Am, F, C, G
const PROG: [[i32; 3]; 4] = [[57, 60, 64], [53, 57, 60], [48, 52, 55], [55, 59, 62]];
/// frames per 16th note at 124 BPM and 60 fps
const STEP_FRAMES: Num<i32, 8> = num!(7.258);
/// 2^(n/12) for n = 0..11, in 8-bit fixed point
const SEMI: [u32; 12] = [256, 271, 287, 304, 323, 342, 362, 384, 406, 431, 456, 483];
const A4: i32 = 69; // pitch the pluck, blip and bell samples were made at
const A2: i32 = 45; // pitch of the bass sample
const UNPITCHED: i32 = -1;

fn rate(m: i32, base: i32) -> Num<u32, 8> {
    let s = m - base;
    let (oct, r) = (s.div_euclid(12), SEMI[s.rem_euclid(12) as usize]);
    Num::from_raw(if oct >= 0 { r << oct } else { r >> (-oct) })
}
fn vol(x: i16) -> Num<i16, 8> { Num::from_raw(x) } // 256 = full volume
fn chord(step: u32) -> [i32; 3] { PROG[(step / 32) as usize % PROG.len()] }
/// the chord's tones over three octaves, low to high: what a run of kills climbs
fn ladder(step: u32) -> [i32; 9] {
    let c = chord(step);
    let mut l = [0; 9];
    for o in 0..3 { for k in 0..3 { l[o * 3 + k] = c[k] + 12 * (o as i32 + 1); } }
    l
}
fn pick3() -> usize { (agb::rng::next_i32().unsigned_abs() % 3) as usize }

#[derive(Clone, Copy)]
struct Note { step: u32, data: SoundData, base: i32, m: i32, vol: i16 }

pub struct Audio<'gba> {
    mixer: Mixer<'gba>,
    clock: Num<i32, 8>,
    step: u32, // the next 16th to be played
    slot: u32, // the last 16th a kill note was booked into
    queue: Vec<Note>,
    pos: i32, // place on the ladder
    dir: i32,
    last_kill: u32,
    frame: u32,
    /// set by the game each frame
    pub alive: bool,
    pub tense: bool,
}

impl<'gba> Audio<'gba> {
    pub fn new(controller: &'gba mut MixerController) -> Self {
        Audio {
            mixer: controller.mixer(Frequency::Hz18157),
            clock: num!(0), step: 0, slot: 0, queue: Vec::new(), pos: 0, dir: 1, last_kill: 0, frame: 0,
            alive: true, tense: false,
        }
    }

    fn now(&mut self, data: SoundData, base: i32, m: i32, v: i16, high: bool) {
        let mut ch = if high { SoundChannel::new_high_priority(data) } else { SoundChannel::new(data) };
        if base != UNPITCHED { ch.playback(rate(m, base)); }
        ch.volume(vol(v));
        let _ = self.mixer.play_sound(ch); // a full mixer just drops the sound
    }

    fn at(&mut self, step: u32, data: SoundData, base: i32, m: i32, v: i16) {
        self.queue.push(Note { step, data, base, m, vol: v });
    }

    /// a note plus its echo three 16ths (a dotted eighth) later
    fn with_echo(&mut self, step: u32, data: SoundData, base: i32, m: i32, v: i16) {
        self.at(step, data, base, m, v);
        self.at(step + 3, data, base, m, v / 3);
    }

    /// the next free 16th; crowded kills queue behind each other, up to half a bar ahead
    fn book(&mut self) -> Option<u32> {
        let n = self.step.max(self.slot + 1);
        if n > self.step + 8 { return None; }
        self.slot = n;
        Some(n)
    }

    fn melody(&mut self, step: u32) -> (i32, [i32; 9]) {
        let l = ladder(step);
        if self.frame.wrapping_sub(self.last_kill) > 96 {
            self.pos = pick3() as i32; // a fresh run starts low
            self.dir = 1;
        } else {
            self.pos += self.dir;
            if self.pos >= 8 || self.pos <= 0 { self.dir = -self.dir; }
        }
        self.pos = self.pos.clamp(0, 8);
        self.last_kill = self.frame;
        (l[self.pos as usize], l)
    }

    pub fn play(&mut self, ev: Ev) {
        let ch = chord(self.step);
        match ev {
            // ungridded, but still in key
            Ev::Hit => self.now(BLIP, A4, ch[pick3()] + 24, 50, false),
            Ev::Launch => self.now(BLIP, A4, ch[0], 40, false),
            Ev::Die => self.now(DIE, UNPITCHED, 0, 230, true),
            _ => {
                // an instant crunch so the hit feels immediate, then the note on the grid
                let (crunch, cv) = match ev { Ev::Station => (BIG, 230), Ev::Pod => (BOOM, 110), _ => (BOOM, 70) };
                self.now(crunch, UNPITCHED, 0, cv, false);
                let Some(n) = self.book() else { return };
                let (m, l) = self.melody(n);
                match ev {
                    Ev::Rock => self.with_echo(n, PLUCK, A4, m + 12, 90),
                    Ev::Missile => self.at(n, PLUCK, A4, m + 12, 70),
                    Ev::Pod => self.with_echo(n, BELL, A4, m - 12, 200),
                    Ev::Station => {
                        // a downbeat of its own: kick and low root, the whole chord, then a run up the ladder
                        let c = chord(n);
                        self.at(n, KICK, UNPITCHED, 0, 255);
                        self.at(n, BASS, A2, c[0] - 24, 230);
                        for t in c { self.at(n, PLUCK, A4, t + 12, 110); }
                        for (i, &t) in l.iter().take(6).enumerate() { self.at(n + 1 + i as u32, PLUCK, A4, t, 120); }
                        self.slot = n + 6;
                    }
                    _ => {}
                }
            }
        }
    }

    fn play_step(&mut self, n: u32) {
        // booked notes first (they matter most), then the band
        let due: Vec<Note> = self.queue.iter().copied().filter(|q| q.step <= n).collect();
        self.queue.retain(|q| q.step > n);
        for q in due { self.now(q.data, q.base, q.m, q.vol, true); }
        if !self.alive { return; }
        let b = n % 16;
        let root = chord(n)[0] - 24;
        if b % 8 == 0 || (self.tense && b % 4 == 0) { self.now(KICK, UNPITCHED, 0, if b == 0 { 200 } else { 160 }, false); }
        if self.tense && b % 8 == 4 { self.now(CLAP, UNPITCHED, 0, 90, false); }
        let hat = if self.tense { b % 2 == 0 } else { b % 4 == 2 };
        if hat { self.now(HAT, UNPITCHED, 0, 60, false); }
        let bass = if self.tense { b % 2 == 0 } else { matches!(b, 0 | 6 | 8 | 14) };
        if bass { self.now(BASS, A2, root + if self.tense && b % 4 == 2 { 12 } else { 0 }, 140, false); }
    }

    /// call once per frame
    pub fn frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        self.clock += 1;
        while self.clock >= STEP_FRAMES {
            self.clock -= STEP_FRAMES;
            let n = self.step;
            self.play_step(n);
            self.step += 1;
        }
        self.mixer.frame();
    }
}
