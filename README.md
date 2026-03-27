# rust-micro-bit

A countdown timer app for the **BBC Micro:Bit v2**, written in Rust using `#![no_std]` bare-metal embedded.

## What It Does

- **Menu mode** — set a countdown (0–60 minutes) using the two buttons
- **Countdown mode** — timer counts down; corners flash each second as a visual tick
- Display shows the current minute value as a digit glyph on the 5×5 LED matrix

### Button Controls

| Mode     | Button A       | Button B       | A + B          |
|----------|----------------|----------------|----------------|
| Menu     | Decrement time | Increment time | Start timer    |
| Countdown | —             | —              | Reset to menu  |

## Hardware

- **Board:** BBC Micro:Bit v2 (nRF52833 — ARM Cortex-M4F)
- **Display:** 5×5 LED matrix
- **Input:** Buttons A and B

## Project Structure

```
rust-micro-bit/
├── .cargo/config.toml   # Cross-compile target + probe-rs runner
├── src/
│   ├── main.rs          # App loop, state machine, button handling
│   ├── types.rs         # Shared type aliases (LedMatrix)
│   ├── digits.rs        # LED glyphs for digits 0–60
│   └── symbols.rs       # Special symbols (corners, cross)
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

## Known TODOs

- **Button debounce** — raw polling causes multiple triggers per press
- **Timing drift** — loop-count accumulation skews the clock; should use `timer.read()` for elapsed time

## Resources

- [Embedded Rust Discovery Book](https://docs.rust-embedded.org/discovery/microbit/)
- [impl Rust for Microbit](https://mb2.implrust.com/)
- [microbit-v2 crate docs](https://crates.io/crates/microbit-v2)
