//! Lesson 1: hello, world. Puts text on a background layer and prints to the emulator's log.
//!
//!     cargo build --release --example 01_hello
#![no_std]
#![no_main]

use agb::{
    display::{Priority, tiled::{RegularBackground, RegularBackgroundSize, TileFormat, TileSetting}},
    include_background_gfx,
};

// The font is an image with one 8x8 tile per character. The macro converts it at compile time
// into tile data plus the palettes it needs. "001020" is the colour shown where nothing is drawn.
include_background_gfx!(mod gfx, "001020", FONT => "gfx/font.png");
const GLYPHS: &str = " ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789.-:!?/><#'()_+";

#[agb::entry]
fn main(mut gba: agb::Gba) -> ! {
    agb::println!("Hello from the Game Boy Advance!"); // shows up in mGBA's log window

    let mut gfx = gba.graphics.get();
    gfx.set_background_palettes(gfx::PALETTES);
    let mut bg = RegularBackground::new(Priority::P0, RegularBackgroundSize::Background32x32, TileFormat::FourBpp);

    // Each character is one tile: find its index in the font and place that tile on the map.
    let text = "HELLO WORLD!";
    for (i, c) in text.chars().enumerate() {
        let glyph = GLYPHS.find(c).unwrap_or(0);
        let tile = if glyph == 0 { TileSetting::BLANK } else { gfx::FONT.tile_settings[glyph] };
        bg.set_tile((9 + i as i32, 9), &gfx::FONT.tiles, tile);
    }

    loop {
        // every frame: describe what should be on screen, then commit it at the next vertical blank
        let mut frame = gfx.frame();
        bg.show(&mut frame);
        frame.commit();
    }
}
