# rust-micro-bit

A countdown timer app for the **BBC Micro:Bit v2**, written in Rust using `#![no_std]` bare-metal embedded.

## What It Does

- **Menu mode** — set a countdown (1–99 minutes) using the two buttons
- **Countdown mode** — timer counts down with rich LED display:
  - **> 25 seconds remaining** — shows minutes as dot-count glyphs (tens on left columns, ones on right)
  - **≤ 25 seconds remaining** — LED grid fills dot-by-dot showing exact seconds
  - **< 10 seconds remaining** — shows a large digit glyph for the final countdown
  - **Finished** — bell animation (DING/DONG) plays on the LED matrix; any button resets
- **Tick indicator** — centre column dots pulse every 2 seconds during countdown

### Button Controls


| Mode       | Button A          | Button B          | A + B          |
|------------|-------------------|-------------------|----------------|
| Menu       | Decrement 1 min   | Increment 1 min   | Start timer    |
| Countdown  | —                 | —                 | —              |
| Finished   | Reset to menu     | Reset to menu     | Reset to menu  |

## Hardware

- **Board:** BBC Micro:Bit v2 (nRF52833 — ARM Cortex-M4F)
- **Display:** 5×5 LED matrix (non-blocking, TIMER1-driven)
- **Input:** Buttons A and B (GPIOTE interrupts + TIMER0 debounce)
- **Clock:** RTC0 at 1 Hz for countdown ticks

## Project Structure

```
rust-micro-bit/
├── .cargo/config.toml   # Cross-compile target + probe-rs runner
├── src/
│   ├── main.rs          # App loop, state machine, interrupt handlers
│   ├── types.rs         # Shared type aliases (LedMatrix)
│   ├── digits.rs        # LED glyphs for digits 0–9
│   └── symbols.rs       # Special glyphs (corners, cross, bell DING/DONG)
├── Cargo.toml
├── Embed.toml           # probe-rs config
└── memory.x             # Flash/RAM layout
```

## Build & Flash

```bash
# Add the embedded target (once)
rustup target add thumbv7em-none-eabihf

# Install flashing tools (once)
cargo install probe-rs-tools flip-link --locked

# Flash to connected Micro:Bit
cargo run --release
```

> See [`setup.md`](setup.md) for full environment setup from scratch.

## Resources

- [Embedded Rust Discovery Book](https://docs.rust-embedded.org/discovery/microbit/)
- [impl Rust for Microbit](https://mb2.implrust.com/)
- [microbit-v2 crate docs](https://crates.io/crates/microbit-v2)
