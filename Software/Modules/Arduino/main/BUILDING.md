# Building, flashing and testing

Two independent builds live in this folder:

- the **firmware**, cross-compiled for the ATmega328P/PB with `arduino-cli` and MiniCore
- the **unit tests**, compiled natively with your host compiler and CMake

You only need the Arduino toolchain for the first. If you just want to run the
tests, skip to [Unit tests](#6-unit-tests).

Every command below was run on Ubuntu 22.04 (x86-64). Paths differ on
macOS/Windows where noted.

---

## 1. Firmware toolchain

### 1.1 Install arduino-cli

The official installer drops a single static binary. Pick a directory on your
`PATH`:

```sh
mkdir -p ~/.local/bin
curl -fsSL https://raw.githubusercontent.com/arduino/arduino-cli/master/install.sh \
  | BINDIR=~/.local/bin sh
arduino-cli version
```

Verified with `arduino-cli 1.5.1`.

### 1.2 Install MiniCore

The GoWired MCU runs at **8 MHz on an external crystal**. Which ATmega it is
depends on the board revision:

| Board revision | MCU | MiniCore *Variant* |
| --- | --- | --- |
| Earlier boards | ATmega328PB | `328PB` (`variant=modelPB`) |
| Newer boards | ATmega328P-AU | `328P / 328PA` (`variant=modelP`) |

That is the only setting that differs between them — see
[section 4](#4-compiling-by-hand). Both are TQFP-32 parts, which matters: this sketch uses
A6/A7 for the current sensor and the internal thermometer, and those two ADC
inputs exist only in the 32-pin package. A DIP-28 ATmega328P will not work.

The stock `arduino:avr` core supports neither the 328PB variant nor an 8 MHz
external clock on a 328P out of the box, so the project uses
[MiniCore](https://github.com/MCUdude/MiniCore).

Add MiniCore's index and install it:

```sh
arduino-cli core install MiniCore:avr \
  --additional-urls https://mcudude.github.io/MiniCore/package_MCUdude_MiniCore_index.json
arduino-cli core list
```

To avoid repeating the URL, put it in your config instead:

```sh
arduino-cli config init
arduino-cli config add board_manager.additional_urls \
  https://mcudude.github.io/MiniCore/package_MCUdude_MiniCore_index.json
```

In the **Arduino IDE**, paste that same URL into *File → Preferences →
Additional Boards Manager URLs*, then install "MiniCore" from *Tools → Board →
Boards Manager*. MiniCore's own
[installation notes](https://github.com/MCUdude/MiniCore#how-to-install) cover
this in more detail.

MiniCore brings its own toolchain, `avrdude` and the `ctags` build arduino-cli
uses to preprocess `.ino` files — nothing else is needed.

> Installing MiniCore also works when `arduino-cli core install arduino:avr`
> fails with `Error verifying signature: signature expired`: only Arduino's
> *official* package index is signature-checked, and third-party indexes are not.
> If you need the stock AVR core anyway, see
> [Appendix A](#appendix-a-installing-the-stock-avr-core-without-the-package-index).

### 1.3 Install the libraries

Four are required. Only two are actually *used* by this sketch; the other two
exist because Arduino compiles **every** `.cpp` in a library you include, and
`GoWired-lib` carries drivers this sketch does not need. See
[section 8](#8-about-the-unused-library-dependencies).

```sh
arduino-cli lib install "MySensors"
arduino-cli lib install "ADCTouch"
arduino-cli lib install "PCF8575-lib"
```

| Library                                                          | Why                                                      |
| ---------------------------------------------------------------- | -------------------------------------------------------- |
| [MySensors](https://github.com/mysensors/MySensors)              | RS485 transport and the message protocol. Used.           |
| [GoWired-lib](https://github.com/GoWired/GoWired-lib)            | ADC sampling for `PowerSensor` / `AnalogTemp`. Used.       |
| [ADCTouch](https://github.com/martin2250/ADCTouch)               | Needed only to compile `GoWired-lib`'s `CommonIO.cpp`.    |
| [PCF8575-lib](https://github.com/feanor-anglin/PCF8575-lib)      | Needed only to compile `GoWired-lib`'s `ExpanderIO.cpp`.  |

Note `PCF8575-lib` specifically — it is the GoWired author's own fork. The
similarly named `PCF8575` (Rob Tillaart) and `PCF8575 library` (xreef) will
**not** work; `ExpanderIO.cpp` declares a default-constructed `PCF8575 Expander;`
and those libraries have no default constructor.

`GoWired-lib` is not in the Arduino library index, so clone it into your sketchbook:

```sh
git clone https://github.com/GoWired/GoWired-lib.git \
  ~/Arduino/libraries/GoWired-lib
```

Only if you enable an external probe in `Configuration.h`, add one of:

```sh
arduino-cli lib install "arduino-sht"   # for GW_PROBE_SHT30
arduino-cli lib install "DHTlib"        # for GW_PROBE_DHT22
```

### 1.4 C++17

This sketch needs C++17. **MiniCore already compiles at `-std=gnu++17`**, so
there is nothing to do — neither in the IDE nor on the command line.

This only becomes a problem if you build with the stock `arduino:avr` core
instead, which still uses `-std=gnu++11`. In that case see
[Appendix C](#appendix-c-c17-on-the-stock-arduino-avr-core).

> Do **not** work around it with
> `--build-property compiler.cpp.extra_flags=-std=gnu++17` on MiniCore. That
> property is where MiniCore's LTO menu puts `-flto`, so overriding it silently
> turns link-time optimisation off and costs about 2 kB of flash.

---

## 2. Configuring the module

All configuration is in [Configuration.h](Configuration.h). This section follows
the project author's official *Programowanie MCU* instructions step by step and
gives the current equivalent of each setting.

The settings the author documents all still exist and still do the same thing.
Most were **renamed**: they are now `constexpr` values rather than `#define`s, so
the compiler type-checks them and mistakes such as two children sharing a sensor
id become build errors. [Section 2.6](#26-conformance-with-the-official-instructions)
maps every documented name onto its current one.

Lines not discussed here should be left alone.

### 2.1 Module identification

Unchanged — still macros, because MySensors reads them itself.

```c
#define MY_NODE_ID 1                  // unique per module
#define SN "GetWired 2SSR Module"     // sketch name (optional, keep it short)
#define SV "1.0"                      // firmware version (optional)
```

`MY_NODE_ID` must be unique: two modules with the same id cannot share a
gateway. The shipped default is `AUTO`, which lets the gateway assign one; the
official instructions recommend setting it explicitly so the id survives a
re-pairing.

### 2.2 General definitions

| Official | Now | Meaning |
| --- | --- | --- |
| `MAX_CURRENT` | `cfg::kPower.max_current_a` | Amps. 3 for 2SSR, 10 for RGBW / 4RelayDin. |
| `MVPERAMP` | `cfg::kPower.mv_per_amp` | Current-sensor sensitivity. ACS712-5A → 185, ACS712-20A → 100. |
| `RECEIVER_VOLTAGE` | `cfg::kPower.receiver_voltage` | Load voltage: 230 for 2SSR, 12 or 24 for RGBW. |
| `DIMMING_STEP` | `cfg::kDimmer.step` | Experiment for different RGBW transition effects. |
| `DIMMING_INTERVAL` | `cfg::kDimmer.interval_ms` | As above. |
| `UP_TIME` | `cfg::kShutter.up_time_s` | Shutter travel time up, seconds. |
| `DOWN_TIME` | `cfg::kShutter.down_time_s` | Shutter travel time down, seconds. |

```cpp
constexpr gw::PowerTuning kPower = {
    /* max_current_a     */ 3,
    /* receiver_voltage  */ 230,
    /* cos_phi           */ 1.0f,
    /* measuring_time_ms */ 20,
    /* mv_per_amp        */ 185,
};

constexpr gw::ShutterTuning kShutter = {
    /* up_time_s                 */ 21,
    /* down_time_s               */ 20,
    /* calibration_current_floor */ 0.2f,  // was PS_OFFSET
    /* calibration_samples       */ 1,     // was CALIBRATION_SAMPLES
};
```

> **On shutter auto-calibration.** The official text wraps `CALIBRATION_SAMPLES`
> / `PS_OFFSET` in an `RS_AUTO_CALIBRATION` toggle and notes that the feature
> does not work yet. There is no such toggle here — it had already been dropped
> from the repository before this refactor. What exists instead: `UP_TIME` /
> `DOWN_TIME` are always used as the defaults, and a calibration run can be
> triggered at runtime by sending `cmd1` to the configuration child (id 20).
> Measured times are stored in EEPROM and used in preference to the configured
> ones from then on. Calibration needs the current sensor to detect the end
> stops, so it is refused when `kPowerSensor` is `false`.

### 2.3 Output configuration

The author's rule — *only one output may be active at a time* — is now
structural rather than a convention you have to honour. Instead of commenting
out five of six `#define`s, set one selector:

```c
#define GW_DEVICE GW_DOUBLE_RELAY
```

| Official `#define` | Now |
| --- | --- |
| `DOUBLE_RELAY` | `#define GW_DEVICE GW_DOUBLE_RELAY` |
| `ROLLER_SHUTTER` | `#define GW_DEVICE GW_ROLLER_SHUTTER` |
| `DIMMER` | `#define GW_DEVICE GW_DIMMER` |
| `RGB` | `#define GW_DEVICE GW_RGB` |
| `RGBW` | `#define GW_DEVICE GW_RGBW` |
| *(not in the official text)* | `#define GW_DEVICE GW_FOUR_RELAY` — 4RelayDin |

Selecting two outputs at once is no longer expressible, so the old
`#error "Exactly one of ... must be defined!"` guard is gone.

`GW_FOUR_RELAY` must also set `kInternalTemperature = false` — the 4RelayDin
shield has no thermistor, because its analog pins carry the four current
sensors. A `static_assert` will tell you if you forget.

### 2.4 Input configuration

All inputs may be active at the same time, as before.

| Official | Now |
| --- | --- |
| `INPUT_1` … `INPUT_4` | `cfg::kInputSettings[n].enabled` |
| `PULLUP_n` (comment out to change the input's behaviour) | `cfg::kInputSettings[n].pullup` |
| `INVERT_n` | `cfg::kInputSettings[n].invert` |
| `POWER_SENSOR` | `cfg::kPowerSensor` |
| `INTERNAL_TEMP` | `cfg::kInternalTemperature` |
| `EXTERNAL_TEMP` | `cfg::kExternalTemperature` plus `GW_PROBE_DHT22` or `GW_PROBE_SHT30` |

```cpp
constexpr InputSetting kInputSettings[gw::kMaxInputs] = {
    //             enabled  pullup  invert
    /* INPUT_1 */ {true,    true,   false},
    /* INPUT_2 */ {true,    true,   false},
    /* INPUT_3 */ {true,    true,   false},
    /* INPUT_4 */ {false,   true,   false},   // INPUT_4 not wired
};

constexpr bool kPowerSensor = true;
constexpr bool kInternalTemperature = true;
constexpr bool kExternalTemperature = false;
```

`pullup = true` is `INPUT_PULLUP`, for a dry contact switching to ground.
`pullup = false` is plain `INPUT`, for a sensor that drives the line itself —
that is the variant the official text describes as commenting out `PULLUP_n`.

Three things the author's text specifies by hand are now derived, so you cannot
set them inconsistently:

- **`INPUT_ID_n`** — the child id, now `first_input_id(GW_DEVICE) + n`. That is
  2, 3, 4, 5 for every variant except `GW_FOUR_RELAY`, which uses 21–24 because
  ids 4–7 carry its per-relay power sensors. (The id in the official example,
  `INPUT_ID_1 4`, predates the current layout.)
- **`PIN_n`** — now `cfg::input_pin(n)`, taken from the pin map in section 5 of
  `Configuration.h`. Inputs 1–4 are on `INPUT_PIN_3`…`INPUT_PIN_6`.
- **`NUMBER_OF_INPUTS`** — now counted from the `enabled` flags.

Disabling an input in the middle does **not** renumber the ones after it: each
slot keeps its own child id, so turning off `INPUT_2` leaves `INPUT_3` on id 4
rather than shifting it to 3 under a controller already bound to it.

> **On the RGBW / `INPUT_1` requirement.** The official text notes that an RGBW
> module needs at least one digital input active. That no longer applies. The
> dimmer's two wall switches are owned by the dimmer itself and sit on
> `INPUT_PIN_1`/`INPUT_PIN_2`, independent of the four general-purpose inputs on
> `INPUT_PIN_3`…`INPUT_PIN_6`. An RGBW build with all four disabled compiles and
> runs; it simply has no general-purpose inputs.

### 2.5 External temperature sensor

The official text supports DHT22 only. SHT30 works too — pick one in
`Configuration.h`:

```c
#define GW_PROBE_SHT30      // or GW_PROBE_DHT22
```

and set `cfg::kExternalTemperature = true`. Install the matching library
(`arduino-sht` or `DHTlib`).

### 2.6 Conformance with the official instructions

Everything the official instructions describe is still configurable and still
behaves the same way. Renames only:

| Official name | Current location | Status |
| --- | --- | --- |
| `MY_NODE_ID` | same | unchanged |
| `SN`, `SV` | same | unchanged |
| `MAX_CURRENT` | `cfg::kPower.max_current_a` | renamed |
| `MVPERAMP` | `cfg::kPower.mv_per_amp` | renamed |
| `RECEIVER_VOLTAGE` | `cfg::kPower.receiver_voltage` | renamed |
| `DIMMING_STEP` | `cfg::kDimmer.step` | renamed |
| `DIMMING_INTERVAL` | `cfg::kDimmer.interval_ms` | renamed |
| `UP_TIME`, `DOWN_TIME` | `cfg::kShutter.up_time_s` / `.down_time_s` | renamed |
| `CALIBRATION_SAMPLES` | `cfg::kShutter.calibration_samples` | renamed |
| `PS_OFFSET` | `cfg::kShutter.calibration_current_floor` | renamed |
| `RS_AUTO_CALIBRATION` | — | **absent**; runtime `cmd1` instead (see 2.2) |
| `DOUBLE_RELAY` … `RGBW` | `GW_DEVICE` | one selector replaces six flags |
| `INPUT_1` … `INPUT_4` | `cfg::kInputSettings[n].enabled` | renamed |
| `PULLUP_n` | `cfg::kInputSettings[n].pullup` | renamed |
| `INVERT_n` | `cfg::kInputSettings[n].invert` | renamed |
| `INPUT_ID_n` | derived | now computed |
| `PIN_n` | `cfg::input_pin(n)` | now computed |
| `NUMBER_OF_INPUTS` | derived | now computed |
| `POWER_SENSOR` | `cfg::kPowerSensor` | renamed |
| `INTERNAL_TEMP` | `cfg::kInternalTemperature` | renamed |
| `EXTERNAL_TEMP` | `cfg::kExternalTemperature` + `GW_PROBE_*` | renamed, SHT30 added |

Default values that differ from the official examples — all of them inherited
from the repository, not introduced here:

| Setting | Official example | Repository / current |
| --- | --- | --- |
| `MY_NODE_ID` | `1` | `AUTO` |
| `DIMMING_INTERVAL` | `10` | `1` |
| `PS_OFFSET` | `0.5` | `0.2` |
| `NUMBER_OF_INPUTS` | `1` | all four inputs enabled |

### 2.7 Original text

Reproduced verbatim for reference.

<details>
<summary>Programowanie MCU (oryginał)</summary>

Po podłączeniu całego zestawu można przejść do programowania modułów. Najpierw
pobierz kod z naszego repozytorium GitHub. Konfiguracja oprogramowania odbywa
się poprzez edycję pliku `Configuration.h`. Żeby zrozumieć na czym to polega,
przyjrzyjmy się kilku linijkom tego pliku. Linijki, których nie przeanalizujemy
nie są w tym momencie potrzebne i powinny pozostać niezmienione.

**Identyfikacja modułu**

Pierwsze zmiany w pliku `Configuration.h` dotyczą identyfikacji danego modułu:

- `MY_NODE_ID` – unikalny numer modułu (każdy moduł musi mieć Node ID; nie można
  podłączyć dwóch modułów o tym samym numerze do jednego Gateway'a),
- `SN` (sketch name) – nazwa modułu (nieobowiązkowa, najlepiej, żeby była krótka
  i wyróżniająca),
- `SV` (sketch version) – numer wersji firmware (nieobowiązkowy).

```c
// Identification
#define MY_NODE_ID 1
#define SN "GetWired 2SSR Module"
#define SV "1.0"
```

**Definicje**

Należy również podać kilka ogólnych definicji:

- `MAX_CURRENT` – wartość w amperach (należy ustawić 3 dla 2SSR i 10 dla RGBW),
- `MVPERAMP` – czułość czujnika prądu (185 dla 2SSR, 100 dla RGBW),
- `RECEIVER_VOLTAGE` – napięcie pracy podłączonego odbiornika (230 dla 2SSR, 12
  lub 24 dla RGBW),
- `DIMMING_STEP`, `DIMMING_INTERVAL` – poeksperymentuj z tymi wartościami w celu
  uzyskania odmiennych efektów przejścia w module RGBW,
- `UP_TIME`, `DOWN_TIME` – czas ruchu w górę i w dół dla rolety w sekundach
  (funkcja autokalibracji na razie jeszcze nie działa); ustaw w przypadku
  podłączenia rolety do modułu 2SSR.

```c
// Power Sensor
#define MAX_CURRENT 3           // 2SSR - 3; RGBW, 4RelayDin - 10 [A]
#define MVPERAMP 185            // ACS7125A: 185 mV/A; ACS71220A: 100 mV/A
#define RECEIVER_VOLTAGE 230    // 230V, 24V, 12V - values for power usage calculation

// Dimmer
#define DIMMING_STEP 1
#define DIMMING_INTERVAL 10

// Roller Shutter
//#define RS_AUTO_CALIBRATION
#ifdef RS_AUTO_CALIBRATION
  #define CALIBRATION_SAMPLES 2
  #define PS_OFFSET 0.5
#else
  #define UP_TIME 21
  #define DOWN_TIME 20
#endif
```

**Konfiguracja wyjścia**

To jest właściwie dość ważny moment – należy wybrać wyjście danego modułu.
Należy pamiętać, że tylko jedno wyjście może być aktywne na raz.

- `DOUBLE_RELAY` – odkomentuj, jeśli chcesz używać modułu 2SSR jako 2-kanałowego
  sterownika oświetlenia,
- `ROLLER_SHUTTER` – odkomentuj, jeśli chcesz używać modułu 2SSR jako sterownika
  rolety,
- `DIMMER`, `RGB`, `RGBW` – odkomentuj jedno z tych wyjść w zależności od
  rodzaju taśmy LED, którą chcesz kontrolować modułem RGBW.

```c
// 2SSR
#define DOUBLE_RELAY
//#define ROLLER_SHUTTER

// Dimmer / RGB / RGBW
//#define DIMMER
//#define RGB
//#define RGBW
```

**Konfiguracja wejść**

Poniższe parametry umożliwiają skonfigurowanie wejść obsługiwanych przez moduł.
Wszystkie mogą być aktywne jednocześnie:

- `INPUT_1` – `INPUT_4` – cyfrowe wejścia o różnorodnym przeznaczeniu (możesz
  wybrać wariant działania danego wejścia zakomentowując parametr `PULLUP_X`),
- `POWER_SENSOR` – zakomentuj, jeśli nie chcesz korzystać z wbudowanego czujnika
  prądu,
- `INTERNAL_TEMP` – zakomentuj, jeśli nie chcesz korzystać z wbudowanego
  czujnika temperatury,
- `EXTERNAL_TEMP` – odkomentuj, jeśli chcesz zastosować zewnętrzny czujnik
  temperatury (obecnie jest wspierane jedynie DHT22).

Ważne: w przypadku modułu RGBW przynajmniej jedno cyfrowe wejście musi być
aktywne (`INPUT_1`).

```c
// Digital input
#define INPUT_1
#ifdef INPUT_1
  #define INPUT_ID_1 4
  #define PIN_1 INPUT_PIN_3
  #define PULLUP_1
  #define NUMBER_OF_INPUTS 1
#endif

// ACS712 Power Sensor
#define POWER_SENSOR

// Analog Internal Thermometer
#define INTERNAL_TEMP

// 1wire external thermometer (e.g. DHT22)
//#define EXTERNAL_TEMP
```

</details>

---

## 3. Configure, build and flash with the tool

[`tools/gowired.py`](tools/gowired.py) does everything in sections 2, 3.x and 4
for you. Standard library only — no `pip install`.

```sh
./tools/gowired.py
```

That checks the toolchain, offers to install anything missing, asks what you are
building, detects the MCU, builds, and flashes.

It exists to remove the settings you would otherwise have to remember:

- **MCU variant.** `--mcu auto` (the default) reads the device signature through
  the programmer — `0x1e950f` is a 328P, `0x1e9516` a 328PB — and picks the
  MiniCore variant itself. You never choose between `modelP` and `modelPB`.
- **Couplings the firmware asserts on.** Selecting `four_relay` switches the
  internal thermometer off, because that shield has none; selecting a probe sets
  both `kExternalTemperature` *and* the matching `GW_PROBE_*` macro.
- **The LTO trap.** It never touches `compiler.cpp.extra_flags`, so it cannot
  silently disable link-time optimisation (see [section 1.4](#14-c17)).
- **Clean working tree.** Configuration is applied to a staging copy in `/tmp`,
  so `git status` stays clean. Pass `--write-config` if you *do* want the settings
  written into your `Configuration.h`.

### Commands

| Command | Does |
| --- | --- |
| `gowired.py` | interactive: doctor → configure → build → flash |
| `gowired.py doctor [--fix]` | check, and optionally install, the toolchain and libraries |
| `gowired.py detect` | identify the connected MCU |
| `gowired.py build` | configure and build |
| `gowired.py flash` | configure, build and upload |
| `gowired.py test` | run the host unit tests |
| `gowired.py list` | show devices, MCUs, probes and saved profiles |
| `gowired.py profile save NAME` | store a configuration for reuse |

### Non-interactive use

Every setting has a flag, so this works from a script or CI:

```sh
./tools/gowired.py build -d rgbw -m 328p --node-id 21 \
    --max-current 10 --mv-per-amp 100 --receiver-voltage 24 \
    --inputs 1,2 --no-internal-temp

./tools/gowired.py build -d roller_shutter --up-time 30 --down-time 28
./tools/gowired.py build -d double_relay --probe sht30 --heating-node 1
```

`-d/--device` takes `double_relay`, `roller_shutter`, `four_relay`, `dimmer`,
`rgb`, `rgbw`. `--inputs` takes `all`, `none`, or a list like `1,2,4`. Boolean
features have `--x` / `--no-x` pairs. `gowired.py build --help` lists them all.

### Profiles

If you flash the same few configurations repeatedly, save them:

```sh
./tools/gowired.py build -d roller_shutter --node-id 12 --up-time 30 --save attic
./tools/gowired.py flash -p attic          # later, one command
./tools/gowired.py profile show attic
```

Profiles live in `tools/profiles.json`, which is plain JSON and safe to commit if
you want the whole installation described in the repository.

### Flashing with it

```sh
./tools/gowired.py flash -p attic --test
```

`--test` runs the host unit tests first and refuses to flash if they fail. The
tool prints the voltage-jumper reminder before uploading and the CONF-button
sequence afterwards. If the upload fails it lists the usual causes (USB
permissions, jumper, orientation).

> Verified while writing this: doctor (including installing arduino-cli and
> MiniCore from scratch), configure, build, profiles, and the upload path as far
> as avrdude — which runs, is handed a valid command line, and stops only at
> `cannot find USB device ... USBasp`. **The upload itself was never completed**,
> because no programmer or board was attached. See [section 5](#5-flashing).

---

## 4. Compiling by hand

### Board settings

The official instructions give these settings, which are what the FQBNs below
encode:

The official instructions give these settings:

| Setting | MCU | Gateway |
| --- | --- | --- |
| Board | ATmega328 | ATmega328 |
| Clock | 8 MHz external | 8 MHz external |
| Variant | 328PB *or* 328P/328PA — match your board | as the MCU |
| Bootloader | Yes | No |

In the Arduino IDE, select *Tools → Board → MiniCore → ATmega328* and set those
four from the *Tools* menu.

### Command line

From this directory — pick the `variant` that matches your MCU:

```sh
# ATmega328P-AU (newer boards)
arduino-cli compile \
  --fqbn 'MiniCore:avr:328:clock=8MHz_external,variant=modelP,bootloader=uart0' .

# ATmega328PB (earlier boards)
arduino-cli compile \
  --fqbn 'MiniCore:avr:328:clock=8MHz_external,variant=modelPB,bootloader=uart0' .
```

Expected output shape (DOUBLE_RELAY on a 328P-AU):

```
Sketch uses 20448 bytes (63%) of program storage space. Maximum is 32384 bytes.
Global variables use 1199 bytes (58%) of dynamic memory, leaving 849 bytes ...
```

The FQBN maps onto the table above:

| Table row | FQBN fragment |
| --- | --- |
| Board: ATmega328 | `MiniCore:avr:328` |
| Clock: 8 MHz external | `clock=8MHz_external` |
| Variant: 328P / 328PA | `variant=modelP` |
| Variant: 328PB | `variant=modelPB` |
| Bootloader: Yes | `bootloader=uart0` |
| Bootloader: No (Gateway) | `bootloader=no_bootloader` |

A 328PB build is about 2 kB larger and uses ~80 bytes more SRAM than the same
sketch built for a 328P, because MiniCore adds the 328PB's second USART and TWI.
This firmware uses neither, so nothing is lost by moving to the 328P.

Getting the clock wrong is the dangerous one: an otherwise clean build for
16 MHz running on an 8 MHz crystal halves the RS485 baud rate and doubles every
delay, so the node simply never talks to the gateway. `arduino-cli board details
-b MiniCore:avr:328` lists every option.

`bootloader=no_bootloader` also reclaims the 512 bytes MiniCore's bootloader
reserves, which is why the Gateway build has slightly more room.

### Checking size for every variant

Handy when you change shared code and want to be sure nothing overflowed:

```sh
FQBN='MiniCore:avr:328:clock=8MHz_external,variant=modelPB,bootloader=uart0'
for V in GW_DOUBLE_RELAY GW_ROLLER_SHUTTER GW_FOUR_RELAY GW_DIMMER GW_RGB GW_RGBW; do
  sed -i "s/^#define GW_DEVICE .*/#define GW_DEVICE $V/" Configuration.h
  printf '%-18s ' "$V"
  arduino-cli compile --fqbn "$FQBN" . 2>&1 \
    | grep -oE 'Sketch uses [0-9]+|Global variables use [0-9]+' | tr '\n' ' '
  echo
done
```

Remember to set `GW_DEVICE` back to the variant you actually want, and to flip
`kInternalTemperature` for `GW_FOUR_RELAY`.

---

## 5. Flashing

> **Not fully verified here.** No programmer or board was attached to the machine
> these instructions were written on. The command lines are known to be
> well-formed — avrdude accepts them and fails only on the absent USBasp — but no
> firmware was actually written to a chip, and the fuse and voltage-jumper steps
> are transcribed from the official instructions rather than tested. Treat the
> first upload as something to watch.

Firmware goes on with an **ISP programmer (USBasp)**, not over serial — the MCU
is programmed before it is mated to its shield.

### Set the programming voltage first

The adapter has a voltage jumper. **Set it before connecting anything:**

| Target | Jumper |
| --- | --- |
| MCU | 5 V |
| Gateway | 3.3 V |

### From the Arduino IDE

1. *Tools → Programmer → USBasp*
2. *Sketch → Upload Using Programmer* (**not** the plain Upload button, which
   would try the serial bootloader)

### From the command line

```sh
arduino-cli upload \
  --fqbn 'MiniCore:avr:328:clock=8MHz_external,variant=modelPB,bootloader=uart0' \
  --programmer usbasp \
  .
```

`--programmer usbasp` is the equivalent of *Upload Using Programmer*; no `-p`
port is needed because USBasp is addressed over USB, not a serial port.

Fuses are written from the board settings when you burn via a programmer, so the
8 MHz external clock selection has to be right *before* this step — an ATmega
fused for an external crystal that is not present will not respond afterwards.

### Fitting the MCU to its shield

Do this with **bus power removed**:

1. Press **CONF** on the Gateway's LED panel. The **CONF LED** lights, and bus
   power is off.
2. Fit the MCU to the shield.
3. Press **CONF** again. The CONF LED goes out, the **POWER** LED lights, and
   bus voltage is restored.

### Later updates over the bus

`MY_OTA_FIRMWARE_FEATURE` is enabled in `Configuration.h`, so subsequent updates
can go over RS485 with the MySensors OTA tooling instead of an ISP programmer.
The node must already be running firmware that has the feature enabled, so the
first flash always has to be wired.

---

## 6. Unit tests

Needs only CMake ≥ 3.14 and a C++17 host compiler. GoogleTest is fetched
automatically on first configure, so the first run needs network.

```sh
cd Software/Modules/Arduino/main
cmake -S test -B build-test
cmake --build build-test -j
./build-test/gowired_tests
```

Expect `[  PASSED  ] 155 tests.`

Useful variations:

```sh
./build-test/gowired_tests --gtest_filter='Shutter*'      # one area
./build-test/gowired_tests --gtest_list_tests             # what exists
ctest --test-dir build-test --output-on-failure           # via ctest
```

No hardware, no Arduino toolchain and no MySensors installation are involved:
`test/fakes.h` implements every HAL interface, and `src/domain` contains no
Arduino headers. If a test ever fails to compile because something wants
`<Arduino.h>`, the layering has been broken — that is the point.

---

## 7. ATmega328PB → ATmega328P-AU

Newer boards use the ATmega328P-AU in place of the ATmega328PB. From the
firmware's side this is a one-line change: set MiniCore's *Variant* to
`328P / 328PA` (`variant=modelP`) instead of `328PB`. Everything else — board,
8 MHz external clock, bootloader, programming procedure — is unchanged.

What was checked:

- **Memory budget is identical.** Both are 32 KB flash / 2 KB SRAM / 1 KB EEPROM.
- **No firmware in this repository uses a 328PB-only peripheral.** A search
  across all seven sketches for `USART1`, `TWI1`, `SPI1`, `PORTE`/`PE0`–`PE3`,
  timers 3 and 4, and the PTC touch controller found nothing. The only
  registers this sketch touches directly are `ADMUX`, `ADCSRA`, `ADCL`, `ADCH`
  (the Vcc bandgap measurement), `MCUSR` and the watchdog — identical on both
  parts.
- **The PWM pins still work.** Outputs are on pins 5, 9, 6 and 10, which are
  OC0B, OC1A, OC0A and OC1B — timers 0 and 1, present on both.
- **It gets smaller.** A 328P build is ~2 kB less flash and ~80 bytes less SRAM,
  because MiniCore stops adding the 328PB's extra USART and TWI instances.

What is **not** covered by the above, and is a hardware review rather than a
firmware one:

- **Pinout and electrical compatibility** against both datasheets and the actual
  PCB. In particular confirm A6/A7 (ADC6/ADC7) land on the same pads — this
  sketch needs them for the current sensor and the internal thermometer, and they
  exist only in the TQFP-32 package. The `-AU` suffix is that package, so this
  should hold, but confirm it rather than take it from here.
- **Production programming scripts.** Anything invoking `avrdude -p m328pb`
  directly needs changing to `-p m328p`; the device signatures differ. Going
  through MiniCore handles this for you.
- **The other GoWired products.** The grep above covers the firmware in this
  repository. If any product depends on the 328PB's hardware touch controller or
  its extra peripherals in hardware rather than in code, that is outside what a
  source search can tell you. (The Touch sketches use the `ADCTouch` library,
  which senses capacitance in software through the ordinary ADC, so they are not
  affected.)

---

## 8. About the unused library dependencies

Two of the four libraries are dead weight. `ADCTouch` and `PCF8575-lib` are
needed only because Arduino compiles every source file of a library you include,
and `GoWired-lib` bundles a touch-input driver and an I2C expander driver
alongside the two ADC readers this sketch actually uses.

It is not merely a setup annoyance — the unused `ExpanderIO.cpp` declares a
file-scope `PCF8575 Expander;` whose constructor has side effects, so the linker
cannot discard it. It costs every build about **348 bytes of flash and 116 bytes
of SRAM** for a peripheral this firmware never touches. (The pre-refactor
firmware paid exactly the same, so the before/after comparison in
[README.md](README.md) is unaffected.)

The two pieces still used from `GoWired-lib` are `PowerSensor::MeasureAC/MeasureDC`
and `AnalogTemp::MeasureT` — roughly 80 lines of ADC sampling in total. Copying
them into `src/platform/` would drop three of the four library dependencies,
reclaim that flash and SRAM, and make this whole section unnecessary. It would
also mean this sketch no longer shares those drivers with the other GoWired
sketches, which is a call for the project owner rather than something to do
silently.

---

## Appendix A: installing the stock AVR core without the package index

Only relevant if you want the stock `arduino:avr` core as well -- MiniCore
installs normally even when this fails, because only Arduino's own package
index is signature-checked. Use this if `arduino-cli core install arduino:avr`
fails with
`Error verifying signature: signature expired: is your system clock set
correctly?`. First check the obvious cause:

```sh
date    # a clock that is wrong, or far in the future, invalidates the signature
```

If the clock is right and it still fails, the core and its toolchain can be
unpacked by hand. arduino-cli discovers installed platforms by scanning the
filesystem, so this works even with no usable package index.

```sh
P=~/.arduino15/packages/arduino
mkdir -p "$P/hardware/avr" "$P/tools/avr-gcc" ~/.arduino15/packages/builtin/tools/ctags
cd /tmp

# AVR core
curl -fsSLO https://downloads.arduino.cc/cores/staging/avr-1.8.6.tar.bz2
tar xjf avr-1.8.6.tar.bz2
mv avr-1.8.6 "$P/hardware/avr/1.8.6"

# avr-gcc 7.3.0 toolchain (~38 MB)
curl -fsSLO https://downloads.arduino.cc/tools/avr-gcc-7.3.0-atmel3.6.1-arduino7-x86_64-pc-linux-gnu.tar.bz2
tar xjf avr-gcc-7.3.0-atmel3.6.1-arduino7-x86_64-pc-linux-gnu.tar.bz2
mv avr "$P/tools/avr-gcc/7.3.0-atmel3.6.1-arduino7"

# ctags, which arduino-cli uses to preprocess .ino files
curl -fsSLO https://downloads.arduino.cc/tools/ctags-5.8-arduino11-pm-x86_64-pc-linux-gnu.tar.bz2
tar xjf ctags-5.8-arduino11-pm-x86_64-pc-linux-gnu.tar.bz2
mv ctags-5.8-arduino11 ~/.arduino15/packages/builtin/tools/ctags/5.8-arduino11
```

Verify:

```sh
~/.arduino15/packages/arduino/tools/avr-gcc/7.3.0-atmel3.6.1-arduino7/bin/avr-g++ --version
```

The version directories in these paths are not decoration — arduino-cli matches
them against the `runtime.tools.*` references in `platform.txt`. Renaming them
breaks the build.

`avrdude` is not installed by the above, so `arduino-cli upload` will not work on
this path. Compiling and size-checking do. Install `avrdude` from your package
manager if you need to flash.

Libraries can still be installed normally on this path — only the *package* index
is signature-checked; the *library* index is not. If `arduino-cli lib install`
also fails, clone into `~/Arduino/libraries/` instead:

```sh
git clone --depth 1 https://github.com/mysensors/MySensors.git       ~/Arduino/libraries/MySensors
git clone --depth 1 https://github.com/martin2250/ADCTouch.git       ~/Arduino/libraries/ADCTouch
git clone --depth 1 https://github.com/feanor-anglin/PCF8575-lib.git ~/Arduino/libraries/PCF8575-lib
git clone --depth 1 https://github.com/GoWired/GoWired-lib.git       ~/Arduino/libraries/GoWired-lib
```

To keep libraries outside your sketchbook, point at them explicitly:

```sh
arduino-cli compile --fqbn arduino:avr:nano:cpu=atmega328 \
  --libraries /path/to/libs \
  --build-property compiler.cpp.extra_flags=-std=gnu++17 .
```

---

## Appendix B: troubleshooting

Every entry below is an error actually hit while setting this up.

| Symptom | Cause and fix |
| --- | --- |
| `Error verifying signature: signature expired` | arduino-cli rejects Arduino's package index. Check `date`; otherwise use [Appendix A](#appendix-a-installing-the-stock-avr-core-without-the-package-index). |
| `Platform 'arduino:avr' not found: platform not installed` | The core install silently failed — often the signature error above. `arduino-cli core list` shows nothing. |
| `fork/exec {runtime.tools.ctags.path}/ctags: no such file` | `ctags` missing. Installed automatically by `core install`; fetch it by hand per Appendix A. |
| `PCF8575.h: No such file or directory` | `PCF8575-lib` not installed. Required even though this sketch never uses the expander. |
| `no matching function for call to 'PCF8575::PCF8575()'` | Wrong PCF8575 library. Use `PCF8575-lib` (feanor-anglin), not Rob Tillaart's or xreef's. |
| `ADCTouch.h: No such file or directory` | `ADCTouch` not installed. Also required only to compile `GoWired-lib`. |
| Errors about `if constexpr`, `inline constexpr`, structured bindings | C++17 not enabled. See [section 1.4](#14-c17). |
| `static_assert` "Two children share a sensor id" | Your `kInputCount` plus enabled features collide in the child-id space. Reduce `kInputCount` or disable a feature. |
| `static_assert` "FOUR_RELAY has no internal thermometer" | Set `kInternalTemperature = false` for `GW_FOUR_RELAY`. |
| `region 'text' overflowed` / `data section exceeds` | Sketch too large for 32 KB / 2 KB. Disable a feature in `Configuration.h`. |
| Tests fail to configure: cannot fetch GoogleTest | First CMake configure needs network. Behind a proxy, set `HTTPS_PROXY`. |

---

## Appendix C: C++17 on the stock Arduino AVR core

Not needed for normal use -- MiniCore already compiles at `-std=gnu++17`. This
matters only if you compile-check against `arduino:avr`, which fixes the standard
at `-std=gnu++11`.

*arduino-cli* -- that core keeps `-flto` in `compiler.cpp.flags` rather than in
`extra_flags`, so overriding `extra_flags` there is safe:

```sh
arduino-cli compile -b arduino:avr:nano:cpu=atmega328 \
  --build-property compiler.cpp.extra_flags=-std=gnu++17 .
```

*Arduino IDE* -- copy [platform.local.txt](platform.local.txt) next to that
core's `platform.txt` and restart; paths are in the file's comments.

The same trick on **MiniCore** would disable LTO, because that is where MiniCore
puts `-flto`. On MiniCore, pass nothing.
