// No standard library — we're running on bare metal with no OS.
#![no_std]
// No Rust runtime entry point; cortex_m_rt provides the reset handler instead.
#![no_main]

use cortex_m_rt::entry; // Marks `main` as the reset handler for Cortex-M.
use microbit::{board::Board, display::blocking::Display, hal::Timer};
use panic_halt as _; // On panic, halt the processor (stops execution).

// `#[entry]` replaces the standard `main`; `-> !` means this function never returns.
#[entry]
fn main() -> ! {
    // Take ownership of all board peripherals (can only be called once).
    let board = Board::take().unwrap();
    // Wrap TIMER0 peripheral for use as a blocking delay source.
    let mut timer = Timer::new(board.TIMER0);
    // Initialise the 5×5 LED matrix via its GPIO pins.
    let mut display = Display::new(board.display_pins);

    // 5×5 bitmap for a heart shape: 1 = LED on, 0 = LED off.
    let heart = [
        [0, 1, 0, 1, 0],
        [1, 1, 1, 1, 1],
        [1, 1, 1, 1, 1],
        [0, 1, 1, 1, 0],
        [0, 0, 1, 0, 0],
    ];

    loop {
        // Show the heart for 1000 ms (blocking — uses the timer internally).
        display.show(&mut timer, heart, 1000);
        // Turn off all LEDs before the pause.
        display.clear();
        // Wait 500 ms with the display blank, creating a blinking effect.
        timer.delay_ms(500_u32);
    }
}
