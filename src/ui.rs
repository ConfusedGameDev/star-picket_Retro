//! Text on a background layer: menus, the leaderboard, banners. Using a background instead of sprites
//! means text never competes with the game for the GBA's 128 sprites.
use agb::display::{
    GraphicsFrame, Priority,
    tile_data::TileData,
    tiled::{RegularBackground, RegularBackgroundSize, TileFormat, TileSetting},
};

/// Must match GLYPHS in tools/make_gfx.py: the font image has one 8x8 cell per glyph, one row per ink.
const GLYPHS: &str = " ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789.-:!?/><#'()_";
pub const COLS: i32 = 30; // visible tiles across
pub const ROWS: i32 = 20; // and down

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Ink { White = 0, Amber = 1, Dim = 2, Rose = 3 }

pub struct Text {
    bg: RegularBackground,
    font: &'static TileData,
    logo: &'static TileData,
}

impl Text {
    pub fn new(font: &'static TileData, logo: &'static TileData) -> Self {
        Text { bg: RegularBackground::new(Priority::P0, RegularBackgroundSize::Background32x32, TileFormat::FourBpp), font, logo }
    }

    pub fn clear(&mut self) {
        for y in 0..ROWS { for x in 0..COLS { self.bg.set_tile((x, y), &self.font.tiles, TileSetting::BLANK); } }
    }

    pub fn clear_row(&mut self, y: i32) {
        for x in 0..COLS { self.bg.set_tile((x, y), &self.font.tiles, TileSetting::BLANK); }
    }

    /// Unknown characters print as blanks. Lowercase is shown as uppercase.
    pub fn put(&mut self, x: i32, y: i32, s: &str, ink: Ink) {
        for (i, c) in s.chars().enumerate() {
            let idx = GLYPHS.find(c.to_ascii_uppercase()).unwrap_or(0);
            let tile = if idx == 0 { TileSetting::BLANK } else { self.font.tile_settings[ink as usize * GLYPHS.len() + idx] };
            self.bg.set_tile((x + i as i32, y), &self.font.tiles, tile);
        }
    }

    pub fn centre(&mut self, y: i32, s: &str, ink: Ink) {
        self.put((COLS - s.len() as i32) / 2, y, s, ink);
    }

    /// the title logo, centred, its top on row `y`
    pub fn logo(&mut self, y: i32) {
        let (w, h) = (self.logo.width as i32, self.logo.height as i32);
        let x0 = (COLS - w) / 2;
        for ty in 0..h {
            for tx in 0..w {
                let setting = self.logo.tile_settings[(ty * w + tx) as usize];
                self.bg.set_tile((x0 + tx, y + ty), &self.logo.tiles, setting);
            }
        }
    }

    pub fn show(&self, frame: &mut GraphicsFrame) { self.bg.show(frame); }
}
