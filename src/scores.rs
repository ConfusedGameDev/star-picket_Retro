//! The local leaderboard: the top ten patrols, kept in the cartridge's battery-backed SRAM.
use agb::save::{SaveManager, SaveSlotManager};
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

pub const MAX_ENTRIES: usize = 10;
/// Identifies our save data; anything else in SRAM is treated as blank and reformatted.
const MAGIC: [u8; 32] = *b"STAR PICKET RETRO / TOP PILOTS 1";

#[derive(Clone, Serialize, Deserialize)]
pub struct Entry {
    pub name: [u8; 3],
    pub score: u32,
    pub round: u8,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Board {
    /// best first
    pub entries: Vec<Entry>,
    /// the callsign entered last time, offered again next time
    pub last_name: Option<[u8; 3]>,
}

impl Board {
    pub fn best(&self) -> u32 { self.entries.first().map_or(0, |e| e.score) }

    /// whether this score earns a place in the top ten
    pub fn qualifies(&self, score: u32) -> bool {
        score > 0 && (self.entries.len() < MAX_ENTRIES || self.entries.last().is_some_and(|e| score > e.score))
    }

    /// add an entry; returns its place (0 = top). Ties go below the earlier score.
    pub fn insert(&mut self, entry: Entry) -> usize {
        let at = self.entries.iter().position(|e| entry.score > e.score).unwrap_or(self.entries.len());
        self.last_name = Some(entry.name);
        self.entries.insert(at, entry);
        self.entries.truncate(MAX_ENTRIES);
        at
    }
}

pub struct Store {
    slots: Option<SaveSlotManager<()>>,
}

impl Store {
    /// Declares 32 KiB SRAM (emulators and flash carts read this from the ROM) and opens it.
    /// If the save can't be opened the game still runs; scores just aren't kept.
    pub fn open(save: &mut SaveManager) -> Self {
        Store { slots: save.init_sram(1, MAGIC).ok() }
    }

    pub fn load(&mut self) -> Board {
        self.slots.as_mut().and_then(|s| s.read::<Board>(0).ok()).unwrap_or_default()
    }

    /// true if the board was written
    pub fn save(&mut self, board: &Board) -> bool {
        self.slots.as_mut().is_some_and(|s| s.write(0, board, &()).is_ok())
    }
}
