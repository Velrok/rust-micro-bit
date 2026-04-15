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
        rtc::{Rtc, RtcCompareReg, RtcInterrupt},
        timer::OneShot,
    },
    pac::{GPIOTE, Interrupt, RTC0, TIMER0, TIMER1, interrupt},
};
use panic_halt as _;
use rtt_target::{rprintln, rtt_init_print}; // On panic, halt the processor (stops execution).

enum Stage {
    Menu,
    Countdown,
    Fin(bool),
}

struct AppState {
    stage: Stage,
    seconds_remaining: u32,
}

const ONE_MINUTE: u32 = 60;
const MAX_MINUTES: u32 = ONE_MINUTE * 100;
const DEFAULT_TIMER_SEC: u32 = 5 * ONE_MINUTE;

impl AppState {
    const fn new() -> Self {
        AppState {
            stage: Stage::Menu,
            seconds_remaining: DEFAULT_TIMER_SEC,
        }
    }

    fn reset(&mut self) {
        rprintln!("reset");
        self.stage = Stage::Menu;
        self.seconds_remaining = DEFAULT_TIMER_SEC;
    }

    fn inc_timer(&mut self) {
        rprintln!("inc_timer");
        if self.seconds_remaining < MAX_MINUTES - ONE_MINUTE {
            self.seconds_remaining += ONE_MINUTE;
        }
    }

    fn dec_timer(&mut self) {
        rprintln!("dec_timer");
        if self.seconds_remaining > ONE_MINUTE {
            self.seconds_remaining -= ONE_MINUTE;
        }
    }

    fn start_timer(&mut self) {
        rprintln!("start_timer");
        self.stage = Stage::Countdown;
    }

    fn handle_button_pressed(&mut self, a: bool, b: bool) {
        // self.buttons = (a, b);
        match self.stage {
            Stage::Menu => match (a, b) {
                (true, true) => self.start_timer(),
                (true, false) => self.dec_timer(),
                (false, true) => self.inc_timer(),
                (false, false) => {}
            },
            Stage::Countdown => {}
            Stage::Fin(_) => {
                if a || b {
                    self.reset()
                }
            }
        }
    }

    fn render_countdown(&self) -> BitImage {
        if self.seconds_remaining < 10 {
            let digit = self.seconds_remaining as usize;
            return match digits::DIGITS.get(digit) {
                Some(&glyph) => BitImage::new(&glyph),
                None => BitImage::new(&symbols::CROSS),
            };
        }
        if self.seconds_remaining <= 25 {
            let mut grid = [[0u8; 5]; 5];
            let mut rem = self.seconds_remaining as usize;
            'grid: for row in 0..5 {
                for col in 0..5 {
                    if rem == 0 {
                        break 'grid;
                    }
                    grid[row][col] = 1;
                    rem -= 1;
                }
            }
            return BitImage::new(&grid);
        }
        let minutes = self.seconds_remaining / 60;
        if minutes >= MAX_MINUTES {
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

        if let Stage::Countdown = self.stage
            && self.seconds_remaining.is_multiple_of(2)
        {
            grid[1][2] = 1;
            grid[3][2] = 1;
        }

        BitImage::new(&grid)
    }

    fn render(&self) -> BitImage {
        match self.stage {
            Stage::Menu | Stage::Countdown => self.render_countdown(),
            Stage::Fin(ding) => BitImage::new(match ding {
                true => &symbols::BELL_DING,
                false => &symbols::BELL_DONG,
            }),
        }
    }

    fn ring(&mut self) {
        self.stage = Stage::Fin(true);
    }

    fn tick_second(&mut self) {
        match self.stage {
            Stage::Countdown => {
                if self.seconds_remaining > 0 {
                    self.seconds_remaining -= 1;
                } else {
                    self.ring()
                };
                rprintln!("tick: {}s remaining", self.seconds_remaining);
            }
            Stage::Fin(ding) => self.stage = Stage::Fin(!ding),
            _ => {}
        }
    }
}

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
    rtt_init_print!();
    rprintln!("booting...");

    cortex_m::interrupt::free(|cs| APP_STATE.borrow(cs).replace(Some(AppState::new())));
    rprintln!("app state initialised");

    let board = Board::take().unwrap();

    Clocks::new(board.CLOCK).start_lfclk();
    rprintln!("lfclk started");

    setup_rtc(board.RTC0);
    rprintln!("rtc ready");

    setup_display(board.TIMER1, board.display_pins);
    rprintln!("display ready");

    setup_debounce_timer(board.TIMER0);
    rprintln!("debounce timer ready");

    setup_gpiote(board.GPIOTE, board.buttons);
    rprintln!("gpiote ready — entering loop");

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
    // prescaler = 0 → counter runs at 32768 Hz
    // Compare0 = 32768 → fires every 1 second
    let mut rtc = Rtc::new(rtc0, 0).unwrap();
    rtc.set_compare(RtcCompareReg::Compare0, 32768).unwrap();
    rtc.enable_interrupt(RtcInterrupt::Compare0, None);
    rtc.enable_counter();

    cortex_m::interrupt::free(|cs| {
        unsafe {
            cortex_m::peripheral::NVIC::unmask(Interrupt::RTC0);
        }
        RTC.borrow(cs).replace(Some(rtc));
    });
}

fn setup_debounce_timer(timer0: TIMER0) {
    // OneShot mode: timer counts up once, fires CC[0] event, then stops.
    // We enable the interrupt here but keep it masked in the NVIC — it will
    // only be unmasked by the GPIOTE handler when a press is detected.
    let mut timer = Timer::one_shot(timer0);
    timer.enable_interrupt();
    cortex_m::interrupt::free(|cs| {
        // TIMER0 interrupt stays masked until first button press
        DEBOUNCE_TIMER.borrow(cs).replace(Some(timer));
    });
}

fn setup_gpiote(gpiote_periph: GPIOTE, buttons: Buttons) {
    let gpiote = Gpiote::new(gpiote_periph);

    // Buttons are active-low: pin is pulled high at rest, goes low when pressed.
    // We degrade the typed pin to an erased `Pin` so both buttons share one type.
    let btn_a = buttons.button_a.degrade();
    let btn_b = buttons.button_b.degrade();

    // hi_to_lo = falling edge = button press event.
    // Each channel watches one pin and raises the GPIOTE interrupt on that edge.
    gpiote
        .channel1()
        .input_pin(&btn_a)
        .hi_to_lo()
        .enable_interrupt();

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
            RTC.borrow(cs).borrow_mut().as_mut(),
            APP_STATE.borrow(cs).borrow_mut().as_mut(),
            DISPLAY.borrow(cs).borrow_mut().as_mut(),
        ) && rtc.is_event_triggered(RtcInterrupt::Compare0)
        {
            rtc.reset_event(RtcInterrupt::Compare0);
            // Schedule the next 1-second tick by advancing the compare value.
            // The RTC counter is 24-bit (max 0x00FF_FFFF = 16_777_215), so we
            // mask with 0x00FF_FFFF to wrap around correctly instead of
            // overflowing into bits the hardware ignores.
            let next = (rtc.get_counter() + 32768) & 0x00FF_FFFF;
            rtc.set_compare(RtcCompareReg::Compare0, next).ok();
            app_state.tick_second();
            display.show(&app_state.render());
        }
    });
}

// GPIOTE fires on the first falling edge (button press).
//
// Debounce strategy — interrupt suppression window:
//   1. Clear the edge events so the interrupt doesn't re-fire immediately.
//   2. Mask GPIOTE in the NVIC — no more button interrupts until we re-enable it.
//   3. Start TIMER0 as a 50 ms one-shot. While it counts, any mechanical bounce
//      on the pin is completely ignored because GPIOTE is masked.
//   4. When TIMER0 fires the pin has settled; TIMER0 handler reads the final state.
#[interrupt]
fn GPIOTE() {
    cortex_m::interrupt::free(|cs| {
        if let (Some(gpiote), Some(timer)) = (
            GPIOTE_PERIPH.borrow(cs).borrow().as_ref(),
            DEBOUNCE_TIMER.borrow(cs).borrow_mut().as_mut(),
        ) {
            // Clear pending events on both channels so the GPIOTE interrupt
            // doesn't re-trigger the moment we return from this handler.
            gpiote.channel1().reset_events();
            gpiote.channel2().reset_events();

            // Mask GPIOTE in the NVIC — suppresses all further button edges
            // for the duration of the debounce window.
            cortex_m::peripheral::NVIC::mask(Interrupt::GPIOTE);

            // Start 50 ms one-shot. TIMER0 runs at 1 MHz (1 tick = 1 µs),
            // so 50_000 ticks = 50 ms — long enough to outlast contact bounce.
            timer.start(50_000u32);
            // Unmask TIMER0 so its CC[0] event can wake us when the window ends.
            unsafe {
                cortex_m::peripheral::NVIC::unmask(Interrupt::TIMER0);
            }
        }
    });
}

// TIMER0 fires when the 50 ms debounce window has elapsed.
// The pins have had time to settle, so we can now safely read them.
#[interrupt]
fn TIMER0() {
    cortex_m::interrupt::free(|cs| {
        if let (Some(timer), Some(buttons), Some(app_state), Some(display)) = (
            DEBOUNCE_TIMER.borrow(cs).borrow_mut().as_mut(),
            BUTTONS.borrow(cs).borrow_mut().as_mut(),
            APP_STATE.borrow(cs).borrow_mut().as_mut(),
            DISPLAY.borrow(cs).borrow_mut().as_mut(),
        ) {
            // Acknowledge the timer event and mask TIMER0 again — it has no
            // further work to do until the next button press.
            timer.reset_event();
            cortex_m::peripheral::NVIC::mask(Interrupt::TIMER0);

            // Read settled pin levels. Buttons are active-low, so `is_low`
            // returns true when the button is still held at this moment.
            let (btn_a, btn_b) = buttons;
            let a = btn_a.is_low().unwrap();
            let b = btn_b.is_low().unwrap();

            // Only act if at least one button is still down — this discards
            // phantom triggers caused by pure noise that resolved to no press.
            if a || b {
                app_state.handle_button_pressed(a, b);
                display.show(&app_state.render());
            }

            // Re-arm GPIOTE so the next button press can start a new debounce cycle.
            unsafe {
                cortex_m::peripheral::NVIC::unmask(Interrupt::GPIOTE);
            }
        }
    });
}
