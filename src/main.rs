// No standard library — we're running on bare metal with no OS.
#![no_std]
// No Rust runtime entry point; cortex_m_rt provides the reset handler instead.
#![no_main]

mod digits;
mod symbols;
mod types;

use core::cell::RefCell;
use cortex_m::interrupt::Mutex;
use cortex_m_rt::entry;
use embedded_hal::digital::InputPin;
use microbit::{
    Board,
    board::Buttons,
    hal::{
        clocks::Clocks,
        gpio::{Floating, Input, Pin},
        gpiote::Gpiote,
        rtc::{Rtc, RtcInterrupt},
    },
    pac::{GPIOTE, Interrupt, RTC0, interrupt},
};
// Marks `main` as the reset handler for Cortex-M.
use panic_halt as _; // On panic, halt the processor (stops execution).

enum Stage {
    Menu,
    Countdown,
}

struct AppState {
    stage: Stage,
    seconds_remaining: u32,
    // buttons: (bool, bool),
}

const DEFAULT_TIMER_SEC: u32 = 5 * 60;

impl AppState {
    const fn new() -> Self {
        AppState {
            stage: Stage::Menu,
            seconds_remaining: DEFAULT_TIMER_SEC,
        }
    }

    fn reset(&mut self) {
        self.stage = Stage::Menu;
        self.seconds_remaining = DEFAULT_TIMER_SEC;
    }

    fn handle_button_pressed(&mut self, a: bool, b: bool) {
        // self.buttons = (a, b);
        match self.stage {
            Stage::Menu => match (a, b) {
                (true, true) => self.stage = Stage::Countdown,
                (true, false) => {
                    if self.seconds_remaining > 0 {
                        self.seconds_remaining -= 1;
                    }
                }
                (false, true) => self.seconds_remaining += 1,
                (false, false) => {}
            },
            Stage::Countdown => match (a, b) {
                (true, true) => self.reset(),
                (true, false) => {}
                (false, true) => {}
                (false, false) => {}
            },
        }
    }
    fn tick_second(&mut self) {
        match self.stage {
            Stage::Menu => {}
            Stage::Countdown => {
                if self.seconds_remaining > 0 {
                    self.seconds_remaining -= 1;
                }
            }
        }
    }
}

// static BOARD: Mutex<RefCell<Option<Board>>> = Mutex::new(RefCell::new(None));
static GPIOTE_PERIPH: Mutex<RefCell<Option<Gpiote>>> = Mutex::new(RefCell::new(None));
type ButtonPin = Pin<Input<Floating>>;
static BUTTONS: Mutex<RefCell<Option<(ButtonPin, ButtonPin)>>> = Mutex::new(RefCell::new(None));
static APP_STATE: Mutex<RefCell<Option<AppState>>> = Mutex::new(RefCell::new(None));
static RTC: Mutex<RefCell<Option<Rtc<RTC0>>>> = Mutex::new(RefCell::new(None));

// `#[entry]` replaces the standard `main`; `-> !` means this function never returns.
#[entry]
fn main() -> ! {
    cortex_m::interrupt::free(|cs| APP_STATE.borrow(cs).replace(Some(AppState::new())));
    let board = Board::take().unwrap();
    Clocks::new(board.CLOCK).start_lfclk();
    setup_rtc(board.RTC0);
    setup_gpiote(board.GPIOTE, board.buttons);
    loop {
        cortex_m::asm::wfi();
    }
}

fn setup_rtc(rtc0: RTC0) {
    // fRTC = 32_768 / (prescaler + 1) => prescaler = 32767 gives 1 Hz
    let mut rtc = Rtc::new(rtc0, 32767).unwrap();
    rtc.enable_interrupt(RtcInterrupt::Tick, None);
    rtc.enable_counter();

    cortex_m::interrupt::free(|cs| {
        unsafe {
            cortex_m::peripheral::NVIC::unmask(Interrupt::RTC0);
        }
        RTC.borrow(cs).replace(Some(rtc));
    });
}

fn setup_gpiote(gpiote_periph: GPIOTE, buttons: Buttons) {
    let gpiote = Gpiote::new(gpiote_periph);

    let btn_a = buttons.button_a.degrade();
    let btn_b = buttons.button_b.degrade();

    // setup chan1 for button_a hi to low
    gpiote
        .channel1()
        .input_pin(&btn_a)
        .hi_to_lo()
        .enable_interrupt();

    // setup chan2 for button_b hi to low
    gpiote
        .channel2()
        .input_pin(&btn_b)
        .hi_to_lo()
        .enable_interrupt();

    cortex_m::interrupt::free(|cs| {
        // unmask GPIOTE interrupt
        unsafe {
            cortex_m::peripheral::NVIC::unmask(Interrupt::GPIOTE);
        }
        GPIOTE_PERIPH.borrow(cs).replace(Some(gpiote));
        BUTTONS.borrow(cs).replace(Some((btn_a, btn_b)));
    });
}

// once per sec
#[interrupt]
fn RTC0() {
    cortex_m::interrupt::free(|cs| {
        if let (Some(rtc), Some(app_state)) = (
            RTC.borrow(cs).borrow().as_ref(),
            APP_STATE.borrow(cs).borrow_mut().as_mut(),
        ) && rtc.is_event_triggered(RtcInterrupt::Tick)
        {
            rtc.reset_event(RtcInterrupt::Tick);
            app_state.tick_second();
        }
    });
}

// on button click
#[interrupt]
fn GPIOTE() {
    cortex_m::interrupt::free(|cs| {
        if let (Some(gpiote), Some(buttons), Some(app_state)) = (
            GPIOTE_PERIPH.borrow(cs).borrow().as_ref(),
            BUTTONS.borrow(cs).borrow_mut().as_mut(),
            APP_STATE.borrow(cs).borrow_mut().as_mut(),
        ) {
            let (btn_a, btn_b) = buttons;
            let a_pressed = btn_a.is_low().unwrap();
            let b_pressed = btn_b.is_low().unwrap();

            if gpiote.channel1().is_event_triggered() {
                gpiote.channel1().reset_events();
                // button_a pressed
                app_state.handle_button_pressed(a_pressed, b_pressed);
            }
            if gpiote.channel2().is_event_triggered() {
                gpiote.channel2().reset_events();
                // button_b pressed
                app_state.handle_button_pressed(a_pressed, b_pressed);
            }
        }
    });
}
