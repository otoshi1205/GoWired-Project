# GoWired module firmware, in Rust

A rewrite of [`../main`](../main) for the ATmega328P-AU / ATmega328PB, with no
Arduino core, no MySensors library, and no C++.

One firmware covers six board variants. All behaviour is in a `no_std` crate that
runs on the host under `cargo test`; the register layer is 600 lines and the
wiring is one file.

```sh
./tools/gowired-rs.py            # interactive: check, configure, build, flash
cargo test                       # 270 tests, no hardware, no toolchain
```

## Layout

```
crates/gowired-core/     no_std, no registers, no protocol bytes on the wire
  src/hal.rs             traits the hardware must implement, and the units
  src/text.rs            string literals that stay in flash (PROGMEM shim)
  src/domain/            all behaviour. Six variants, three implementations.
  src/proto/             MySensors 2.x: message codec, RS485 link, node FSM
  src/tests/             the whole suite. Runs on the host.
crates/gowired-avr/      ATmega328P/PB registers. The only untestable part.
crates/gowired-firmware/ the binary
  src/config.rs          everything you edit
  src/main.rs            wiring: names one device, runs the loop
tools/gowired-rs.py      configure, build, flash, measure
tools/golden/            generates the reference frames the codec is tested against
```

The dependency rule is one-directional and enforced by the crate graph rather
than by convention: `gowired-core` cannot reach a register because it does not
depend on `gowired-avr`. That is what the C++ version's `src/hal` / `src/domain` /
`src/platform` directory split was asking for, made structural.

## What changed, and why

### No MySensors library

The C++ build used the MySensors Arduino library: 51000 lines, of which a leaf
node on a wired bus needs a small, well-specified subset. There is no Rust port,
and binding to the C++ one would mean keeping the Arduino core and the
single-translation-unit restriction that `IBus` existed to work around.

So [`proto`](crates/gowired-core/src/proto/) implements the subset: the V2 message
header, the ICSC framing the RS485 transport uses, node-id assignment,
presentation, `C_SET`/`C_REQ`, echo requests, and the `I_PING` / `I_HEARTBEAT` /
`I_DISCOVER` housekeeping a controller expects an answer to. About 900 lines, all
of it host-tested.

**The encoder is checked byte-for-byte against the C++ library's own output.**
Not against the specification -- against frames produced by compiling
MySensors' `MyMessage.cpp` and dumping its memory. See
[`tools/golden/`](tools/golden/). That is what decides whether a controller which
is already paired with a module keeps working.

Deliberately absent, and listed in
[`proto::node`](crates/gowired-core/src/proto/node.rs): **message signing**,
**OTA firmware update** (the C++ build had `MY_OTA_FIRMWARE_FEATURE` on), and
routing/repeating. The first two are omitted rather than approximated; a
half-working OTA implementation bricks nodes. Flashing is over ISP either way.

### No floating point

The measurement chain works in milliamps and tenths of a degree, not `f32`.

This is not stylistic. Rust supplies AVR's soft-float from
`compiler_builtins`, in generic Rust rather than the hand-written assembly
avr-libc ships:

| routine | bytes |
| --- | --- |
| `__addsf3` | 2180 |
| `__divsf3` | 2072 |
| `__mulsf3` | 1802 |
| `__cmpsf2` | 196 |

There is no supported way to make the linker prefer avr-libc's versions any more,
and with them linked in the `ROLLER_SHUTTER` variant came to **32840 bytes: it did
not fit**. Not referencing them at all brought it to 26002.

MySensors still wants an `f32` on the wire for `V_TEMP` and `V_WATT`, so
[`proto::fixed`](crates/gowired-core/src/proto/fixed.rs) *constructs* one with
integer long division -- correctly rounded, and checked against real `f32`
division over about 13000 values.

An ADC count at 185 mV/A is 26 mA, so a milliamp is already forty times finer than
the hardware resolves. Nothing measurable was given up.

### No vtables, and no `static mut`

Every seam is a generic parameter over
[`hal::Platform`](crates/gowired-core/src/hal.rs), so the firmware's one choice of
hardware is resolved at compile time. The C++ version reached the same end by
relying on the linker to discard the vtables of the five variants it did not
build; here there is nothing to discard.

Every object -- transport, node, device, module -- lives in `main`'s stack frame,
so the borrows are checked and nothing needs a `static mut`. That frame is the
firmware's largest single SRAM cost and the tool reports it.

## Choosing a variant

Which board it is, is a cargo feature, because it decides which of the three
device implementations is compiled at all:

| feature | class |
| --- | --- |
| `device-double-relay` | `RelayBankDevice` |
| `device-four-relay` | `RelayBankDevice` |
| `device-roller-shutter` | `RollerShutterDevice` |
| `device-dimmer` | `DimmerDevice` |
| `device-rgb` | `DimmerDevice` |
| `device-rgbw` | `DimmerDevice` |

Everything else is a `const` in
[`config.rs`](crates/gowired-firmware/src/config.rs): pin map, current limit,
travel times, which inputs are enabled. Misconfigurations are build errors --
`config.rs` asserts that no two children share a sensor id, and that
`FOUR_RELAY` is not asked for a thermometer it has no pin for.

`tools/gowired-rs.py` sets the feature and the `-C target-cpu` for you, and reads
the device signature off the board so the 328P / 328PB choice never has to be
remembered.

## Size, measured

Every variant, both parts, against the 32384 bytes a bootloader leaves:

| variant | 328P flash | 328PB flash | static SRAM | `main` frame |
| --- | --- | --- | --- | --- |
| `double-relay` | 22860 | 22936 | 148 | 762 |
| `roller-shutter` | 26002 | 26078 | 150 | 767 |
| `four-relay` | 22872 | 22948 | 142 | 796 |
| `dimmer` | 24056 | 24132 | 182 | 742 |
| `rgb` | 24056 | 24132 | 182 | 742 |
| `rgbw` | 24056 | 24132 | 182 | 742 |

Worst case is 81% of flash. SRAM in use is static plus `main`'s frame -- around
950 bytes of 2048 -- plus the frames of the four outlined functions and the two
interrupt handlers, which is a few hundred more. Regenerate with
`./tools/gowired-rs.py sizes`.

For comparison, the C++ build of the same six variants was 20448--22596 bytes of
flash and 1199--1265 bytes of static SRAM. This is 2--3.5 kB more flash, from
Rust's integer codegen and its 1.1 kB `u32` division routine where avr-libc's is
68 bytes; and rather less SRAM, because the strings are in flash and the protocol
buffers are sized rather than assumed.

## Tests

```sh
cargo test
```

270 tests. No hardware, no AVR toolchain, no MySensors install, no gateway.

They are a port of the C++ suite's 166, test for test, plus coverage that suite
had no reason to have: the protocol (the C++ build got that from a library), the
fixed-point encoder, and the ADC scaling -- which in the C++ build lived in
GoWired-lib next to the sampling loop and so could not be exercised without an
ADC. That last one immediately earned its keep: it caught an overflow where a
mistyped `mv_per_amp` would have reported a *small* current in the middle of an
overload.

## What has not been verified

No part of this has run on a chip. There was no programmer and no board.

- The domain logic and the protocol encoding are tested thoroughly, and the
  encoding is checked against the C++ library's own bytes.
- The register layer's addresses and interrupt vector numbers are checked against
  the toolchain's own `iom328p.h` and `iom328pb.h`, and every variant compiles
  and links for both parts.
- The `lpm` flash-string path is verified by disassembly: `avr-nm` shows no
  presentation string in `.data`, and `gowired-rs.py build --check-strings`
  re-checks it.
- **Nothing has been flashed, and no frame has been put on real wire.** The
  timing-critical parts -- the USART baud divisor, the RS485 driver turnaround,
  the DHT22 bit-banging -- are derived from datasheets, not measured.

[BUILDING.md](BUILDING.md) says what to check first when it does meet hardware.
