#!/usr/bin/env python3
"""Build and flash the Rust GoWired firmware.

The Rust equivalent of ../../main/tools/gowired.py, and it exists for the same
reason: nobody should have to remember which cargo features a board needs, or
whether this batch of boards has the 328P or the 328PB.

    ./tools/gowired-rs.py                    # interactive
    ./tools/gowired-rs.py doctor             # check the toolchain
    ./tools/gowired-rs.py build -d dimmer    # build one variant
    ./tools/gowired-rs.py flash -d dimmer    # build and flash
    ./tools/gowired-rs.py sizes              # every variant, both parts

Standard library only; no pip install.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
FIRMWARE = ROOT / "crates" / "gowired-firmware"
CONFIG = FIRMWARE / "src" / "config.rs"

# --------------------------------------------------------------------------- #
# Terminal
# --------------------------------------------------------------------------- #

COLOUR = sys.stdout.isatty() and os.environ.get("NO_COLOR") is None


def _paint(code: str, text: str) -> str:
    return f"\033[{code}m{text}\033[0m" if COLOUR else text


def info(msg: str) -> None:
    print(f"{_paint('36', '..')} {msg}")


def ok(msg: str) -> None:
    print(f"{_paint('32', 'ok')} {msg}")


def warn(msg: str) -> None:
    print(f"{_paint('33', '!!')} {msg}")


def fail(msg: str) -> None:
    # Flush first: when stdout is a pipe and stderr is not, an unflushed heading
    # lands after the error it belongs to.
    sys.stdout.flush()
    print(f"{_paint('31', 'err')} {msg}", file=sys.stderr)


def die(msg: str, code: int = 1) -> "NoReturn":  # type: ignore[name-defined]
    fail(msg)
    sys.exit(code)


# --------------------------------------------------------------------------- #
# What can be built
# --------------------------------------------------------------------------- #


@dataclass(frozen=True)
class Device:
    """One board variant."""

    key: str
    feature: str
    label: str
    #: Whether the board has an on-board thermometer.
    thermometer: bool


DEVICES = [
    Device("double-relay", "device-double-relay", "2SSR, two independent relays", True),
    Device("roller-shutter", "device-roller-shutter", "2SSR driving one cover", True),
    Device("four-relay", "device-four-relay", "4RelayDin, four relays", False),
    Device("dimmer", "device-dimmer", "single-colour LED strip", True),
    Device("rgb", "device-rgb", "RGB strip", True),
    Device("rgbw", "device-rgbw", "RGBW strip", True),
]

DEVICE_BY_KEY = {d.key: d for d in DEVICES}


@dataclass(frozen=True)
class Mcu:
    """One target part."""

    key: str
    target_cpu: str
    label: str
    #: JEDEC device signature, as avrdude reports it.
    signature: str
    #: What avrdude calls the part.
    avrdude_part: str


MCUS = [
    Mcu("328p", "atmega328p", "ATmega328P-AU (newer boards)", "1e950f", "m328p"),
    Mcu("328pb", "atmega328pb", "ATmega328PB (earlier boards)", "1e9516", "m328pb"),
]

MCU_BY_KEY = {m.key: m for m in MCUS}

PROBES = {
    "none": None,
    "sht30": "probe-sht30",
    "dht22": "probe-dht22",
}

#: Flash available with a bootloader reserved, matching what the C++ build
#: measured against. Without one it is the full 32768.
FLASH_BUDGET = 32384
SRAM_TOTAL = 2048


# --------------------------------------------------------------------------- #
# Toolchain
# --------------------------------------------------------------------------- #


def run(cmd: list[str], **kwargs) -> subprocess.CompletedProcess:
    """Runs a command, inheriting the environment plus the AVR toolchain path."""
    env = dict(os.environ)
    avr_bin = find_avr_bin()
    if avr_bin:
        env["PATH"] = f"{avr_bin}{os.pathsep}{env.get('PATH', '')}"
    return subprocess.run(cmd, env=env, **kwargs)


def find_avr_bin() -> Path | None:
    """Locates avr-gcc, which is the linker and the size/nm tooling.

    Prefers whatever is on PATH, then the copy the Arduino IDE installs. Rust
    does not ship an AVR linker, so without this nothing links.
    """
    if shutil.which("avr-gcc"):
        return None  # already reachable
    for base in (
        Path.home() / ".arduino15" / "packages" / "arduino" / "tools" / "avr-gcc",
        Path.home() / ".arduino" / "packages" / "arduino" / "tools" / "avr-gcc",
        Path("/usr/lib/avr/bin"),
    ):
        if not base.exists():
            continue
        for candidate in sorted(base.glob("*/bin/avr-gcc"), reverse=True):
            return candidate.parent
    return None


def avr_tool(name: str) -> str | None:
    """Path to an avr-* binary, or None."""
    if shutil.which(name):
        return name
    avr_bin = find_avr_bin()
    if avr_bin and (avr_bin / name).exists():
        return str(avr_bin / name)
    return None


def nightly_toolchain() -> str | None:
    """The toolchain rust-toolchain.toml pins, if it is installed."""
    text = (ROOT / "rust-toolchain.toml").read_text(encoding="utf-8")
    match = re.search(r'channel\s*=\s*"([^"]+)"', text)
    if not match:
        return None
    return match.group(1)


def doctor(fix: bool = False) -> bool:
    """Checks everything a build needs. Returns whether it is ready."""
    healthy = True
    channel = nightly_toolchain()

    print(_paint("1", "Toolchain"))

    if not shutil.which("cargo"):
        fail("cargo not found. Install Rust: https://rustup.rs")
        return False
    ok("cargo found")

    if channel:
        installed = subprocess.run(
            ["rustup", "toolchain", "list"], capture_output=True, text=True, check=False
        ).stdout
        if channel in installed:
            ok(f"toolchain {channel}")
        elif fix:
            info(f"installing {channel} (AVR needs nightly: it has no prebuilt core)")
            if run(["rustup", "toolchain", "install", channel, "--profile", "minimal",
                    "--component", "rust-src"]).returncode != 0:
                fail(f"could not install {channel}")
                healthy = False
            else:
                ok(f"toolchain {channel}")
        else:
            fail(f"toolchain {channel} not installed. Re-run with --fix, or:")
            print(f"      rustup toolchain install {channel} "
                  f"--profile minimal --component rust-src")
            healthy = False

        # rust-src is what -Zbuild-std compiles core from.
        components = subprocess.run(
            ["rustup", "component", "list", "--toolchain", channel, "--installed"],
            capture_output=True, text=True, check=False,
        ).stdout
        if "rust-src" in components:
            ok("rust-src component")
        elif fix:
            info("installing rust-src")
            run(["rustup", "component", "add", "rust-src", "--toolchain", channel])
        else:
            fail("rust-src missing; -Zbuild-std cannot build core without it")
            healthy = False

    avr_bin = find_avr_bin()
    if shutil.which("avr-gcc"):
        ok("avr-gcc on PATH (the linker)")
    elif avr_bin:
        ok(f"avr-gcc found at {avr_bin}")
    else:
        fail("avr-gcc not found. It is the linker; Rust does not ship one for AVR.")
        print("      Debian/Ubuntu:  sudo apt install gcc-avr avr-libc")
        print("      Or install the Arduino IDE's AVR core, which bundles it.")
        healthy = False

    if avr_tool("avrdude"):
        ok("avrdude found (needed only to flash)")
    else:
        warn("avrdude not found; `build` works, `flash` will not")

    print()
    if healthy:
        ok("ready to build")
    else:
        fail("not ready; fix the above")
    return healthy


# --------------------------------------------------------------------------- #
# Configuration
# --------------------------------------------------------------------------- #


class PatchError(RuntimeError):
    """A configuration edit did not match exactly once."""


def _sub_once(text: str, pattern: str, replacement: str, what: str) -> str:
    """Substitutes exactly once, or raises.

    The C++ tool learned this the hard way: a regex that silently matched nothing
    produced six "different" builds with identical sizes. A patch that does not
    apply has to be an error, not a shrug.
    """
    new_text, count = re.subn(pattern, replacement, text, flags=re.MULTILINE)
    if count != 1:
        raise PatchError(f"{what}: pattern matched {count} times, expected 1")
    return new_text


def apply_settings(text: str, *, thermometer: bool | None = None,
                   node_id: int | None = None,
                   probe: bool | None = None) -> str:
    """Rewrites config.rs for a build. Returns the new contents."""
    if thermometer is not None:
        text = _sub_once(
            text,
            r"^pub const INTERNAL_TEMPERATURE: bool = (?:true|false);",
            f"pub const INTERNAL_TEMPERATURE: bool = {str(thermometer).lower()};",
            "INTERNAL_TEMPERATURE",
        )
    if probe is not None:
        text = _sub_once(
            text,
            r"^pub const EXTERNAL_TEMPERATURE: bool = (?:true|false);",
            f"pub const EXTERNAL_TEMPERATURE: bool = {str(probe).lower()};",
            "EXTERNAL_TEMPERATURE",
        )
    if node_id is not None:
        text = _sub_once(
            text,
            r"^pub const NODE_ID: u8 = .*;",
            f"pub const NODE_ID: u8 = {node_id};",
            "NODE_ID",
        )
    return text


class ConfigEdit:
    """Edits config.rs for the duration of a build, then puts it back.

    The C++ tool built from a staging copy of the tree. Here the edit is in place
    and reverted, because cargo's incremental state and the workspace layout make
    a copy more trouble than it is worth -- but the file is restored even if the
    build fails or the user interrupts it.
    """

    def __init__(self, keep: bool = False, **settings):
        self.keep = keep
        self.settings = settings
        self.original: str | None = None

    def __enter__(self) -> None:
        self.original = CONFIG.read_text(encoding="utf-8")
        patched = apply_settings(self.original, **self.settings)
        if patched != self.original:
            CONFIG.write_text(patched, encoding="utf-8")
        return None

    def __exit__(self, *exc) -> None:
        if self.original is not None and not self.keep:
            CONFIG.write_text(self.original, encoding="utf-8")


# --------------------------------------------------------------------------- #
# Build
# --------------------------------------------------------------------------- #


def elf_path() -> Path:
    return ROOT / "target" / "avr-none" / "release" / "gowired-firmware.elf"


def build(device: Device, mcu: Mcu, probe: str = "none",
          node_id: int | None = None, quiet: bool = False) -> Path:
    """Builds one variant. Returns the path to the ELF."""
    features = [device.feature]
    if PROBES.get(probe):
        features.append(PROBES[probe])

    env_flags = f"-C target-cpu={mcu.target_cpu}"

    settings: dict = {}
    if not device.thermometer:
        # The 4RelayDin has no thermometer: its analog pins are the four current
        # sensors. Asking for one is a compile error, so the tool turns it off
        # rather than letting the user hit an assertion.
        settings["thermometer"] = False
    if probe != "none":
        settings["probe"] = True
    if node_id is not None:
        settings["node_id"] = node_id

    cmd = [
        "cargo", "build", "--release",
        "--no-default-features",
        "--features", ",".join(features),
    ]

    if not quiet:
        info(f"building {device.key} for {mcu.label}")
        info(f"  features: {','.join(features)}")
        info(f"  target-cpu: {mcu.target_cpu}")

    with ConfigEdit(**settings):
        env = dict(os.environ)
        env["RUSTFLAGS"] = env.get("RUSTFLAGS", "") + " " + env_flags
        avr_bin = find_avr_bin()
        if avr_bin:
            env["PATH"] = f"{avr_bin}{os.pathsep}{env.get('PATH', '')}"
        result = subprocess.run(
            cmd, cwd=FIRMWARE, env=env,
            capture_output=quiet, text=True, check=False,
        )

    if result.returncode != 0:
        if quiet and result.stderr:
            print(result.stderr, file=sys.stderr)
        die(f"build failed for {device.key}")

    elf = elf_path()
    if not elf.exists():
        die(f"build reported success but {elf} is missing")
    return elf


@dataclass
class Sizes:
    """What a build costs."""

    flash: int
    static_sram: int
    main_frame: int

    @property
    def flash_percent(self) -> float:
        return 100.0 * self.flash / FLASH_BUDGET

    @property
    def sram_estimate(self) -> int:
        """Static plus `main`'s frame.

        Not the whole story: functions called from `main` and the two interrupt
        handlers add their own frames on top. It is the dominant term, and the one
        that changes when the code changes.
        """
        return self.static_sram + self.main_frame


def measure(elf: Path) -> Sizes:
    """Reads flash, static SRAM and `main`'s stack frame out of an ELF."""
    size_tool = avr_tool("avr-size")
    if not size_tool:
        die("avr-size not found; cannot measure")

    out = run([size_tool, str(elf)], capture_output=True, text=True).stdout
    numbers = out.strip().splitlines()[-1].split()
    flash, data, bss = int(numbers[0]), int(numbers[1]), int(numbers[2])

    # `main` allocates its frame with `subi r28, lo` / `sbci r29, hi`, which is
    # where every object in the firmware lives -- the design deliberately has no
    # `static mut`.
    frame = 0
    objdump = avr_tool("avr-objdump")
    if objdump:
        disasm = run([objdump, "-d", str(elf)], capture_output=True, text=True).stdout
        block = re.search(r"<main>:\n((?:.*\n){0,40})", disasm)
        if block:
            # objdump prints the immediate in upper case for some values and
            # lower for others, hence IGNORECASE.
            lo = re.search(r"subi\s+r28, 0x([0-9a-f]+)", block.group(1), re.IGNORECASE)
            hi = re.search(r"sbci\s+r29, 0x([0-9a-f]+)", block.group(1), re.IGNORECASE)
            if lo:
                frame = int(lo.group(1), 16) + (int(hi.group(1), 16) << 8 if hi else 0)

    return Sizes(flash=flash, static_sram=data + bss, main_frame=frame)


def check_flash_strings(elf: Path) -> bool:
    """Checks that no presentation string ended up in SRAM.

    The `gw_text!` macro exists to keep those in flash, and the whole point of it
    is an absence -- so it is easy to lose without noticing. A `Text` that was
    accidentally a plain `&str` would show up as a `.data` symbol.
    """
    nm = avr_tool("avr-nm")
    if not nm:
        warn("avr-nm not found; cannot verify flash strings")
        return True

    out = run([nm, "-S", str(elf)], capture_output=True, text=True).stdout
    # `.data` symbols are 'd' or 'D'. Anonymous rodata blobs are expected (jump
    # tables, the config structs); a *long* one is a string that should have been
    # in flash.
    suspicious = []
    for line in out.splitlines():
        parts = line.split()
        if len(parts) >= 4 and parts[2] in ("d", "D"):
            try:
                size = int(parts[1], 16)
            except ValueError:
                continue
            if size >= 32 and "switch.table" not in parts[3]:
                suspicious.append((size, parts[3]))
    if suspicious:
        warn("large .data symbols -- check these are not strings that should be in flash:")
        for size, name in sorted(suspicious, reverse=True)[:5]:
            print(f"      {size:5d}  {name[:90]}")
        return False
    return True


def report(device: Device, mcu: Mcu, sizes: Sizes) -> None:
    """Prints the size table for one build."""
    flash_note = ""
    if sizes.flash > FLASH_BUDGET:
        flash_note = _paint("31", "  DOES NOT FIT")
    elif sizes.flash_percent > 90:
        flash_note = _paint("33", "  tight")

    print()
    print(f"{device.key} / {mcu.key}")
    print(f"  flash        {sizes.flash:6d} of {FLASH_BUDGET}"
          f"  ({sizes.flash_percent:.0f}%){flash_note}")
    print(f"  static SRAM  {sizes.static_sram:6d}")
    print(f"  main frame   {sizes.main_frame:6d}   every object lives here; no static mut")
    print(f"  SRAM in use  {sizes.sram_estimate:6d} of {SRAM_TOTAL}"
          f"  ({100.0 * sizes.sram_estimate / SRAM_TOTAL:.0f}%),"
          f" plus call frames and interrupts")


# --------------------------------------------------------------------------- #
# Flash
# --------------------------------------------------------------------------- #


def detect_mcu(programmer: str) -> Mcu | None:
    """Reads the device signature and matches it against the known parts.

    This is the whole reason the tool exists: nobody should have to remember
    whether this batch of boards has the 328P or the 328PB, and getting it wrong
    means avrdude refuses to write or -- worse -- the wrong fuses.
    """
    avrdude = avr_tool("avrdude")
    if not avrdude:
        return None

    for mcu in MCUS:
        result = run(
            [avrdude, "-p", mcu.avrdude_part, "-c", programmer, "-n"],
            capture_output=True, text=True,
        )
        blob = (result.stdout + result.stderr).lower()
        if mcu.signature in blob.replace("0x", "").replace(" ", ""):
            return mcu
        if "device signature" in blob and mcu.signature in blob:
            return mcu
    return None


def flash(elf: Path, mcu: Mcu, programmer: str = "usbasp") -> bool:
    """Writes an ELF to the part with avrdude."""
    avrdude = avr_tool("avrdude")
    if not avrdude:
        die("avrdude not found; cannot flash")

    objcopy = avr_tool("avr-objcopy")
    if not objcopy:
        die("avr-objcopy not found; cannot make a .hex")

    hex_path = elf.with_suffix(".hex")
    if run([objcopy, "-O", "ihex", "-R", ".eeprom", str(elf), str(hex_path)]).returncode != 0:
        die("could not convert the ELF to Intel hex")

    info(f"flashing {hex_path.name} to {mcu.label} via {programmer}")
    result = run([
        avrdude, "-p", mcu.avrdude_part, "-c", programmer,
        "-U", f"flash:w:{hex_path}:i",
    ])
    if result.returncode != 0:
        fail("avrdude failed")
        print("      Check: programmer connected, bus power off, "
              "voltage jumper set (5 V for an MCU, 3.3 V for a Gateway).")
        print("      See BUILDING.md section 5.")
        return False
    ok("flashed")
    return True


# --------------------------------------------------------------------------- #
# Commands
# --------------------------------------------------------------------------- #


def cmd_doctor(args) -> int:
    return 0 if doctor(fix=args.fix) else 1


def cmd_build(args) -> int:
    device = DEVICE_BY_KEY[args.device]
    mcu = MCU_BY_KEY[args.mcu]
    elf = build(device, mcu, probe=args.probe, node_id=args.node_id)
    sizes = measure(elf)
    report(device, mcu, sizes)
    if args.check_strings:
        print()
        if check_flash_strings(elf):
            ok("no presentation strings in SRAM")
    if sizes.flash > FLASH_BUDGET:
        return 1
    return 0


def cmd_flash(args) -> int:
    device = DEVICE_BY_KEY[args.device]

    if args.mcu == "auto":
        info("reading the device signature")
        detected = detect_mcu(args.programmer)
        if detected is None:
            warn("could not read a signature; is the programmer connected?")
            print("      Pass --mcu 328p or --mcu 328pb to build without detecting.")
            return 1
        ok(f"detected {detected.label}")
        mcu = detected
    else:
        mcu = MCU_BY_KEY[args.mcu]

    elf = build(device, mcu, probe=args.probe, node_id=args.node_id)
    sizes = measure(elf)
    report(device, mcu, sizes)
    if sizes.flash > FLASH_BUDGET:
        die("does not fit; refusing to flash")
    print()
    return 0 if flash(elf, mcu, args.programmer) else 1


def cmd_sizes(args) -> int:
    """Builds everything and prints one table. What the README quotes."""
    print(f"{'variant':<16}{'328P flash':>12}{'328PB flash':>13}"
          f"{'static':>9}{'main frame':>12}")
    print("-" * 62)
    worst = 0
    for device in DEVICES:
        row = [f"{device.key:<16}"]
        static = frame = 0
        for mcu in MCUS:
            elf = build(device, mcu, quiet=True)
            sizes = measure(elf)
            worst = max(worst, sizes.flash)
            mark = "!" if sizes.flash > FLASH_BUDGET else ""
            row.append(f"{sizes.flash:>11}{mark}")
            if mcu.key == "328p":
                static, frame = sizes.static_sram, sizes.main_frame
        row.append(f"{static:>9}{frame:>12}")
        print("".join(row))
    print("-" * 62)
    print(f"flash budget {FLASH_BUDGET} with a bootloader reserved; worst case {worst}")
    return 0 if worst <= FLASH_BUDGET else 1


def cmd_test(args) -> int:
    """Runs the host test suite."""
    result = subprocess.run(["cargo", "test"], cwd=ROOT, check=False)
    return result.returncode


def choose(prompt: str, options: list[tuple[str, str]], default: int = 0) -> str:
    """Numbered menu. Returns the chosen key."""
    print()
    print(_paint("1", prompt))
    for i, (key, label) in enumerate(options, start=1):
        marker = " (default)" if i - 1 == default else ""
        print(f"  {i}. {key:<16} {label}{marker}")
    while True:
        raw = input(f"choice [1-{len(options)}, blank for default]: ").strip()
        if not raw:
            return options[default][0]
        if raw.isdigit() and 1 <= int(raw) <= len(options):
            return options[int(raw) - 1][0]
        print("  not one of the options")


def interactive() -> int:
    print(_paint("1", "GoWired firmware installer (Rust)"))
    print()
    if not doctor():
        answer = input("\nTry to install what is missing? [Y/n]: ").strip().lower()
        if answer in ("", "y", "yes"):
            if not doctor(fix=True):
                die("still not ready; see BUILDING.md")
        else:
            return 1

    device_key = choose("Which board?", [(d.key, d.label) for d in DEVICES])
    probe_key = choose(
        "External temperature/humidity probe?",
        [("none", "not fitted"), ("sht30", "SHT30 over I2C"),
         ("dht22", "DHT22 / AM2302, one wire")],
    )

    mcu_key = choose(
        "Which part?",
        [("auto", "read the signature off the board (recommended)")]
        + [(m.key, m.label) for m in MCUS],
    )

    print()
    do_flash = input("Flash it after building? [y/N]: ").strip().lower() in ("y", "yes")

    args = argparse.Namespace(
        device=device_key, mcu=mcu_key, probe=probe_key, node_id=None,
        programmer="usbasp", check_strings=True,
    )
    if do_flash:
        return cmd_flash(args)
    if mcu_key == "auto":
        args.mcu = "328p"
        info("building for the 328P; signature detection only happens when flashing")
    return cmd_build(args)


def add_build_args(parser: argparse.ArgumentParser, *, allow_auto: bool = False) -> None:
    parser.add_argument("-d", "--device", choices=list(DEVICE_BY_KEY),
                        default="double-relay", help="board variant")
    mcus = (["auto"] if allow_auto else []) + list(MCU_BY_KEY)
    parser.add_argument("-m", "--mcu", choices=mcus,
                        default="auto" if allow_auto else "328p", help="target part")
    parser.add_argument("-p", "--probe", choices=list(PROBES), default="none",
                        help="external probe")
    parser.add_argument("-n", "--node-id", type=int, default=None,
                        help="fixed MySensors node id (default: ask the controller)")
    parser.add_argument("--programmer", default="usbasp", help="avrdude programmer")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=__doc__.splitlines()[0],
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="Run with no arguments for the interactive installer.",
    )
    sub = parser.add_subparsers(dest="command")

    p_doctor = sub.add_parser("doctor", help="check the toolchain")
    p_doctor.add_argument("--fix", action="store_true", help="install what is missing")
    p_doctor.set_defaults(func=cmd_doctor)

    p_build = sub.add_parser("build", help="build one variant")
    add_build_args(p_build)
    p_build.add_argument("--check-strings", action="store_true",
                        help="verify no presentation strings landed in SRAM")
    p_build.set_defaults(func=cmd_build)

    p_flash = sub.add_parser("flash", help="build and flash")
    add_build_args(p_flash, allow_auto=True)
    p_flash.set_defaults(func=cmd_flash, check_strings=False)

    p_sizes = sub.add_parser("sizes", help="build every variant and tabulate")
    p_sizes.set_defaults(func=cmd_sizes)

    p_test = sub.add_parser("test", help="run the host test suite")
    p_test.set_defaults(func=cmd_test)

    args = parser.parse_args(argv)
    if args.command is None:
        return interactive()
    return args.func(args)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        print()
        die("interrupted", code=130)
    except EOFError:
        # Piped or closed stdin; not a crash.
        print()
        die("no input", code=130)
    except PatchError as exc:
        die(f"could not patch config.rs -- {exc}\n"
            "     Has the file been edited by hand? The tool expects the shipped"
            " layout.")
