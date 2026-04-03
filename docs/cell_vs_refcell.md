# `Cell` vs `RefCell` for Shared Statics

## The rule

- Use **`Cell<T>`** when `T: Copy`
- Use **`RefCell<T>`** when `T` is not `Copy`

## Why it matters for statics

Statics shared between ISRs and `main` use the pattern:

```rust
static FOO: Mutex<Cell<T>>    // for Copy types
static FOO: Mutex<RefCell<T>> // for non-Copy types
```

## `Cell<T>` — for `Copy` types

`Cell` has no runtime borrow check. It works by value:

```rust
cell.set(new_value);   // replaces
let v = cell.get();    // copies out
```

Suitable for:

```rust
static BUTTONS_PRESSED: Mutex<Cell<(bool, bool)>> =
    Mutex::new(Cell::new((false, false)));

static APP_STATE: Mutex<Cell<AppState>> =
    Mutex::new(Cell::new(AppState::new()));
```

Both `(bool, bool)` and `AppState` derive `Copy`, so `Cell` works.

## `RefCell<T>` — for non-Copy types

`RefCell` adds interior mutability with a runtime borrow check, allowing
`&mut T` behind a shared `&T`.

```rust
cell.borrow()      // returns Ref<T>   — shared access
cell.borrow_mut()  // returns RefMut<T> — mutable access
```

`RefMut<T>` implements `DerefMut`, so you can write through it with `*`:

```rust
*GPIOTE_PERIPH.borrow(cs).borrow_mut() = Some(gpiote);
```

Necessary for peripherals like `Gpiote` which are not `Copy`:

```rust
static GPIOTE_PERIPH: Mutex<RefCell<Option<Gpiote>>> =
    Mutex::new(RefCell::new(None));
```

## The `Cell::new(None)` trap

`Cell<Option<Gpiote>>` compiles — `Cell::new()` has no `Copy` bound.
The bound is only on `Cell::get()`. The error surfaces later when you
try to read the value:

```rust
let g = GPIOTE_PERIPH.borrow(cs).get(); // ERROR: Gpiote is not Copy
```

So always use `RefCell` for non-`Copy` types, even if the declaration compiles.

## Naming

Avoid naming a static the same as a PAC interrupt variant. The `#[interrupt]`
macro registers functions by matching against the PAC `Interrupt` enum
(`GPIOTE = 6`, `RTC0 = 11`, etc.). A static named `GPIOTE` will clash.

```rust
// BAD — clashes with #[interrupt] fn GPIOTE()
static GPIOTE: Mutex<RefCell<Option<Gpiote>>> = ...;

// GOOD
static GPIOTE_PERIPH: Mutex<RefCell<Option<Gpiote>>> = ...;
```
