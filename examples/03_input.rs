//! Lesson 3: input. Fly a ship around with the D-pad; A and B turn it.
//!
//!     cargo build --release --example 03_input
#![no_std]
#![no_main]

use agb::{
    display::{
        AffineMatrix,
        object::{AffineMatrixObject, AffineMode, ObjectAffine, include_aseprite},
    },
    fixnum::{Num, Vector2D, num, vec2},
    input::{Button, ButtonController},
};

include_aseprite!(mod sprites, 32x32 "gfx/ships.png");

#[agb::entry]
fn main(mut gba: agb::Gba) -> ! {
    let mut gfx = gba.graphics.get();
    let mut input = ButtonController::new();
    // fixed point: 24 bits of whole pixels and 8 of fraction, so sub-pixel speeds like 1.5 work
    let mut pos: Vector2D<Num<i32, 8>> = vec2(num!(120.0), num!(80.0));
    let mut angle: Num<i32, 8> = num!(0);

    loop {
        // read the buttons once per frame; everything else asks this snapshot
        input.update();

        // x_tri / y_tri are -1, 0 or +1 for the D-pad axes (y grows downwards on screen)
        let step = vec2(Num::new(input.x_tri() as i32), Num::new(input.y_tri() as i32)) * num!(1.5);
        pos += step;
        if input.is_pressed(Button::A) { angle += num!(0.01); }
        if input.is_pressed(Button::B) { angle -= num!(0.01); }
        if input.is_just_pressed(Button::Start) { pos = vec2(num!(120.0), num!(80.0)); } // edge, not level

        let mut frame = gfx.frame();
        let matrix = AffineMatrixObject::new(AffineMatrix::from_rotation(-angle));
        let mut ship = ObjectAffine::new(sprites::SHIPS.sprite(0), matrix, AffineMode::AffineDouble);
        ship.set_pos((pos.x.floor() - 32, pos.y.floor() - 32));
        ship.show(&mut frame);
        frame.commit();
    }
}
