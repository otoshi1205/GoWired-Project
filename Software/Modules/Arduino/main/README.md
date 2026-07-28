# GoWired Module firmware

Firmware for the GoWired MCU (ATmega328P) driving the 2SSR, RGBW and 4RelayDin
shields. One sketch covers six board variants.

## Layout

```
main.ino              wiring only: constructs the HAL, picks a device, delegates
Configuration.h       everything you edit: transport macros + constexpr config
platform.local.txt    C++17 opt-in for the Arduino IDE (see Building)
src/hal/              interfaces: IGpio, IPwm, IClock, IStore, IBus, sensors
src/domain/           all behaviour. No Arduino headers. Unit tested.
src/platform/         ATmega328P + MySensors adapters. Not unit tested.
test/                 host test suite (GoogleTest)
```

The dependency rule is one-directional: `domain` may include `hal`, and nothing
in `hal` or `domain` may include `<Arduino.h>`, `<MySensors.h>` or anything from
`platform`. That is what lets the whole of `domain` compile and run on a host.

`<MySensors.h>` pulls its transport implementation in as source and so may
appear in only one translation unit. Keeping it behind `IBus`
(`src/platform/mysensors_bus.h`, included solely by `main.ino`) means the rest
of the sketch is not subject to that restriction.

## Choosing a variant

Set `GW_DEVICE` in [Configuration.h](Configuration.h) to one of
`GW_DOUBLE_RELAY`, `GW_ROLLER_SHUTTER`, `GW_FOUR_RELAY`, `GW_DIMMER`, `GW_RGB`,
`GW_RGBW`, then adjust the `cfg::` constants below it.

Six variants map onto three implementations of `IDevice`:

| `GW_DEVICE`         | class                 |
| ------------------- | --------------------- |
| `GW_DOUBLE_RELAY`   | `RelayBankDevice`     |
| `GW_FOUR_RELAY`     | `RelayBankDevice`     |
| `GW_ROLLER_SHUTTER` | `RollerShutterDevice` |
| `GW_DIMMER`         | `DimmerDevice`        |
| `GW_RGB`            | `DimmerDevice`        |
| `GW_RGBW`           | `DimmerDevice`        |

`main.ino` names exactly one of them, so the linker discards the code and
vtables of the variants you did not build. `Module` is written once against
`IDevice` regardless.

Misconfigurations are build errors rather than surprises: `Configuration.h`
`static_assert`s that no two children share a sensor id, and that
`GW_FOUR_RELAY` is not asked for an internal thermometer it has no pin for.

## Building

The easy way — [`tools/gowired.py`](tools/gowired.py) asks what you are building,
picks the board settings for you (including reading the MCU's signature so you
never choose between the 328P and 328PB by hand), builds, and flashes:

```sh
./tools/gowired.py                     # interactive: check, configure, build, flash
./tools/gowired.py doctor --fix        # install arduino-cli deps and MiniCore
./tools/gowired.py flash -p attic      # re-flash a saved configuration
```

It builds a staging copy, so your working tree is left untouched unless you pass
`--write-config`. Stdlib only, no pip install. See
[BUILDING.md section 3](BUILDING.md#3-configure-build-and-flash-with-the-tool).

The manual route, and full toolchain setup, is in **[BUILDING.md](BUILDING.md)**:

```sh
arduino-cli compile \
  --fqbn 'MiniCore:avr:328:clock=8MHz_external,variant=modelP,bootloader=uart0' .
```

Things that trip people up:

- The target is an **ATmega328P-AU (newer boards) or ATmega328PB (earlier ones),
  8 MHz external crystal**, programmed through [MiniCore](https://github.com/MCUdude/MiniCore)
  with an ISP programmer — not over serial. Set `variant=modelP` or
  `variant=modelPB` to match your board; that is the only difference between them.
- The sketch needs **C++17**, which MiniCore already provides. Do *not* force it
  with `--build-property compiler.cpp.extra_flags=...` on MiniCore — that is
  where its LTO flags live, and overriding them costs ~2 kB of flash.
- Four libraries are required — `MySensors`, `GoWired-lib`, `ADCTouch` and
  `PCF8575-lib` — but only the first two are used. Arduino compiles every source
  file of a library you include, and GoWired-lib bundles drivers this sketch does
  not need. Use `PCF8575-lib` specifically; the similarly named libraries by Rob
  Tillaart and xreef will not compile against it.

GoWired-lib supplies the ADC sampling in `PowerSensor` and `AnalogTemp` only. The
debounce, shutter and dimmer logic it also carries was moved into `src/domain` so
it could be tested; those classes are no longer used here.

Configuration follows the project author's official *Programowanie MCU*
instructions; [BUILDING.md section 2](BUILDING.md#2-configuring-the-module) maps
every documented setting onto its current name.

### Size, measured

ATmega328P-AU at 8 MHz, MiniCore, LTO on, bootloader reserved (32384 B usable):

| Variant | Flash before | after | SRAM before | after |
|---|---|---|---|---|
| DOUBLE_RELAY | 16448 | 20448 | 1313 | **1199** |
| ROLLER_SHUTTER | 18032 | 22596 | 1313 | **1209** |
| FOUR_RELAY | *did not compile* | 20588 | — | 1265 |
| DIMMER | 18450 | 21232 | 1511 | **1203** |
| RGB | 18502 | 21232 | 1515 | **1203** |
| RGBW | 18508 | 21232 | 1517 | **1203** |

SRAM is lower across the board; flash costs 2.7–4.6 kB more, worst case 70% of
the budget. See [BUILDING.md section 7](BUILDING.md#7-about-the-unused-library-dependencies)
for one easy kilobyte back.

## Tests

```sh
cmake -S test -B build-test
cmake --build build-test -j
./build-test/gowired_tests
```

155 tests. No hardware, no Arduino toolchain, no MySensors install: GoogleTest is
fetched by CMake on first configure, and `test/fakes.h` implements every HAL
interface, so the tests drive real domain code directly.

`FakeClock` advances on every `now_ms()` call, because the ported input and
dimmer code polls in blocking loops exactly as the original did; that is what
lets those loops terminate under test.
