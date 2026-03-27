// No standard library — we're running on bare metal with no OS.
#![no_std]
// No Rust runtime entry point; cortex_m_rt provides the reset handler instead.
#![no_main]

mod digits;
mod symbols;
mod types;

use cortex_m_rt::entry; // Marks `main` as the reset handler for Cortex-M.
use embedded_hal::digital::InputPin;
use microbit::hal::gpio::{p0::P0_14, p0::P0_23, Floating, Input};
use microbit::{board::Board, display::blocking::Display, hal::Timer};
use panic_halt as _; // On panic, halt the processor (stops execution).

// TODO: typo — rename `Menue` to `Menu`
#[derive(Copy, Clone)]
enum Mode {
    Menue,
    CountDown,
}

#[derive(Copy, Clone)]
enum Action {
    IncTimer,
    DecTimer,
    StartTimer,
    None,
    Reset,
}

struct AppState {
    mode: Mode,
    countdown_minutes: usize,
}

impl AppState {
    // TODO: button debounce — raw is_low() polling causes multiple triggers on a single press;
    //       track previous button state and only act on rising/falling edge transitions.

    fn decrement_minute(self) -> AppState {
        if self.countdown_minutes > 0 {
            AppState {
                countdown_minutes: self.countdown_minutes - 1,
                ..self
            }
        } else {
            self
        }
    }

    fn increment_minute(self) -> AppState {
        AppState {
            countdown_minutes: (self.countdown_minutes + 1).clamp(0, 60),
            ..self
        }
    }

    fn timer_started(&self) -> bool {
        match self.mode {
            Mode::Menue => false,
            Mode::CountDown => true,
        }
    }
}

// type ButtonA = P0_14<Input<Floating>>;
// type ButtonB = P0_23<Input<Floating>>;

// `#[entry]` replaces the standard `main`; `-> !` means this function never returns.
#[entry]
fn main() -> ! {
    // Take ownership of all board peripherals (can only be called once).
    let board = Board::take().unwrap();
    let mut button_a = board.buttons.button_a;
    let mut button_b = board.buttons.button_b;

    let mut state: AppState = AppState {
        mode: Mode::Menue,
        countdown_minutes: 5,
    };
    let mut second_indicator_on: bool = false;

    // // Wrap TIMER0 peripheral for use as a blocking delay source.
    let mut timer = Timer::new(board.TIMER0);
    // // Initialise the 5×5 LED matrix via its GPIO pins.
    let mut display = Display::new(board.display_pins);
    display.clear();

    const PAUSE: u32 = 200;
    const ONE_SECOND: u32 = 1000;
    const ONE_MINUTE: u32 = ONE_SECOND * 60;
    let mut minute_tracker: u32 = 0;
    let mut second_tracker: u32 = 0;

    let mut display_buffer;

    loop {
        let action = infer_action(
            state.mode,
            button_a.is_low().unwrap(),
            button_b.is_low().unwrap(),
        );
        state = handle_action(state, action);

        display_buffer = render_state(&state);

        // TODO: timing drift — accumulating PAUSE per loop assumes display.show() takes exactly
        //       PAUSE ms, but any jitter causes clock skew. Consider using timer.read() for
        //       elapsed time instead of loop-count accumulation.
        minute_tracker += PAUSE;
        if minute_tracker >= ONE_MINUTE {
            minute_tracker = 0;
            state = handle_minute_passing(state);
        }

        second_tracker += PAUSE;
        if second_tracker >= ONE_SECOND {
            second_indicator_on = !second_indicator_on;
            second_tracker = 0;
        }

        if second_indicator_on && state.timer_started() {
            display_buffer = overlay(display_buffer, symbols::CORNERS);
        }

        display.show(&mut timer, display_buffer, PAUSE);
    }
}

fn overlay(buff1: types::LedMatrix, buff2: types::LedMatrix) -> types::LedMatrix {
    let mut result = [[0u8; 5]; 5];
    for row in 0..5 {
        for col in 0..5 {
            result[row][col] = buff1[row][col] | buff2[row][col];
        }
    }
    result
}

fn render_state(state: &AppState) -> types::LedMatrix {
    match digits::DIGITS.get(state.countdown_minutes) {
        Some(&glyph) => glyph,
        None => symbols::CROSS,
    }
}

fn handle_minute_passing(state: AppState) -> AppState {
    match state.mode {
        Mode::Menue => state,
        Mode::CountDown => state.decrement_minute(),
    }
}

fn handle_action(state: AppState, action: Action) -> AppState {
    match state.mode {
        Mode::Menue => match action {
            Action::IncTimer => state.increment_minute(),
            Action::DecTimer => state.decrement_minute(),
            Action::StartTimer => AppState {
                mode: Mode::CountDown,
                ..state
            },
            _ => state,
        },
        Mode::CountDown => match action {
            Action::Reset => AppState {
                mode: Mode::Menue,
                countdown_minutes: 5,
            },
            _ => state,
        },
    }
}

fn infer_action(mode: Mode, button_a_pressed: bool, button_b_pressed: bool) -> Action {
    match mode {
        Mode::Menue => match (button_a_pressed, button_b_pressed) {
            (true, true) => Action::StartTimer,
            (true, false) => Action::DecTimer,
            (false, true) => Action::IncTimer,
            (false, false) => Action::None,
        },
        Mode::CountDown => match (button_a_pressed, button_b_pressed) {
            (true, true) => Action::Reset,
            (true, false) => Action::None,
            (false, true) => Action::None,
            (false, false) => Action::None,
        },
    }
}
