//! Lesson 4: audio. A plays a note; Up/Down move it through a scale. One sample, any pitch.
//!
//!     cargo build --release --example 04_audio
#![no_std]
#![no_main]

use agb::{
    fixnum::Num,
    include_wav,
    input::{Button, ButtonController},
    sound::mixer::{Frequency, SoundChannel, SoundData},
};

// The wav must be recorded at the mixer's rate (18157 Hz here) or it plays at the wrong speed.
static PLUCK: SoundData = include_wav!("sfx/pluck.wav"); // an A (440 Hz)

/// playback speed for `semitones` above the sample's pitch, as 8-bit fixed point (256 = 1x)
fn speed(semitones: i32) -> Num<u32, 8> {
    const SEMI: [u32; 12] = [256, 271, 287, 304, 323, 342, 362, 384, 406, 431, 456, 483];
    let (octave, step) = (semitones.div_euclid(12), SEMI[semitones.rem_euclid(12) as usize]);
    let raw = if octave >= 0 { step << octave } else { step >> -octave };
    // agb 0.25's mixer panics at exactly 1.5x speed; nudge that one value (see the guide)
    Num::from_raw(if raw == 384 { 383 } else { raw })
}

#[agb::entry]
fn main(mut gba: agb::Gba) -> ! {
    let mut gfx = gba.graphics.get();
    let mut input = ButtonController::new();
    let mut mixer = gba.mixer.mixer(Frequency::Hz18157);

    const SCALE: [i32; 8] = [0, 2, 3, 5, 7, 8, 10, 12]; // A minor, in semitones above A
    let mut degree = 0usize;

    loop {
        input.update();
        if input.is_just_pressed(Button::Up) && degree < SCALE.len() - 1 { degree += 1; }
        if input.is_just_pressed(Button::Down) && degree > 0 { degree -= 1; }
        if input.is_just_pressed(Button::A) {
            let mut note = SoundChannel::new(PLUCK);
            note.playback(speed(SCALE[degree])).volume(Num::from_raw(200));
            let _ = mixer.play_sound(note); // None if all 8 channels are busy: the note is dropped
        }

        let frame = gfx.frame();
        // the mixer must be fed every frame or the audio stutters
        mixer.frame();
        frame.commit();
    }
}
