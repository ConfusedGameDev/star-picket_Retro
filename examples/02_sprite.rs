//! Lesson 2: sprites. One plain sprite, and one rotated by the hardware's affine unit.
//!
//!     cargo build --release --example 02_sprite
#![no_std]
#![no_main]

use agb::{
    display::{
        AffineMatrix,
        object::{AffineMatrixObject, AffineMode, Object, ObjectAffine, include_aseprite},
    },
    fixnum::{Num, num},
};

// ships.png is a strip of 32x32 frames; `32x32` tells the macro to split it into frames.
include_aseprite!(mod sprites, 32x32 "gfx/ships.png");

#[agb::entry]
fn main(mut gba: agb::Gba) -> ! {
    let mut gfx = gba.graphics.get();
    let mut angle: Num<i32, 8> = num!(0); // in turns: 0.25 is a quarter turn

    loop {
        angle = (angle + num!(0.004)).rem_euclid(num!(1));

        let mut frame = gfx.frame();

        // A plain object: position is its top-left corner, in pixels.
        let mut plain = Object::new(sprites::SHIPS.sprite(0));
        plain.set_pos((60 - 16, 80 - 16));
        plain.show(&mut frame);

        // An affine object: the matrix maps screen pixels back into the sprite, so a rotation by
        // `angle` on screen uses the inverse rotation. AffineDouble doubles the drawing area to
        // 64x64 so the corners aren't clipped while it turns, so the top-left moves by 32, not 16.
        let matrix = AffineMatrixObject::new(AffineMatrix::from_rotation(-angle));
        let mut spinning = ObjectAffine::new(sprites::SHIPS.sprite(3), matrix, AffineMode::AffineDouble);
        spinning.set_pos((180 - 32, 80 - 32));
        spinning.show(&mut frame);

        frame.commit();
    }
}
