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
| Start | Pause (resume, or quit to the title) |
| Menus | D-pad to choose, A or Start to confirm, B to go back |

## What's in this prototype

- Title screen with LAUNCH, TOP PILOTS and CREDITS, over the drifting starfield.
- A local leaderboard of the top ten patrols (callsign, round reached, score), saved in the cartridge's
  battery-backed SRAM so it survives power-off. Make the top ten and you enter a three-letter callsign arcade-style:
  Up/Down changes the letter, A moves on, B goes back. Your last callsign is offered next time.
- Pause menu, Game Over screen, and ROUND / SECTOR CLEAR banners. Text is drawn on a background layer, so it
  costs none of the GBA's 128 sprites; the playfield is darkened in hardware behind the pause and Game Over text.
- A wrapping sector about 8.5 screens across, with two parallax star layers.
- The player ship and bullets rotated by the GBA's hardware: 32 facings, one shared rotation matrix each.
- Hex stations: shoot out all six cannon pods (200 each) or hit the core three times (1500 for the station).
  Stations launch homing missiles when you are in range. Clear every station to start the next round.
- Enemy ships. Lone fighters arrive from off-screen and hunt you. Squadrons of five fly in a V from the nearest
  base ("BATTLE STATIONS"), in three types: I weaves at you, P swings in from your flank like a boomerang, E is a
  fast dart that barely turns. Kill the leader and the wingmen break off and flee; shoot down all five for a bonus
  (500 / 1000 / 1500). Fighters 50 / 60 / 70, leaders 100 / 120 / 140. They're faster relative to you than on
  the web, because the GBA screen shows far less of the sector.
- Drifting asteroids, explosions, score, three lives, and a radar of the whole sector (fighters show in their
  type's colour, leaders larger).
- The music system from the web version: a 124 BPM loop over Am · F · C · G, kill notes snapped to the next 16th
  and pitched from the current chord, kill streaks that climb the chord, and a fake dotted-eighth echo. The drums and
  bass get busier while a station has you in range.

Not ported yet: the spy ship, Condition Yellow/Red, the five station designs, mines, voice callouts,
and the art style toggle. The online leaderboard and share card can't come over: the GBA has no network, so the
leaderboard is local.

## Building

You need [rustup](https://rustup.rs). The toolchain is pinned in `rust-toolchain.toml` to a nightly from around agb
0.25's release (newer nightlies currently fail to build `agb_hashmap`).

```sh
cargo install agb-gbafix
cargo build --release
agb-gbafix target/thumbv4t-none-eabi/release/star-picket-retro -o dist/star-picket.gba
```

`cargo run --release` builds and opens the game in `mgba-qt` if it's on your PATH.
`cargo build --release --features showcase` parks a station right in front of the start position and launches a
squadron within a second, for testing.
`cargo build --release --features quickover` gives you one ship that explodes 3 seconds in, for testing the menus.
`cargo build --release --features soundcheck` plays every pitched instrument at every note the game can use and
logs the result to mGBA's log: run it after changing the sounds or the pitch maths.

## Art and sound

- Sprites: the [Lunar Battle Pack](https://mattwalkden.itch.io/lunar-battle-pack) by Matt Walkden (CC0), in
  `assets/lunar/`. `tools/make_gfx.py` (needs Pillow) cuts it into the GBA-sized sheets in `gfx/`; the station hull,
  core and digits are drawn by that script.
- Sounds: synthesized by `tools/make_sfx.py` (standard library only) into `sfx/`. Pitched instruments are one sample
  each, re-pitched per note with the mixer's playback speed.

Re-run either script after changing it, then rebuild.
