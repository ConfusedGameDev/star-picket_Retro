# Star Picket Retro

A Game Boy Advance port of [Star Picket](https://star-picket.vercel.app), the sector-patrol shooter, written in Rust with
[agb](https://crates.io/crates/agb).

**Play it:** load `dist/star-picket.gba` in any GBA emulator ([mGBA](https://mgba.io) recommended) or on real hardware
with a flash cart.

## Controls

| Button | Action |
|---|---|
| D-pad | Steer: the ship turns toward the direction you hold and speeds up |
| (automatic) | Guns fire fore and aft on their own |
| Start | Pause; after Game Over, start again |

## What's in this prototype

- A wrapping sector about 8.5 screens across, with two parallax star layers.
- The player ship and bullets rotated by the GBA's hardware: 32 facings, one shared rotation matrix each.
- Hex stations: shoot out all six cannon pods (200 each) or hit the core three times (1500 for the station).
  Stations launch homing missiles when you are in range. Clear every station to start the next round.
- Drifting asteroids, explosions, score, three lives, and a radar of the whole sector.
- The music system from the web version: a 124 BPM loop over Am · F · C · G, kill notes snapped to the next 16th
  and pitched from the current chord, kill streaks that climb the chord, and a fake dotted-eighth echo. The drums and
  bass get busier while a station has you in range.

Not ported yet: squadrons, the spy ship, Condition Yellow/Red, the five station designs, mines, voice callouts,
title screen, and a saved high-score table. The online leaderboard and share card can't come over: the GBA has
no network.

## Building

You need [rustup](https://rustup.rs). The toolchain is pinned in `rust-toolchain.toml` to a nightly from around agb
0.25's release (newer nightlies currently fail to build `agb_hashmap`).

```sh
cargo install agb-gbafix
cargo build --release
agb-gbafix target/thumbv4t-none-eabi/release/star-picket-retro -o dist/star-picket.gba
```

`cargo run --release` builds and opens the game in `mgba-qt` if it's on your PATH.
`cargo build --release --features showcase` parks a station right in front of the start position, for testing.
`cargo build --release --features soundcheck` plays every pitched instrument at every note the game can use and
logs the result to mGBA's log: run it after changing the sounds or the pitch maths.

## Art and sound

- Sprites: the [Lunar Battle Pack](https://mattwalkden.itch.io/lunar-battle-pack) by Matt Walkden (CC0), in
  `assets/lunar/`. `tools/make_gfx.py` (needs Pillow) cuts it into the GBA-sized sheets in `gfx/`; the station hull,
  core and digits are drawn by that script.
- Sounds: synthesized by `tools/make_sfx.py` (standard library only) into `sfx/`. Pitched instruments are one sample
  each, re-pitched per note with the mixer's playback speed.

Re-run either script after changing it, then rebuild.
