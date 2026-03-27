## 🎯 Overview

The Micro:Bit v2 uses an **nRF52833** (ARM Cortex-M4F), so you'll cross-compile from your Mac targeting `thumbv7em-none-eabihf`.

---

## 1️⃣ Install Prerequisites

**Rust toolchain + the embedded target:**
```bash
# Install Rust if you haven't
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Add the Micro:Bit v2 target (Cortex-M4F with FPU)
rustup target add thumbv7em-none-eabihf
```

**probe-rs** — the tool that flashes firmware and gives you debugging/RTT logging:
```bash
cargo install probe-rs-tools --locked
```

**flip-link** — safer linker that catches stack overflows:
```bash
cargo install flip-link
```

---

## 2️⃣ Create Your Project

```bash
cargo new my-microbit-project
cd my-microbit-project
```

---

## 3️⃣ `Cargo.toml` — Add Dependencies

```toml
[package]
name = "my-microbit-project"
version = "0.1.0"
edition = "2021"

[dependencies]
microbit-v2 = "0.15"
cortex-m-rt = "0.7"
cortex-m = "0.7"
panic-halt = "0.2"

# Optional but recommended: logging over RTT
rtt-target = { version = "0.5", features = ["cortex-m"] }
```

---

## 4️⃣ `.cargo/config.toml` — Tell Cargo About the Target

Create `.cargo/config.toml`:
```toml
[build]
target = "thumbv7em-none-eabihf"

[target.thumbv7em-none-eabihf]
runner = "probe-rs run --chip nRF52833_xxAA"
rustflags = [
  "-C", "linker=flip-link",
  "-C", "link-arg=-Tlink.x",
  "-C", "link-arg=--nmagic",
]
```

---

## 5️⃣ `Embed.toml` — probe-rs Config (for `cargo embed`)

Create `Embed.toml` in project root:
```toml
[default.general]
chip = "nRF52833_xxAA"

[default.reset]
halt_afterwards = false

[default.rtt]
enabled = true

[default.gdb]
enabled = false
```

---

## 6️⃣ `memory.x` — Memory Layout

Create `memory.x` in project root:
```
MEMORY
{
  FLASH : ORIGIN = 0x00000000, LENGTH = 512K
  RAM   : ORIGIN = 0x20000000, LENGTH = 128K
}
```

---

## 7️⃣ `src/main.rs` — Hello World (blink LEDs)

```rust
#![no_std]
#![no_main]

use cortex_m_rt::entry;
use microbit::{
    board::Board,
    display::blocking::Display,
    hal::Timer,
};
use panic_halt as _;

#[entry]
fn main() -> ! {
    let board = Board::take().unwrap();
    let mut timer = Timer::new(board.TIMER0);
    let mut display = Display::new(board.display_pins);

    let heart = [
        [0, 1, 0, 1, 0],
        [1, 1, 1, 1, 1],
        [1, 1, 1, 1, 1],
        [0, 1, 1, 1, 0],
        [0, 0, 1, 0, 0],
    ];

    loop {
        display.show(&mut timer, heart, 1000);
        display.clear();
        timer.delay_ms(500_u32);
    }
}
```

---

## 8️⃣ Build & Flash

Plug in your Micro:Bit via USB, then:

```bash
# Build only
cargo build --release

# Build + flash + attach RTT console (all in one!)
cargo run --release

# Or use cargo embed for more control
cargo embed --release
```

> 💡 **`cargo run`** works because of the `runner` line in `.cargo/config.toml` — it calls `probe-rs` automatically.

---

## 🗂️ Final File Structure

```
my-microbit-project/
├── .cargo/
│   └── config.toml
├── src/
│   └── main.rs
├── Cargo.toml
├── Embed.toml
└── memory.x
```

---

## 📚 Great Next Resources

- **[Embedded Rust Discovery Book](https://docs.rust-embedded.org/discovery/microbit/)** — the official intro course using the Micro:Bit
- **[impl Rust for Microbit](https://mb2.implrust.com/)** — a newer, more hands-on book specifically for v2
- **[microbit-v2 crate docs](https://crates.io/crates/microbit-v2)** — the main board support crate with lots of examples
