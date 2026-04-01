# Interrupts & Shared State on micro:bit v2

A deep-dive into setting up hardware interrupts for Button A, Button B, and an RTC
tick — and safely sharing state between interrupt handlers and the main loop in
`no_std` Rust on the nRF52833.

---

## Table of Contents

1. [Why interrupts?](#1-why-interrupts)
2. [How interrupts work on Cortex-M](#2-how-interrupts-work-on-cortex-m)
3. [The shared-state problem](#3-the-shared-state-problem)
4. [The `Mutex<RefCell<T>>` pattern](#4-the-mutexrefcellt-pattern)
5. [GPIOTE — GPIO Tasks and Events](#5-gpiote--gpio-tasks-and-events)
6. [Setting up Button A & B interrupts](#6-setting-up-button-a--b-interrupts)
7. [Combo button detection](#7-combo-button-detection)
8. [RTC — Real-Time Clock](#8-rtc--real-time-clock)
9. [Putting it all together](#9-putting-it-all-together)
10. [Debounce](#10-debounce)
11. [Common pitfalls](#11-common-pitfalls)

---

## 1. Why interrupts?

Your current main loop looks like this:

```rust
loop {
    display.show(&mut timer, display_buffer, PAUSE); // busy-waits 200ms
    let a = button_a.is_low().unwrap();              // polls every 200ms
    let b = button_b.is_low().unwrap();
}
```

The CPU runs at **full speed, 100% of the time** — even when nothing is happening.
On a battery-powered device this is wasteful.

With interrupts the flow becomes:

```
CPU sleeps (WFI — Wait For Interrupt)
  │
  ├─ Button pressed  → CPU wakes, ISR (Interrupt Service Routine) sets a flag, CPU sleeps again
  ├─ RTC tick fires  → CPU wakes, ISR sets a flag, CPU sleeps again
  └─ Display timer   → CPU wakes, ISR refreshes LED row, CPU sleeps again
```

The CPU only runs when there is actual work to do. This can reduce active current
from ~8 mA down to ~1–3 mA (the display LEDs themselves still draw power).

---

## 2. How interrupts work on Cortex-M

### The NVIC

The **Nested Vectored Interrupt Controller (NVIC)** is the Cortex-M hardware block
that manages interrupts. Every peripheral that can generate an interrupt has:

- An **IRQ (Interrupt ReQuest) number** — a unique identifier (e.g. `pac::Interrupt::GPIOTE`)
- An **enable bit** in the NVIC — must be set before the interrupt can fire
- A **pending bit** — set by hardware when the event occurs, cleared by the ISR
- A **priority level** — higher priority ISRs can preempt lower ones

### The vector table

At startup, `cortex-m-rt` installs a **vector table** in flash. Each entry is a
function pointer to an ISR. The `#[interrupt]` attribute macro registers your
function into this table under the correct IRQ name.

```
Vector table (flash, address 0x00000000)
├── 0: Stack pointer initial value
├── 1: Reset handler  ← your #[entry] fn
├── 2: NMI (Non-Maskable Interrupt) handler
├── ...
├── 16+N: GPIOTE handler  ← your #[interrupt] fn GPIOTE()
├── 16+M: RTC0 handler    ← your #[interrupt] fn RTC0()
└── ...
```

### Interrupt lifecycle

```
1. Peripheral event occurs (e.g. GPIO pin goes low)
2. Peripheral sets its EVENT register
3. NVIC sees the IRQ is pending
4. CPU finishes current instruction, pushes registers to stack
5. CPU jumps to your ISR
6. ISR runs, MUST clear the peripheral EVENT register
7. CPU pops registers, resumes where it left off
```

> ⚠️ **If you do not clear the EVENT register in the ISR, the interrupt fires
> again immediately after returning — infinite interrupt loop.**

---

## 3. The shared-state problem

Your ISR and your `main` loop run in the same memory space. The ISR can fire **at
any point** between any two instructions in `main`. This creates a classic
concurrency hazard:

```rust
// main loop reads button_pressed in two steps:
let flag = BUTTON_PRESSED;   // ← ISR fires HERE, sets BUTTON_PRESSED = true
if flag { ... }              // ← sees the OLD value, misses the press
```

Or worse with a struct — the ISR might write half of it before `main` reads the
other half, leaving you with a torn read.

In `std` Rust you would use `Mutex` from `std::sync` or an `Atomic`. In `no_std`
on Cortex-M, we use a different pattern.

---

## 4. The `Mutex<RefCell<T>>` pattern

The `cortex_m::interrupt::Mutex` is **not** a thread mutex. It works by
**disabling interrupts** for the duration of the critical section:

```rust
use core::cell::RefCell;
use cortex_m::interrupt::Mutex;

static MY_FLAG: Mutex<RefCell<bool>> = Mutex::new(RefCell::new(false));

// In main or ISR — safe access:
cortex_m::interrupt::free(|cs| {
    *MY_FLAG.borrow(cs).borrow_mut() = true;
});
```

- **`Mutex::new`** — wraps the value; safe to construct as a `static`
- **`interrupt::free(|cs| { ... })`** — disables interrupts, gives you a
  `CriticalSection` token (`cs`)
- **`.borrow(cs)`** — proves you're inside a critical section; returns `&RefCell<T>`
- **`.borrow_mut()`** — gives `RefMut<T>` for mutation

Because interrupts are disabled inside `free()`, the ISR cannot run concurrently —
no torn reads, no missed updates.

### Why `RefCell`?

`Mutex` only gives you a shared `&T` reference (because `&'static Mutex<T>` can be
accessed from anywhere). `RefCell` adds interior mutability with a runtime borrow
check, letting you get `&mut T` behind that shared reference.

### For larger types — wrap in `Option`

Peripherals like `Gpiote` cannot be constructed as a `const`, so we store them as
`Option<T>` and initialise at runtime:

```rust
static GPIOTE_PERIPH: Mutex<RefCell<Option<Gpiote>>> =
    Mutex::new(RefCell::new(None));

// In main, after constructing gpiote:
cortex_m::interrupt::free(|cs| {
    *GPIOTE_PERIPH.borrow(cs).borrow_mut() = Some(gpiote);
});

// In ISR:
cortex_m::interrupt::free(|cs| {
    if let Some(g) = GPIOTE_PERIPH.borrow(cs).borrow_mut().as_ref() {
        // use g
    }
});
```

---

## 5. GPIOTE — GPIO Tasks and Events

**GPIOTE** (GPIO Tasks and Events) is the nRF52833 peripheral that connects GPIO
pins to the interrupt system. Without it, GPIO pins have no interrupt capability.

### Channels

GPIOTE has **8 channels**. Each channel can monitor one pin for:

- **`hi_to_lo()`** — falling edge (pin goes HIGH → LOW) = button press (buttons
  are active-low on micro:bit)
- **`lo_to_hi()`** — rising edge (pin goes LOW → HIGH) = button release
- **`toggle()`** — both edges

Each channel has an **EVENT** register. When the configured edge is detected:
1. The channel's EVENT register is set to `1`
2. If the channel interrupt is enabled, the GPIOTE IRQ fires

All 8 channels share a **single IRQ line** (`GPIOTE`). Your ISR must check which
channel(s) triggered and clear only those events.

### Port events

There is also a **PORT event** — a single event that fires when *any* pin changes
(used for low-power wake from System OFF). We won't use it here.

---

## 6. Setting up Button A & B interrupts

### Pin mapping on micro:bit v2

| Button | nRF52833 pin | Active state |
|--------|-------------|--------------|
| A      | P0.14       | LOW (pressed) |
| B      | P0.23       | LOW (pressed) |

Both have internal pull-ups enabled by the BSP (Board Support Package — `board.buttons.button_a`).

### Full setup code

```rust
use core::cell::RefCell;
use cortex_m::interrupt::Mutex;
use microbit::{
    board::Board,
    hal::gpiote::Gpiote,
    pac::{self, interrupt},
};

// --- Statics ---

// Holds the GPIOTE peripheral so the ISR can clear events
static GPIOTE_PERIPH: Mutex<RefCell<Option<Gpiote>>> =
    Mutex::new(RefCell::new(None));

// (button_a_pressed, button_b_pressed) — set by ISR, consumed by main
static BUTTON_STATE: Mutex<RefCell<(bool, bool)>> =
    Mutex::new(RefCell::new((false, false)));

// --- Entry point ---

#[entry]
fn main() -> ! {
    let board = Board::take().unwrap();

    // 1. Construct GPIOTE
    let gpiote = Gpiote::new(board.GPIOTE);

    // 2. Channel 0 → Button A (P0.14), falling edge
    gpiote
        .channel0()
        .input_pin(&board.buttons.button_a.degrade())
        .hi_to_lo()
        .enable_interrupt();

    // 3. Channel 1 → Button B (P0.23), falling edge
    gpiote
        .channel1()
        .input_pin(&board.buttons.button_b.degrade())
        .hi_to_lo()
        .enable_interrupt();

    // 4. Store in global so ISR can access it
    cortex_m::interrupt::free(|cs| {
        *GPIOTE_PERIPH.borrow(cs).borrow_mut() = Some(gpiote);
    });

    // 5. Unmask the GPIOTE interrupt in the NVIC
    //    SAFETY: we are in the reset handler, no ISR can run yet
    unsafe { pac::NVIC::unmask(pac::Interrupt::GPIOTE) };

    loop {
        // Sleep until the next interrupt
        cortex_m::asm::wfi();

        // Read and consume button state (set by ISR)
        let (a, b) = cortex_m::interrupt::free(|cs| {
            let mut state = BUTTON_STATE.borrow(cs).borrow_mut();
            let snapshot = *state;
            *state = (false, false); // consume
            snapshot
        });

        if a || b {
            let action = infer_action(app_state.mode, a, b);
            app_state = handle_action(app_state, action);
        }
    }
}

// --- ISR ---

#[interrupt]
fn GPIOTE() {
    cortex_m::interrupt::free(|cs| {
        if let Some(gpiote) = GPIOTE_PERIPH.borrow(cs).borrow_mut().as_ref() {
            // Check which channels fired and clear their events
            let a_fired = gpiote.channel0().is_event_triggered();
            let b_fired = gpiote.channel1().is_event_triggered();

            if a_fired { gpiote.channel0().reset_events(); }
            if b_fired { gpiote.channel1().reset_events(); }

            if a_fired || b_fired {
                // Read the CURRENT pin state (not just which edge fired)
                // This naturally handles combo detection — see section 7
                let a_low = gpiote.channel0().input_pin_is_low();
                let b_low = gpiote.channel1().input_pin_is_low();

                let mut state = BUTTON_STATE.borrow(cs).borrow_mut();
                state.0 |= a_low; // OR — don't overwrite if already true
                state.1 |= b_low;
            }
        }
    });
}
```

### Why `.degrade()`?

`board.buttons.button_a` has a fully typed pin (`P0_14<Input<Floating>>`). The
`Gpiote` API accepts an `&Pin<Input<_>>` — a type-erased pin. `.degrade()` strips
the compile-time pin number from the type, giving you the erased form.

---

## 7. Combo button detection

Buttons are pressed by a human — they will **never be electrically simultaneous**.
One will always trigger the GPIOTE interrupt a few milliseconds before the other.

The trick used in the ISR above is:

```rust
// Read CURRENT state of BOTH pins, regardless of which one fired
let a_low = gpiote.channel0().input_pin_is_low();
let b_low = gpiote.channel1().input_pin_is_low();
```

**Scenario: user presses A then B within ~50ms**

```
t=0ms   Button A pressed  → GPIOTE fires, a_low=true,  b_low=false → state=(true, false)
t=30ms  Button B pressed  → GPIOTE fires, a_low=true,  b_low=true  → state=(true, true)
t=?     main loop wakes after wfi(), sees state=(true, true) → combo!
```

Because we use `|=` (OR) when writing to `BUTTON_STATE`, an A-press flag set at
t=0ms is not overwritten to `false` when B fires at t=30ms.

### Consuming state correctly

In `main`, after reading the state, reset it to `(false, false)`. If the user is
still holding both buttons when `main` reads, the ISR will set the flags again on
the *next* edge — which won't come until a release + re-press. This is fine for
your `infer_action` logic.

---

## 8. RTC — Real-Time Clock

### Why RTC instead of TIMER?

| | **TIMER** | **RTC** |
|---|---|---|
| Clock source | 16 MHz HFCLK (High-Frequency Clock) | 32.768 kHz LFCLK (Low-Frequency Clock) |
| Power in sleep | HFCLK stays on (~500 µA) | LFCLK only (~1 µA) |
| Resolution | nanoseconds | ~30 µs |
| Max period | ~268 seconds (32-bit) | ~36 hours |

For a countdown timer measuring minutes, the RTC is the right tool. It runs off the
low-frequency crystal oscillator which keeps ticking even when the CPU is sleeping.

### RTC prescaler

The RTC runs from a 32,768 Hz clock. The **prescaler** divides this down:

```
tick frequency = 32768 / (prescaler + 1)
tick period    = (prescaler + 1) / 32768 seconds
```

Common values:

| Prescaler | Tick frequency | Tick period |
|-----------|---------------|-------------|
| 0         | 32,768 Hz     | ~30 µs      |
| 31        | 1,024 Hz      | ~977 µs     |
| 327       | 100 Hz        | 10 ms       |
| 4095      | 8 Hz          | 125 ms      |

For a 200ms tick (matching your current `PAUSE`): prescaler = `6553`
→ 32768 / 6554 ≈ 5 Hz → 200ms per tick ✓

### Compare registers (CC — Compare Channel)

Rather than just using the raw tick, you can set a **CC** register (CC0–CC3).
The RTC fires a `COMPARE` event when the counter reaches the CC value. You can use
this for one-shot or periodic wakeups at precise intervals.

For a simple periodic tick, the `TICK` interrupt (fires every prescaler tick) is
easier.

### Setup code

```rust
use microbit::hal::rtc::{Rtc, RtcInterrupt};
use microbit::pac::{self, interrupt, RTC0};

static RTC_PERIPH: Mutex<RefCell<Option<Rtc<RTC0>>>> =
    Mutex::new(RefCell::new(None));

// Flag set by RTC ISR, consumed by main
static TICK_FLAG: Mutex<RefCell<bool>> =
    Mutex::new(RefCell::new(false));

// In main:
// Prescaler 4095 → 8 Hz → tick every 125ms (close to your 200ms PAUSE)
let mut rtc = Rtc::new(board.RTC0, 4095).unwrap();
rtc.enable_event(RtcInterrupt::Tick);
rtc.enable_interrupt(RtcInterrupt::Tick, None);
rtc.enable_counter();

cortex_m::interrupt::free(|cs| {
    *RTC_PERIPH.borrow(cs).borrow_mut() = Some(rtc);
});

unsafe { pac::NVIC::unmask(pac::Interrupt::RTC0) };

// In main loop, after wfi():
let ticked = cortex_m::interrupt::free(|cs| {
    let mut flag = TICK_FLAG.borrow(cs).borrow_mut();
    let v = *flag;
    *flag = false;
    v
});

if ticked {
    // update display, advance countdown, etc.
}
```

### RTC ISR

```rust
#[interrupt]
fn RTC0() {
    cortex_m::interrupt::free(|cs| {
        if let Some(rtc) = RTC_PERIPH.borrow(cs).borrow_mut().as_ref() {
            // Clear the TICK event — MANDATORY or ISR fires again immediately
            rtc.reset_event(RtcInterrupt::Tick);
            *TICK_FLAG.borrow(cs).borrow_mut() = true;
        }
    });
}
```

---

## 9. Putting it all together

With both GPIOTE and RTC set up, your main loop becomes:

```rust
loop {
    // CPU sleeps here — wakes on ANY unmasked interrupt
    cortex_m::asm::wfi();

    // --- Handle RTC tick ---
    let ticked = cortex_m::interrupt::free(|cs| {
        let mut flag = TICK_FLAG.borrow(cs).borrow_mut();
        let v = *flag;
        *flag = false;
        v
    });

    if ticked {
        tick_counter += 1;

        // Every 8 ticks @ 8Hz = ~1 second
        if tick_counter % 8 == 0 {
            second_indicator_on = !second_indicator_on;
        }

        // Every 480 ticks @ 8Hz = ~60 seconds
        if tick_counter >= 480 {
            tick_counter = 0;
            app_state = handle_minute_passing(app_state);
        }

        // Refresh display on every tick
        display_buffer = render_state(&app_state);
        if second_indicator_on && app_state.timer_started() {
            display_buffer = overlay(display_buffer, symbols::CORNERS);
        }
        display.show(&mut timer, display_buffer, 0); // 0ms blocking — just latch
    }

    // --- Handle button presses ---
    let (a, b) = cortex_m::interrupt::free(|cs| {
        let mut state = BUTTON_STATE.borrow(cs).borrow_mut();
        let snapshot = *state;
        *state = (false, false);
        snapshot
    });

    if a || b {
        let action = infer_action(app_state.mode, a, b);
        app_state = handle_action(app_state, action);
    }
}
```

### Interrupt priority

By default all interrupts share the same priority on Cortex-M0+ (which the
nRF52833 uses in a compatible mode). This means ISRs do **not** preempt each other
— if GPIOTE fires while RTC0 is being handled, GPIOTE waits. For this use case
that is perfectly fine; both ISRs are very short.

---

## 10. Debounce

Mechanical buttons bounce — the pin rapidly toggles for ~1–10ms before settling.
With interrupts this means **multiple ISR calls per physical press**.

### Software debounce in the ISR

Track the RTC counter value at the last accepted press. Ignore events that arrive
within a debounce window (e.g. 50ms = ~400 RTC ticks at 8 Hz):

```rust
static LAST_A_TICK: Mutex<RefCell<u32>> = Mutex::new(RefCell::new(0));
static LAST_B_TICK: Mutex<RefCell<u32>> = Mutex::new(RefCell::new(0));

const DEBOUNCE_TICKS: u32 = 4; // 4 ticks @ 8Hz = 500ms — adjust to taste

#[interrupt]
fn GPIOTE() {
    cortex_m::interrupt::free(|cs| {
        // Get current RTC counter for debounce timestamp
        let now = if let Some(rtc) = RTC_PERIPH.borrow(cs).borrow().as_ref() {
            rtc.get_counter()
        } else {
            return;
        };

        if let Some(gpiote) = GPIOTE_PERIPH.borrow(cs).borrow().as_ref() {
            let a_fired = gpiote.channel0().is_event_triggered();
            let b_fired = gpiote.channel1().is_event_triggered();

            if a_fired {
                gpiote.channel0().reset_events();
                let mut last = LAST_A_TICK.borrow(cs).borrow_mut();
                if now.wrapping_sub(*last) >= DEBOUNCE_TICKS {
                    *last = now;
                    BUTTON_STATE.borrow(cs).borrow_mut().0 = true;
                }
            }

            if b_fired {
                gpiote.channel1().reset_events();
                let mut last = LAST_B_TICK.borrow(cs).borrow_mut();
                if now.wrapping_sub(*last) >= DEBOUNCE_TICKS {
                    *last = now;
                    BUTTON_STATE.borrow(cs).borrow_mut().1 = true;
                }
            }
        }
    });
}
```

Note the use of `wrapping_sub` — the RTC counter is 24-bit and wraps around.

---

## 11. Common pitfalls

### ❌ Forgetting to clear the EVENT register
```rust
// BAD — ISR fires forever
#[interrupt]
fn GPIOTE() {
    // do stuff but never call gpiote.channel0().reset_events()
}
```
Always call `reset_events()` / `reset_event()` before returning from the ISR.

---

### ❌ Not unmasking in the NVIC
```rust
// You configured GPIOTE but never called:
unsafe { pac::NVIC::unmask(pac::Interrupt::GPIOTE) };
// Result: interrupts configured but never fire
```

---

### ❌ Calling `unwrap()` in an ISR
```rust
#[interrupt]
fn GPIOTE() {
    cortex_m::interrupt::free(|cs| {
        let g = GPIOTE_PERIPH.borrow(cs).borrow_mut().as_ref().unwrap(); // ← DANGER
    });
}
```
If the Option is `None` (initialisation race), this panics in an ISR — with
`panic-halt` the device freezes with no output. Always use `if let Some`.

---

### ❌ Long work inside a critical section
```rust
cortex_m::interrupt::free(|cs| {
    // Interrupts are DISABLED for this entire block
    display.show(&mut timer, buffer, 200); // ← blocks for 200ms with IRQs off!
});
```
Keep critical sections to the minimum needed — read/write a flag, return. Do the
actual work outside `interrupt::free`.

---

### ❌ Shared mutable state without `Mutex`
```rust
// NOT safe — ISR can tear this between the two assignments
static mut BUTTON_A: bool = false;
static mut BUTTON_B: bool = false;
```
On Cortex-M0+ (single-core, no out-of-order execution), single `bool` reads/writes
are atomic in practice, but Rust's memory model does not guarantee this. Use the
`Mutex<RefCell<T>>` pattern or `AtomicBool` from `core::sync::atomic`.

### ✅ `AtomicBool` — simpler for single flags

```rust
use core::sync::atomic::{AtomicBool, Ordering};

static BUTTON_A_PRESSED: AtomicBool = AtomicBool::new(false);

// In ISR:
BUTTON_A_PRESSED.store(true, Ordering::Relaxed);

// In main:
if BUTTON_A_PRESSED.swap(false, Ordering::Relaxed) {
    // handle press
}
```

`AtomicBool` does not require `interrupt::free` and has no runtime borrow-check
overhead. Use it for simple flags; use `Mutex<RefCell<T>>` when you need to share
a peripheral or a struct.
