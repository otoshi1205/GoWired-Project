# Building and flashing

For the C++ firmware, see [`../main/BUILDING.md`](../main/BUILDING.md). This is
the Rust one.

1. [Toolchain](#1-toolchain)
2. [Configuring the module](#2-configuring-the-module)
3. [Build and flash with the tool](#3-build-and-flash-with-the-tool)
4. [Building by hand](#4-building-by-hand)
5. [Flashing](#5-flashing)
6. [Tests](#6-tests)
7. [When it first meets hardware](#7-when-it-first-meets-hardware)
8. [328PB and 328P-AU](#8-328pb-and-328p-au)

---

## 1. Toolchain

Two things, and the tool will install the first for you:

```sh
./tools/gowired-rs.py doctor --fix
```

### Rust nightly

AVR is a tier-3 target. That has one practical consequence: no prebuilt `core`
ships for it, so `core` has to be compiled from source with `-Z build-std`, and
that is nightly-only. The version is pinned in
[`rust-toolchain.toml`](rust-toolchain.toml) and cargo will use it automatically:

```sh
rustup toolchain install nightly-2026-07-27 --profile minimal --component rust-src
```

Pinned rather than floating because the AVR backend is the part of rustc most
likely to regress, and because the target spec itself changed recently:
`avr-unknown-gnu-atmega328` was replaced by `avr-none` plus an explicit
`-C target-cpu`. A toolchain either side of that needs different flags.

The firmware also uses three unstable features, all of them unavoidable on this
target: `asm_experimental_arch` (inline asm for a tier-3 architecture, needed for
`lpm`, `cli`/`sei` and `wdr`), `abi_avr_interrupt` (the interrupt calling
convention), and `panic-immediate-abort` (strips the panic machinery; worth over
2 kB, and there is nowhere to print a message to).

### avr-gcc

Rust does not ship a linker for AVR, so avr-gcc is it. Either:

```sh
sudo apt install gcc-avr avr-libc      # Debian, Ubuntu
```

or use the copy the Arduino IDE installs, which the tool finds on its own at
`~/.arduino15/packages/arduino/tools/avr-gcc/*/bin`. Nothing else from the
Arduino toolchain is needed -- no core, no libraries, no `arduino-cli`.

`avrdude` is needed only to flash.

---

## 2. Configuring the module

Everything is in
[`crates/gowired-firmware/src/config.rs`](crates/gowired-firmware/src/config.rs),
and it follows the project author's *Programowanie MCU* instructions. Here is
every documented setting and where it went:

| Original | Now | Notes |
| --- | --- | --- |
| `MY_NODE_ID` | `NODE_ID` | `AUTO_NODE_ID` asks the controller; the id is then remembered in EEPROM |
| `SN` | `sketch_name()` | a function, so the string stays in flash |
| `SV` | `sketch_version()` | |
| `MY_RS485_BAUD_RATE` | `RS485_BAUD` | |
| `MY_RS485_DE_PIN` | `RS485_DE_PIN` | |
| `MY_RS485_SOH_COUNT` | `RS485_SOH_COUNT` | |
| `MY_TRANSPORT_WAIT_READY_MS` | `TRANSPORT_WAIT_READY_MS` | |
| `MAX_CURRENT` | `POWER.max_current_a` | |
| `MVPERAMP` | `POWER.mv_per_amp` | 2SSR 185, 4RelayDin 73, RGBW 100 |
| `RECEIVER_VOLTAGE` | `POWER.receiver_voltage` | |
| `COSFI` | `POWER.cos_phi_percent` | a percentage now: 100 is unity |
| `POWER_MEASURING_TIME` | `POWER.measuring_time_ms` | |
| `MAX_TEMPERATURE` | `THERMAL.max_temperature_c` | |
| `DIMMING_STEP` | `DIMMER.step` | |
| `DIMMING_INTERVAL` | `DIMMER.interval_ms` | |
| `DIMMING_TOGGLE_STEP` | `DIMMER.toggle_step` | |
| `UP_TIME` | `SHUTTER.up_time_s` | |
| `DOWN_TIME` | `SHUTTER.down_time_s` | |
| `PS_OFFSET` | `SHUTTER.calibration_current_floor_ma` | milliamps now |
| `CALIBRATION_SAMPLES` | `SHUTTER.calibration_samples` | |
| `RS_AUTO_CALIBRATION` | — | always available; send `cmd1` to run it |
| `DOUBLE_RELAY` etc. | a cargo feature | exactly one; see below |
| `INPUT_1`..`INPUT_4` | `INPUT_SETTINGS[n].enabled` | any combination, including none |
| `PULLUP_X` | `INPUT_SETTINGS[n].pullup` | |
| `INVERT_X` | `INPUT_SETTINGS[n].invert` | |
| `POWER_SENSOR` | `POWER_SENSOR` | |
| `INTERNAL_TEMP` | `INTERNAL_TEMPERATURE` | |
| `EXTERNAL_TEMP` | `EXTERNAL_TEMPERATURE` | plus a `probe-*` feature |
| `HEATING_SECTION_SENSOR` | `HEATING_CONTROLLER_NODE` | 0 disables |
| `SPECIAL_BUTTON` | derived from the device kind | |
| `ENABLE_WATCHDOG` | `WATCHDOG` | |

Two notes on the original instructions:

- *"Only one output configuration may be active at a time."* That is now
  structural: it is a cargo feature and `config.rs` refuses to compile if the
  count is not exactly one.

- *"For an RGBW module at least one digital input must be active (INPUT_1)."*
  This does not apply to the C++ refactor or to this one -- both handle zero
  enabled inputs. The original constraint came from `NUMBER_OF_INPUTS` being a
  sum of possibly-undefined macros used as an array bound, so an undefined macro
  leaked through as an identifier. Enable inputs because you have something wired
  to them.

### Which board

Exactly one `device-*` feature. Cargo unions features, so picking a non-default
one needs `--no-default-features`:

```sh
cargo build --release --no-default-features --features device-roller-shutter
```

`FOUR_RELAY` has no thermometer -- its analog pins are the four current sensors --
so it also needs `INTERNAL_TEMPERATURE = false`. That is a compile error rather
than a silent misreading, and the tool sets it for you.

### External probe

`EXTERNAL_TEMPERATURE = true` plus one of `--features probe-sht30` or
`--features probe-dht22`. Unlike the C++ build, both drivers are always compiled
and the feature only chooses which one is named -- there is no third-party library
to install, because both are implemented here. See
[section 7](#7-when-it-first-meets-hardware) for how much to trust them.

---

## 3. Build and flash with the tool

```sh
./tools/gowired-rs.py                          # interactive
./tools/gowired-rs.py doctor --fix             # install what is missing
./tools/gowired-rs.py build -d dimmer          # one variant, with a size report
./tools/gowired-rs.py build -d rgbw --check-strings
./tools/gowired-rs.py flash -d roller-shutter  # detects the part, builds, flashes
./tools/gowired-rs.py flash -d rgb -n 7        # with a fixed node id
./tools/gowired-rs.py sizes                    # every variant, both parts
./tools/gowired-rs.py test                     # the host suite
```

Standard library only. It edits `config.rs` for the build and puts it back
afterwards, even if the build fails; pass nothing and it asks.

`--check-strings` verifies by disassembly that no presentation string ended up in
SRAM. That is worth having as a check rather than an assumption: the
[`gw_text!`](crates/gowired-core/src/text.rs) macro's whole purpose is an
*absence*, and a `Text` that had quietly become a plain `&str` would cost 380
bytes of RAM without any other symptom.

---

## 4. Building by hand

The AVR settings live in
[`crates/gowired-firmware/.cargo/config.toml`](crates/gowired-firmware/.cargo/config.toml),
and cargo reads that based on the working directory -- so build *from that
directory*:

```sh
cd crates/gowired-firmware
cargo build --release --no-default-features --features device-double-relay
```

For the 328PB, override the part:

```sh
RUSTFLAGS="-C target-cpu=atmega328pb" \
  cargo build --release --no-default-features --features device-double-relay
```

`RUSTFLAGS` takes precedence over the config file's `rustflags`, which is why the
tool uses it.

The output is `target/avr-none/release/gowired-firmware.elf`, at the *workspace*
root rather than the crate's. To measure it:

```sh
avr-size target/avr-none/release/gowired-firmware.elf
```

Running `cargo test` from the workspace root is unaffected by any of this: the
root has no `.cargo/config.toml`, and `default-members` excludes the two AVR
crates, so the host build never tries to compile register code or a `#![no_main]`
binary.

---

## 5. Flashing

Same as the C++ firmware: over ISP, not over serial. The author's *Wgrywanie
oprogramowania* instructions apply unchanged, minus the Arduino IDE parts.

1. **Bus power off** before fitting the MCU to the shield.
2. **Voltage jumper**: 5 V for an MCU module, 3.3 V for a Gateway.
3. **Programmer**: USBasp by default; `--programmer` for anything else.
4. Hold **CONF** while seating the MCU, per the shield-fitting procedure.

```sh
./tools/gowired-rs.py flash -d double-relay
```

With `-m auto` (the default) the tool reads the device signature and picks the
part: `0x1e950f` for the 328P, `0x1e9516` for the 328PB. That is the one decision
worth automating, because getting it wrong means either a refusal to write or the
wrong fuses.

There is **no bootloader involvement** and no `variant=modelP` / `variant=modelPB`
menu to get right -- the part shows up only as `-C target-cpu`, and the pin
numbering is a table in
[`gowired-avr/src/pins.rs`](crates/gowired-avr/src/pins.rs).

Fuses are not touched. If a board has never been programmed, set its 8 MHz
external-crystal fuses once with MiniCore or avrdude as before; nothing here
changes them.

### Flash budget

The tool refuses to flash a build that exceeds 32384 bytes, which is what a
bootloader leaves. Without one the whole 32768 is available; adjust
`FLASH_BUDGET` in the tool if that applies.

---

## 6. Tests

```sh
cargo test                 # from the workspace root
cargo test --lib fixed     # one module
cargo clippy --features testing
```

270 tests, no hardware. What they cover:

- **Domain**: the C++ suite's 166 tests, ported test for test -- inputs, the
  shutter position model, the dimmer ramp and colour parsing, all three device
  implementations, the module's presentation / fault latches / command channel.
- **Protocol**: message encoding checked byte-for-byte against frames generated by
  the C++ MySensors library (see [`tools/golden/`](tools/golden/)), ICSC framing
  including every rejection path, and the node driven end-to-end against a
  scripted gateway.
- **Fixed point**: the integer `f32` encoder against real `f32` division, over
  about 13000 values including every rounding tie.
- **Sensing**: ADC scaling, including the orderings that would otherwise overflow.

### Regenerating the golden frames

```sh
cd tools/golden
g++ -std=gnu++11 -I. -I ~/Arduino/libraries/MySensors -I ~/Arduino/libraries/MySensors/core \
    gen.cpp ~/Arduino/libraries/MySensors/core/MyMessage.cpp -o gen
./gen
```

The output is Rust source; paste it over the `GOLDEN` table in
`crates/gowired-core/src/tests/message.rs`.

---

## 7. When it first meets hardware

Nothing here has run on a chip. In rough order of what to check, and why that
order:

1. **Does it boot at all.** The startup sequence is
   `clear_watchdog_reset` → `init` → `node.begin()`. If the watchdog is left armed
   from a previous boot the node resets every 8 seconds; that is the failure mode
   to expect first, and it looks like a dead board.

2. **Is the time base right.** Everything else depends on it. Toggle a spare pin
   from the main loop and check the period against `TIMING.loop_time_ms` (80 ms).
   If it is out by a factor, `F_CPU` in
   [`gowired-avr/src/lib.rs`](crates/gowired-avr/src/lib.rs) does not match the
   fitted crystal -- the fractional-millisecond arithmetic in `clock.rs` assumes
   8 MHz.

3. **Does a frame come back.** The USART divisor is 16 at 57600 and 8 MHz, giving
   58824 baud -- 2.1% fast. That is inside 8N1 tolerance and it is exactly what
   the Arduino core computed for the C++ build, so both ends are wrong by the same
   amount as before. If nothing decodes, suspect the RS485 driver turnaround
   before the divisor: `Clock::delay_us` is calibrated by instruction count, not
   measured, and `flush()` waiting on `TXC0` rather than `UDRE0` is what stops the
   last stop bit being cut off.

4. **Do the readings look right.** Current and temperature scaling is in
   [`domain::sensing`](crates/gowired-core/src/domain/sensing.rs) and is unit
   tested, so a wrong reading points at `mv_per_amp`, `zero_voltage_mv`, or the
   pin map -- not at the arithmetic.

5. **The external probes, last.** The SHT30 path is the one to trust further: I2C
   acknowledges every byte and the reading carries a CRC, so it either works or
   returns `ChecksumError`. The DHT22 path is bit-banged and compares the two
   halves of each bit rather than using a fixed threshold, which should survive
   whatever the compiler does to the loop -- but it has never seen a sensor.

Both probes are off in the default configuration.

---

## 8. 328PB and 328P-AU

They are interchangeable as far as this firmware is concerned, and that is
checkable rather than hopeful. Every register it touches is at the same address on
both parts; only avr-libc's *names* differ, and only for the TWI block (`TWBR` on
the 328P, `TWBR0` on the 328PB, both at 0xB8). The two interrupt vectors used --
`TIMER0_OVF` at 16 and `USART_RX` at 18 -- are the same number on both. All of
that is verified against the toolchain's own `iom328p.h` and `iom328pb.h`, and the
table is in [`gowired-avr/src/regs.rs`](crates/gowired-avr/src/regs.rs).

So the part shows up in exactly one place: `-C target-cpu=atmega328p` or
`atmega328pb`. The 328PB build comes out 76 bytes larger.

The 328PB's extra peripherals -- a second USART, a second TWI, two more timers --
are simply unused.

What this does *not* tell you is whether the two parts are interchangeable on the
**board**: package pinout, the crystal load capacitors, and anything the shield
expects. That is a hardware question for the datasheets and the PCB, not a
firmware one.
