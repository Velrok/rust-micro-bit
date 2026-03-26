// No standard library — we're running on bare metal with no OS.
#![no_std]
// No Rust runtime entry point; cortex_m_rt provides the reset handler instead.
#![no_main]

mod digits;
mod symbols;

use cortex_m_rt::entry; // Marks `main` as the reset handler for Cortex-M.
use embedded_hal::digital::InputPin;
use microbit::hal::gpio::{p0::P0_14, p0::P0_23, Floating, Input};
use microbit::{board::Board, display::blocking::Display, hal::Timer};
use panic_halt as _; // On panic, halt the processor (stops execution).

enum Mode {
    Menue,
    CountDown,
}

type ButtonA = P0_14<Input<Floating>>;
type ButtonB = P0_23<Input<Floating>>;

// `#[entry]` replaces the standard `main`; `-> !` means this function never returns.
#[entry]
fn main() -> ! {
    // Take ownership of all board peripherals (can only be called once).
    let board = Board::take().unwrap();
    let mut button_a = board.buttons.button_a;
    let mut button_b = board.buttons.button_b;
    let mut mode = Mode::Menue;
    let mut countdown_minutes: usize = 5;
    // // Wrap TIMER0 peripheral for use as a blocking delay source.
    let mut timer = Timer::new(board.TIMER0);
    // // Initialise the 5×5 LED matrix via its GPIO pins.
    let mut display = Display::new(board.display_pins);
    display.clear();

    // // 5×5 bitmap for a heart shape: 1 = LED on, 0 = LED off.
    // let heart = [
    //     [0, 1, 0, 1, 0],
    //     [1, 1, 1, 1, 1],
    //     [1, 1, 1, 1, 1],
    //     [0, 1, 1, 1, 0],
    //     [0, 0, 1, 0, 0],
    // ];

    loop {
        match mode {
            Mode::Menue => menue_handler(&mut button_a, &mut button_b, &mut countdown_minutes),
            Mode::CountDown => todo!(),
        }
        let glyph = match digits::DIGITS.get(countdown_minutes) {
            Some(&glyph) => glyph,
            None => symbols::CROSS,
        };
        display.show(&mut timer, glyph, 200);
        // // Show the heart for 1000 ms (blocking — uses the timer internally).
        // display.show(&mut timer, heart, 1000);
        // // Turn off all LEDs before the pause.
        // display.clear();
        // // Wait 500 ms with the display blank, creating a blinking effect.
        // timer.delay_ms(500_u32);
    }
}

fn menue_handler(button_a: &mut ButtonA, button_b: &mut ButtonB, countdown_minutes: &mut usize) {
    if button_a.is_low().unwrap() && *countdown_minutes > 0_usize {
        *countdown_minutes -= 1_usize;
    }

    if button_b.is_low().unwrap() && *countdown_minutes < 60_usize {
        *countdown_minutes += 1_usize;
    }
}
