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
    display::nonblocking::{BitImage, Display},
    hal::{
        Timer,
        clocks::Clocks,
        gpio::{Floating, Input, Pin},
        gpiote::Gpiote,
        rtc::{Rtc, RtcInterrupt},
        timer::OneShot,
    },
    pac::{GPIOTE, Interrupt, RTC0, TIMER0, TIMER1, interrupt},
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

    fn render(&self) -> BitImage {
        let minutes = self.seconds_remaining / 60;
        if minutes > 99 {
            return BitImage::new(&symbols::CROSS);
        }
        let tens = (minutes / 10) as usize;
        let ones = (minutes % 10) as usize;

        let mut grid = [[0u8; 5]; 5];

        // cols 0,1: fill top-to-bottom, left-to-right for `tens`
        let mut rem = tens;
        'tens: for col in 0..2 {
            for row in 0..5 {
                if rem == 0 {
                    break 'tens;
                }
                grid[row][col] = 1;
                rem -= 1;
            }
        }

        // cols 4,3: fill top-to-bottom, right-to-left for `ones`
        let mut rem = ones;
        'ones: for col in [4, 3] {
            for row in 0..5 {
                if rem == 0 {
                    break 'ones;
                }
                grid[row][col] = 1;
                rem -= 1;
            }
        }

        match self.stage {
            Stage::Menu => {}
            Stage::Countdown => {
                if self.seconds_remaining.is_multiple_of(2) {
                    grid[1][2] = 1;
                    grid[3][2] = 1;
                }
            }
        }

        BitImage::new(&grid)
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
static DISPLAY: Mutex<RefCell<Option<Display<TIMER1>>>> = Mutex::new(RefCell::new(None));
static DEBOUNCE_TIMER: Mutex<RefCell<Option<Timer<TIMER0, OneShot>>>> =
    Mutex::new(RefCell::new(None));

// `#[entry]` replaces the standard `main`; `-> !` means this function never returns.
#[entry]
fn main() -> ! {
    cortex_m::interrupt::free(|cs| APP_STATE.borrow(cs).replace(Some(AppState::new())));
    let board = Board::take().unwrap();
    Clocks::new(board.CLOCK).start_lfclk();
    setup_rtc(board.RTC0);
    setup_display(board.TIMER1, board.display_pins);
    setup_debounce_timer(board.TIMER0);
    setup_gpiote(board.GPIOTE, board.buttons);
    loop {
        cortex_m::asm::wfi();
    }
}

fn setup_display(timer1: TIMER1, display_pins: microbit::gpio::DisplayPins) {
    let mut display = Display::new(timer1, display_pins);
    display.show(&BitImage::new(&symbols::BLANK));

    cortex_m::interrupt::free(|cs| {
        unsafe {
            cortex_m::peripheral::NVIC::unmask(Interrupt::TIMER1);
        }
        DISPLAY.borrow(cs).replace(Some(display));
    });
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

fn setup_debounce_timer(timer0: TIMER0) {
    let mut timer = Timer::one_shot(timer0);
    timer.enable_interrupt();
    cortex_m::interrupt::free(|cs| {
        // TIMER0 interrupt stays masked until first button press
        DEBOUNCE_TIMER.borrow(cs).replace(Some(timer));
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

// driven by TIMER1 — multiplexes the LED matrix rows
#[interrupt]
fn TIMER1() {
    cortex_m::interrupt::free(|cs| {
        if let Some(display) = DISPLAY.borrow(cs).borrow_mut().as_mut() {
            display.handle_display_event();
        }
    });
}

// once per sec
#[interrupt]
fn RTC0() {
    cortex_m::interrupt::free(|cs| {
        if let (Some(rtc), Some(app_state), Some(display)) = (
            RTC.borrow(cs).borrow().as_ref(),
            APP_STATE.borrow(cs).borrow_mut().as_mut(),
            DISPLAY.borrow(cs).borrow_mut().as_mut(),
        ) && rtc.is_event_triggered(RtcInterrupt::Tick)
        {
            rtc.reset_event(RtcInterrupt::Tick);
            app_state.tick_second();
            display.show(&app_state.render());
        }
    });
}

// on button edge — start debounce timer, suppress further GPIOTE events
#[interrupt]
fn GPIOTE() {
    cortex_m::interrupt::free(|cs| {
        if let (Some(gpiote), Some(timer)) = (
            GPIOTE_PERIPH.borrow(cs).borrow().as_ref(),
            DEBOUNCE_TIMER.borrow(cs).borrow_mut().as_mut(),
        ) {
            gpiote.channel1().reset_events();
            gpiote.channel2().reset_events();

            // mask GPIOTE until debounce window expires
            cortex_m::peripheral::NVIC::mask(Interrupt::GPIOTE);

            // start 50ms one-shot (timer runs at 1 MHz = 1 cycle/µs)
            timer.start(50_000u32);
            unsafe {
                cortex_m::peripheral::NVIC::unmask(Interrupt::TIMER0);
            }
        }
    });
}

// debounce window expired — read settled pin state and process
#[interrupt]
fn TIMER0() {
    cortex_m::interrupt::free(|cs| {
        if let (Some(timer), Some(buttons), Some(app_state), Some(display)) = (
            DEBOUNCE_TIMER.borrow(cs).borrow_mut().as_mut(),
            BUTTONS.borrow(cs).borrow_mut().as_mut(),
            APP_STATE.borrow(cs).borrow_mut().as_mut(),
            DISPLAY.borrow(cs).borrow_mut().as_mut(),
        ) {
            timer.reset_event();
            cortex_m::peripheral::NVIC::mask(Interrupt::TIMER0);

            let (btn_a, btn_b) = buttons;
            let a = btn_a.is_low().unwrap();
            let b = btn_b.is_low().unwrap();

            if a || b {
                app_state.handle_button_pressed(a, b);
                display.show(&app_state.render());
            }

            // re-enable button interrupts
            unsafe {
                cortex_m::peripheral::NVIC::unmask(Interrupt::GPIOTE);
            }
        }
    });
}
