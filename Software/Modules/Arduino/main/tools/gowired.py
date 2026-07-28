#!/usr/bin/env python3
"""
GoWired module configure / build / flash tool.

Picks the board settings for you instead of making you remember them:

  * detects whether the MCU is an ATmega328P or an ATmega328PB by reading its
    signature through the programmer, so you never choose a "variant" by hand;
  * writes Configuration.h for the device type and features you asked for,
    including the couplings that are easy to forget (FOUR_RELAY has no
    thermistor, an external probe needs a probe type selected);
  * builds in a staging copy so your working tree stays clean;
  * flashes with the ISP programmer.

Requires only the Python standard library.

    ./tools/gowired.py                     # interactive, does everything
    ./tools/gowired.py doctor              # check the toolchain
    ./tools/gowired.py build -d rgbw       # non-interactive build
    ./tools/gowired.py flash -p double_relay
    ./tools/gowired.py profile save attic  # remember a configuration

Run with --help, or `<command> --help`, for the full option list.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Dict, List, Optional, Sequence, Tuple

# --------------------------------------------------------------------------- #
# Layout
# --------------------------------------------------------------------------- #

SKETCH_DIR = Path(__file__).resolve().parent.parent
SKETCH_NAME = SKETCH_DIR.name
CONFIG_H = SKETCH_DIR / "Configuration.h"
TEST_DIR = SKETCH_DIR / "test"
PROFILE_FILE = SKETCH_DIR / "tools" / "profiles.json"

MINICORE_URL = (
    "https://mcudude.github.io/MiniCore/package_MCUdude_MiniCore_index.json"
)

ARDUINO_CLI_INSTALLER = (
    "https://raw.githubusercontent.com/arduino/arduino-cli/master/install.sh"
)

# --------------------------------------------------------------------------- #
# Hardware / device tables
# --------------------------------------------------------------------------- #


@dataclass(frozen=True)
class Device:
    key: str
    macro: str
    label: str
    #: FOUR_RELAY's analog pins carry its four current sensors, so it has no
    #: thermistor. Configuration.h static_asserts this.
    supports_internal_temp: bool = True


DEVICES: Tuple[Device, ...] = (
    Device("double_relay", "GW_DOUBLE_RELAY", "2SSR - two independent relays"),
    Device("roller_shutter", "GW_ROLLER_SHUTTER", "2SSR - one roller shutter"),
    Device("four_relay", "GW_FOUR_RELAY", "4RelayDin - four relays",
           supports_internal_temp=False),
    Device("dimmer", "GW_DIMMER", "RGBW shield - single-colour LED strip"),
    Device("rgb", "GW_RGB", "RGBW shield - RGB strip"),
    Device("rgbw", "GW_RGBW", "RGBW shield - RGBW strip"),
)

DEVICES_BY_KEY = {d.key: d for d in DEVICES}


@dataclass(frozen=True)
class Mcu:
    key: str
    variant: str  #: MiniCore FQBN 'variant' value
    signature: str  #: device signature, lower-case hex, no separators
    avrdude_part: str
    label: str


MCUS: Tuple[Mcu, ...] = (
    Mcu("328p", "modelP", "1e950f", "m328p", "ATmega328P / 328P-AU (newer boards)"),
    Mcu("328pb", "modelPB", "1e9516", "m328pb", "ATmega328PB (earlier boards)"),
)

MCUS_BY_KEY = {m.key: m for m in MCUS}
MCUS_BY_SIGNATURE = {m.signature: m for m in MCUS}

PROBES = {
    "none": (None, None),
    "sht30": ("GW_PROBE_SHT30", "arduino-sht"),
    "dht22": ("GW_PROBE_DHT22", "DHTlib"),
}

#: Libraries the sketch needs. GoWired-lib is not in the Arduino index, and two
#: of these are needed only because Arduino compiles every source file of an
#: included library -- see BUILDING.md section 8.
REQUIRED_LIBS = {
    "MySensors": None,
    "GoWired-lib": "https://github.com/GoWired/GoWired-lib.git",
    "ADCTouch": None,
    "PCF8575-lib": None,
}

BOOTLOADER_CHOICES = {"yes": "uart0", "no": "no_bootloader"}

# --------------------------------------------------------------------------- #
# Terminal helpers
# --------------------------------------------------------------------------- #

_COLOR = sys.stdout.isatty() and os.environ.get("NO_COLOR") is None


def _c(code: str, text: str) -> str:
    return f"\033[{code}m{text}\033[0m" if _COLOR else text


def info(msg: str) -> None:
    print(f"{_c('36', '::')} {msg}")


def ok(msg: str) -> None:
    print(f"{_c('32', ' ok')} {msg}")


def warn(msg: str) -> None:
    print(f"{_c('33', ' !!')} {msg}")


def fail(msg: str) -> None:
    # Flush stdout first, or headings and errors interleave out of order when
    # the two streams are merged into a pipe.
    sys.stdout.flush()
    print(f"{_c('31', 'err')} {msg}", file=sys.stderr)
    sys.stderr.flush()


def die(msg: str, code: int = 1) -> "NoReturn":  # type: ignore[valid-type]
    fail(msg)
    raise SystemExit(code)


def heading(text: str) -> None:
    print()
    print(_c("1", text))
    print(_c("90", "-" * len(text)))


# --------------------------------------------------------------------------- #
# Settings
# --------------------------------------------------------------------------- #


@dataclass
class InputCfg:
    enabled: bool = True
    pullup: bool = True
    invert: bool = False


@dataclass
class Settings:
    """Everything this tool can write into Configuration.h."""

    device: str = "double_relay"
    mcu: str = "auto"  #: '328p', '328pb', or 'auto' to read the signature
    bootloader: str = "yes"

    node_id: str = "AUTO"
    sketch_name: str = "GoWired Module"
    sketch_version: str = "3.0"

    power_sensor: bool = True
    internal_temperature: bool = True
    external_probe: str = "none"  #: none | sht30 | dht22
    error_reporting: bool = True
    heating_controller_node: int = 0

    inputs: List[InputCfg] = field(
        default_factory=lambda: [InputCfg() for _ in range(4)]
    )

    # Tuning the official instructions call out.
    max_current_a: int = 3
    mv_per_amp: int = 185
    receiver_voltage: int = 230
    dimming_step: int = 1
    dimming_interval_ms: int = 1
    up_time_s: int = 21
    down_time_s: int = 20

    # -- serialisation ---------------------------------------------------- #

    def to_json(self) -> dict:
        d = asdict(self)
        d["inputs"] = [asdict(i) if not isinstance(i, dict) else i for i in self.inputs]
        return d

    @classmethod
    def from_json(cls, data: dict) -> "Settings":
        data = dict(data)
        inputs = data.pop("inputs", None)
        known = {f for f in cls.__dataclass_fields__ if f != "inputs"}
        unknown = set(data) - known
        if unknown:
            warn(f"ignoring unknown profile keys: {', '.join(sorted(unknown))}")
        s = cls(**{k: v for k, v in data.items() if k in known})
        if inputs:
            s.inputs = [InputCfg(**i) for i in inputs][:4]
            while len(s.inputs) < 4:
                s.inputs.append(InputCfg(enabled=False))
        return s

    # -- validation ------------------------------------------------------- #

    @property
    def dev(self) -> Device:
        try:
            return DEVICES_BY_KEY[self.device]
        except KeyError:
            die(
                f"unknown device '{self.device}'. "
                f"Choose from: {', '.join(DEVICES_BY_KEY)}"
            )

    def normalise(self) -> List[str]:
        """Fix up couplings the firmware asserts on. Returns notes to show."""
        notes: List[str] = []

        if self.internal_temperature and not self.dev.supports_internal_temp:
            self.internal_temperature = False
            notes.append(
                f"{self.dev.macro} has no thermistor (its analog pins carry the "
                "four current sensors) - internal temperature disabled"
            )

        if self.external_probe not in PROBES:
            die(
                f"unknown probe '{self.external_probe}'. "
                f"Choose from: {', '.join(PROBES)}"
            )

        if self.bootloader not in BOOTLOADER_CHOICES:
            die(f"bootloader must be one of: {', '.join(BOOTLOADER_CHOICES)}")

        if self.mcu != "auto" and self.mcu not in MCUS_BY_KEY:
            die(f"unknown mcu '{self.mcu}'. Choose from: auto, {', '.join(MCUS_BY_KEY)}")

        if self.node_id.upper() != "AUTO":
            try:
                n = int(self.node_id)
            except ValueError:
                die("node id must be a number or AUTO")
            if not 1 <= n <= 254:
                die("node id must be between 1 and 254, or AUTO")

        if not 0 <= self.heating_controller_node <= 254:
            die("heating controller node must be between 0 and 254")

        if self.error_reporting and not (
            self.power_sensor or self.internal_temperature or self.external_probe != "none"
        ):
            notes.append(
                "error reporting is on but there is no sensor to report about"
            )

        return notes

    def fqbn(self, mcu: Mcu) -> str:
        return (
            "MiniCore:avr:328"
            f":clock=8MHz_external,variant={mcu.variant}"
            f",bootloader={BOOTLOADER_CHOICES[self.bootloader]}"
        )

    def summary(self, mcu: Optional[Mcu]) -> str:
        enabled = [
            f"{i + 1}{'' if c.pullup else ' (no pullup)'}{' (inverted)' if c.invert else ''}"
            for i, c in enumerate(self.inputs)
            if c.enabled
        ]
        rows = [
            ("Device", f"{self.dev.macro}  ({self.dev.label})"),
            ("MCU", mcu.label if mcu else f"{self.mcu} (not yet detected)"),
            ("Bootloader", self.bootloader),
            ("Node id", self.node_id),
            ("Name / version", f"{self.sketch_name!r} / {self.sketch_version!r}"),
            ("Power sensor", yn(self.power_sensor)),
            ("Internal temp", yn(self.internal_temperature)),
            ("External probe", self.external_probe),
            ("Error reporting", yn(self.error_reporting)),
            ("Heating node", self.heating_controller_node or "off"),
            ("Digital inputs", ", ".join(enabled) if enabled else "none"),
            ("Max current", f"{self.max_current_a} A @ {self.mv_per_amp} mV/A"),
            ("Load voltage", f"{self.receiver_voltage} V"),
        ]
        if self.device == "roller_shutter":
            rows.append(("Travel up/down", f"{self.up_time_s} s / {self.down_time_s} s"))
        if self.device in ("dimmer", "rgb", "rgbw"):
            rows.append(
                ("Dimming", f"step {self.dimming_step}, {self.dimming_interval_ms} ms")
            )
        width = max(len(k) for k, _ in rows)
        return "\n".join(f"  {k.ljust(width)}  {v}" for k, v in rows)


def yn(b: bool) -> str:
    return "yes" if b else "no"


# --------------------------------------------------------------------------- #
# Configuration.h rewriting
# --------------------------------------------------------------------------- #


class PatchError(RuntimeError):
    pass


def _sub_once(text: str, pattern: str, replacement, what: str) -> str:
    """Substitute exactly once, or raise.

    A regex that silently matches nothing is the worst possible failure mode
    here: you get a clean build of the wrong configuration.
    """
    new, n = re.subn(pattern, replacement, text, count=1, flags=re.M)
    if n != 1:
        raise PatchError(
            f"could not set {what} in Configuration.h "
            f"(pattern matched {n} times, expected 1). "
            "Configuration.h has probably been edited in a way this tool does "
            "not recognise."
        )
    return new


def _lit(value: str) -> str:
    """Escape a replacement string so backslashes/backrefs are literal."""
    return value.replace("\\", "\\\\")


def apply_settings(text: str, s: Settings) -> str:
    """Return Configuration.h with `s` applied. Comments are preserved."""

    def keep_tail(new_value: str):
        """Replace group(1) with new_value, keeping any trailing comment."""
        return lambda m: f"{m.group(1)}{_lit(new_value)}{m.group(2)}"

    text = _sub_once(
        text, r"^(#define MY_NODE_ID )(\S+)(.*)$",
        lambda m: f"{m.group(1)}{_lit(s.node_id)}{m.group(3)}", "node id",
    )
    text = _sub_once(
        text, r'^(#define SN ")([^"]*)(".*)$',
        lambda m: f"{m.group(1)}{_lit(s.sketch_name)}{m.group(3)}", "sketch name",
    )
    text = _sub_once(
        text, r'^(#define SV ")([^"]*)(".*)$',
        lambda m: f"{m.group(1)}{_lit(s.sketch_version)}{m.group(3)}", "sketch version",
    )
    text = _sub_once(
        text, r"^(#define GW_DEVICE )(\S+)(.*)$",
        lambda m: f"{m.group(1)}{s.dev.macro}{m.group(3)}", "device",
    )

    for name, value in (
        ("kPowerSensor", s.power_sensor),
        ("kInternalTemperature", s.internal_temperature),
        ("kExternalTemperature", s.external_probe != "none"),
        ("kErrorReporting", s.error_reporting),
    ):
        text = _sub_once(
            text, rf"^(constexpr bool {name} = )(true|false)(;.*)$",
            keep_tail_bool(value), name,
        )

    text = _sub_once(
        text, r"^(constexpr uint8_t kHeatingControllerNode = )(\d+)(;.*)$",
        lambda m: f"{m.group(1)}{s.heating_controller_node}{m.group(3)}",
        "heating controller node",
    )

    # Probe selection is a macro, because it picks a third-party header.
    for probe_key, (macro, _lib) in PROBES.items():
        if macro is None:
            continue
        active = s.external_probe == probe_key
        text = _sub_once(
            text, rf"^(//)?(#define {macro})\s*$",
            (lambda mm: mm.group(2)) if active else (lambda mm: f"//{mm.group(2)}"),
            macro,
        )

    # Digital inputs.
    for i, cfg in enumerate(s.inputs, start=1):
        text = _sub_once(
            text,
            rf"^(\s*/\* INPUT_{i} \*/ \{{)[^}}]*(\}},.*)$",
            lambda m, c=cfg: (
                f"{m.group(1)}{str(c.enabled).lower()}, "
                f"{str(c.pullup).lower()}, {str(c.invert).lower()}{m.group(2)}"
            ),
            f"INPUT_{i}",
        )

    # Tuning values, matched by their aligned field comments.
    for label, value, what in (
        ("step", s.dimming_step, "dimming step"),
        ("interval_ms", s.dimming_interval_ms, "dimming interval"),
        ("up_time_s", s.up_time_s, "shutter up time"),
        ("down_time_s", s.down_time_s, "shutter down time"),
        ("max_current_a", s.max_current_a, "max current"),
        ("receiver_voltage", s.receiver_voltage, "receiver voltage"),
        ("mv_per_amp", s.mv_per_amp, "mV per amp"),
    ):
        text = _sub_once(
            text,
            rf"^(\s*/\* {label}\s*\*/ )([0-9.]+f?)(,.*)$",
            lambda m, v=value: f"{m.group(1)}{v}{m.group(3)}",
            what,
        )

    return text


def keep_tail_bool(value: bool):
    return lambda m: f"{m.group(1)}{str(value).lower()}{m.group(3)}"


# --------------------------------------------------------------------------- #
# Toolchain
# --------------------------------------------------------------------------- #


def run(
    cmd: Sequence[str], *, capture: bool = True, check: bool = False, timeout: int = 900
) -> subprocess.CompletedProcess:
    return subprocess.run(
        list(cmd),
        capture_output=capture,
        text=True,
        check=check,
        timeout=timeout,
    )


class Toolchain:
    def __init__(self, cli: Optional[str] = None) -> None:
        self.cli = cli or shutil.which("arduino-cli") or ""

    # -- discovery -------------------------------------------------------- #

    def require_cli(self) -> str:
        if not self.cli:
            die(
                "arduino-cli not found on PATH.\n"
                "    Install it automatically:\n"
                "      ./tools/gowired.py doctor --fix\n"
                "    or by hand:\n"
                f"      curl -fsSL {ARDUINO_CLI_INSTALLER} | BINDIR=~/.local/bin sh\n"
                "    then make sure ~/.local/bin is on your PATH.\n"
                "    Or point this tool at it with --arduino-cli /path/to/arduino-cli"
            )
        return self.cli

    def install_arduino_cli(self, bindir: Optional[Path] = None) -> bool:
        """Run the official installer, then use the result immediately.

        The binary is used by absolute path for the rest of this run, so a
        PATH that does not yet include `bindir` is a warning rather than a
        blocker.
        """
        bindir = bindir or (Path.home() / ".local" / "bin")
        info("installing arduino-cli using the official installer")
        print(f"     {ARDUINO_CLI_INSTALLER}")
        print(f"     into {bindir}")

        try:
            bindir.mkdir(parents=True, exist_ok=True)
        except OSError as e:
            fail(f"cannot create {bindir}: {e}")
            return False

        try:
            import urllib.request

            with urllib.request.urlopen(ARDUINO_CLI_INSTALLER, timeout=120) as resp:
                script = resp.read()
        except Exception as e:  # network, TLS, proxy, ...
            fail(f"could not download the installer: {e}")
            print("     if you are behind a proxy, set HTTPS_PROXY and retry,")
            print("     or install arduino-cli by hand and re-run with")
            print("     --arduino-cli /path/to/arduino-cli")
            return False

        if not shutil.which("sh"):
            fail("no POSIX shell found to run the installer")
            return False

        tmp = Path(tempfile.mkdtemp(prefix="arduino-cli-install-")) / "install.sh"
        tmp.write_bytes(script)
        try:
            r = subprocess.run(
                ["sh", str(tmp)],
                capture_output=True,
                text=True,
                env={**os.environ, "BINDIR": str(bindir)},
                timeout=900,
            )
        except subprocess.TimeoutExpired:
            fail("the installer timed out")
            return False
        finally:
            shutil.rmtree(tmp.parent, ignore_errors=True)

        candidate = bindir / "arduino-cli"
        if r.returncode != 0 or not candidate.exists():
            fail("arduino-cli installation failed")
            for line in (r.stdout + r.stderr).strip().splitlines()[-10:]:
                print(f"     {line}")
            return False

        self.cli = str(candidate)
        version = run([self.cli, "version"], timeout=60).stdout.strip()
        ok(f"installed {version}")

        path_entries = os.environ.get("PATH", "").split(os.pathsep)
        if str(bindir) not in path_entries:
            warn(f"{bindir} is not on your PATH - using it directly for this run")
            print(f"     to make it permanent:")
            print(f"       echo 'export PATH=\"{bindir}:$PATH\"' >> ~/.bashrc")
        return True

    def _cli(self, *args: str, timeout: int = 900) -> subprocess.CompletedProcess:
        return run([self.require_cli(), *args], timeout=timeout)

    def has_minicore(self) -> bool:
        r = self._cli("core", "list", timeout=120)
        return "MiniCore:avr" in r.stdout

    def installed_libs(self) -> List[str]:
        r = self._cli("lib", "list", timeout=180)
        names = []
        for line in r.stdout.splitlines()[1:]:
            if line.strip():
                names.append(line.split()[0])
        return names

    def sketchbook_libs(self) -> Path:
        r = self._cli("config", "dump", timeout=60)
        m = re.search(r"^\s*user:\s*(.+)$", r.stdout, re.M)
        base = Path(m.group(1).strip()) if m else Path.home() / "Arduino"
        return base / "libraries"

    # -- installation ----------------------------------------------------- #

    def install_minicore(self) -> bool:
        info("installing MiniCore ...")
        r = self._cli(
            "core", "install", "MiniCore:avr", "--additional-urls", MINICORE_URL,
            timeout=1800,
        )
        if r.returncode != 0:
            fail(r.stdout.strip() or r.stderr.strip())
            return False
        ok("MiniCore installed")
        return True

    def install_lib(self, name: str, git_url: Optional[str]) -> bool:
        if git_url:
            target = self.sketchbook_libs() / name
            if target.exists():
                ok(f"{name} already present")
                return True
            if not shutil.which("git"):
                fail(f"{name} needs git to clone from {git_url}")
                return False
            target.parent.mkdir(parents=True, exist_ok=True)
            info(f"cloning {name} ...")
            r = run(["git", "clone", "--depth", "1", git_url, str(target)], timeout=600)
            if r.returncode != 0:
                fail(r.stderr.strip())
                return False
            ok(f"{name} cloned into {target}")
            return True

        info(f"installing {name} ...")
        r = self._cli("lib", "install", name, timeout=900)
        if r.returncode != 0:
            fail(r.stdout.strip() or r.stderr.strip())
            return False
        ok(f"{name} installed")
        return True

    # -- MCU detection ---------------------------------------------------- #

    def detect_mcu(self, programmer: str = "usbasp") -> Optional[Mcu]:
        """Read the device signature through the programmer.

        Tries every known part until avrdude reports a signature. avrdude exits
        non-zero on a signature mismatch but still prints what it saw, which is
        exactly what we want.
        """
        avrdude, conf = self._find_avrdude()
        if not avrdude:
            warn("avrdude not found; cannot detect the MCU")
            return None

        for mcu in MCUS:
            cmd = [avrdude]
            if conf:
                cmd += ["-C", conf]
            cmd += ["-c", programmer, "-p", mcu.avrdude_part, "-n"]
            try:
                r = run(cmd, timeout=60)
            except subprocess.TimeoutExpired:
                warn("avrdude timed out talking to the programmer")
                return None
            blob = f"{r.stdout}\n{r.stderr}"
            m = re.search(r"[Dd]evice signature\s*=\s*0x([0-9a-fA-F]{6})", blob)
            if m:
                sig = m.group(1).lower()
                found = MCUS_BY_SIGNATURE.get(sig)
                if found:
                    return found
                warn(f"unrecognised device signature 0x{sig}")
                return None
            if "can't open device" in blob or "no programmer" in blob.lower():
                warn("programmer not found - is the USBasp plugged in?")
                return None
        warn("could not read a device signature")
        return None

    def _find_avrdude(self) -> Tuple[Optional[str], Optional[str]]:
        """Prefer MiniCore's avrdude, since it knows the 328PB."""
        root = Path.home() / ".arduino15" / "packages"
        for pkg in ("MiniCore", "arduino"):
            base = root / pkg / "tools" / "avrdude"
            if not base.is_dir():
                continue
            for version in sorted(base.iterdir(), reverse=True):
                exe = version / "bin" / "avrdude"
                conf = version / "etc" / "avrdude.conf"
                if exe.exists():
                    return str(exe), str(conf) if conf.exists() else None
        found = shutil.which("avrdude")
        return (found, None) if found else (None, None)


# --------------------------------------------------------------------------- #
# Staging, build, flash
# --------------------------------------------------------------------------- #


def stage_sketch(s: Settings, dest: Path) -> Path:
    """Copy the sketch to `dest` and apply settings there."""
    target = dest / SKETCH_NAME
    shutil.copytree(
        SKETCH_DIR,
        target,
        ignore=shutil.ignore_patterns(
            "test", "tools", "build*", "*.o", "*.elf", "*.hex", "__pycache__", ".git"
        ),
    )
    cfg = target / "Configuration.h"
    cfg.write_text(apply_settings(cfg.read_text(), s))
    return target


def explain_build_failure(output: str) -> None:
    """Turn the firmware's static_asserts into actionable advice."""
    hints = [
        (
            "Two children share a sensor id",
            "Two MySensors children collide. Disable an input or a feature.",
        ),
        (
            "FOUR_RELAY has no internal thermometer",
            "Disable the internal temperature sensor for FOUR_RELAY "
            "(this tool normally does that for you).",
        ),
        (
            "no probe is selected",
            "An external probe is enabled but no probe type was chosen; "
            "pass --probe sht30 or --probe dht22.",
        ),
        (
            "PCF8575.h: No such file",
            "Install the PCF8575-lib library: ./tools/gowired.py doctor --fix",
        ),
        (
            "ADCTouch.h: No such file",
            "Install the ADCTouch library: ./tools/gowired.py doctor --fix",
        ),
        (
            "region `text' overflowed",
            "Sketch too large for the flash. Disable a feature.",
        ),
        (
            "section `.data' will not fit",
            "Not enough SRAM. Disable a feature.",
        ),
    ]
    matched = [advice for needle, advice in hints if needle in output]
    if matched:
        print()
        warn("likely cause:")
        for advice in matched:
            print(f"     - {advice}")


SIZE_RE = re.compile(
    r"Sketch uses (\d+) bytes \((\d+)%\).*?Maximum is (\d+)", re.S
)
RAM_RE = re.compile(
    r"Global variables use (\d+) bytes \((\d+)%\).*?leaving (\d+) bytes", re.S
)


def report_size(output: str) -> None:
    m = SIZE_RE.search(output)
    if m:
        used, pct, total = m.groups()
        note = "  <-- tight" if int(pct) >= 90 else ""
        ok(f"flash {used} / {total} B  ({pct}%){note}")
    m = RAM_RE.search(output)
    if m:
        used, pct, free = m.groups()
        note = "  <-- little stack left" if int(free) < 400 else ""
        ok(f"sram  {used} B ({pct}%), {free} B free for stack{note}")


def build(
    tc: Toolchain, s: Settings, mcu: Mcu, *, verbose: bool, out_dir: Optional[Path],
    libraries: Optional[Sequence[str]] = None,
) -> Tuple[bool, Path, Path]:
    fqbn = s.fqbn(mcu)
    staging = Path(tempfile.mkdtemp(prefix="gowired-"))
    sketch = stage_sketch(s, staging)
    build_path = staging / "build"

    info(f"building {s.dev.macro} for {mcu.label}")
    if verbose:
        print(f"     fqbn: {fqbn}")

    cmd = [
        tc.require_cli(), "compile",
        "--fqbn", fqbn,
        "--build-path", str(build_path),
        str(sketch),
    ]
    for lib in libraries or ():
        cmd += ["--libraries", lib]
    if out_dir:
        out_dir.mkdir(parents=True, exist_ok=True)
        cmd += ["--output-dir", str(out_dir)]

    r = run(cmd, timeout=1800)
    output = f"{r.stdout}\n{r.stderr}"

    if r.returncode != 0:
        fail("build failed")
        errs = [l for l in output.splitlines() if re.search(r"\berror\b", l, re.I)]
        for line in (errs or output.splitlines())[:25]:
            print(f"     {line}")
        explain_build_failure(output)
        return False, staging, build_path

    report_size(output)
    if verbose:
        print(output.strip())
    return True, staging, build_path


def flash(
    tc: Toolchain, s: Settings, mcu: Mcu, sketch: Path, build_path: Path, programmer: str
) -> bool:
    heading("Flashing")
    warn(
        "check the adapter's voltage jumper first: 5 V for an MCU, "
        "3.3 V for a Gateway"
    )
    fqbn = s.fqbn(mcu)
    # `upload` takes neither --libraries (it does not compile) nor the default
    # build location: the binaries were produced under our own --build-path, so
    # it has to be told where they are.
    cmd = [
        tc.require_cli(), "upload",
        "--fqbn", fqbn,
        "--programmer", programmer,
        "--input-dir", str(build_path),
        str(sketch),
    ]
    info(f"uploading with {programmer} ...")
    r = run(cmd, timeout=900)
    output = f"{r.stdout}\n{r.stderr}"
    if r.returncode != 0:
        fail("upload failed")
        # arduino-cli prints its whole usage block on a bad flag; keep the
        # signal, drop the manual.
        lines = [
            l for l in output.splitlines()
            if l.strip() and not re.match(r"^\s*(-|--|Usage:|Flags:|Global Flags:)", l)
        ]
        for line in lines[:20]:
            print(f"     {line}")
        print()
        warn("things to check:")
        print("     - is the USBasp connected, and does your user have USB access?")
        print("       (udev: SUBSYSTEM==\"usb\", ATTR{idVendor}==\"16c0\", MODE=\"0666\")")
        print("     - is the programming voltage jumper set correctly?")
        print("     - is the MCU seated in the adapter the right way round?")
        return False
    ok("uploaded")
    print()
    info("to fit the MCU to its shield:")
    print("     1. press CONF on the Gateway LED panel (CONF LED on, bus power off)")
    print("     2. fit the MCU to the shield")
    print("     3. press CONF again (CONF LED off, POWER LED on)")
    return True


# --------------------------------------------------------------------------- #
# Profiles
# --------------------------------------------------------------------------- #


def load_profiles() -> Dict[str, dict]:
    if not PROFILE_FILE.exists():
        return {}
    try:
        return json.loads(PROFILE_FILE.read_text())
    except json.JSONDecodeError as e:
        die(f"{PROFILE_FILE} is not valid JSON: {e}")


def save_profiles(profiles: Dict[str, dict]) -> None:
    PROFILE_FILE.parent.mkdir(parents=True, exist_ok=True)
    PROFILE_FILE.write_text(json.dumps(profiles, indent=2, sort_keys=True) + "\n")


def get_profile(name: str) -> Settings:
    profiles = load_profiles()
    if name not in profiles:
        available = ", ".join(sorted(profiles)) or "none saved yet"
        die(f"no profile named '{name}'. Available: {available}")
    return Settings.from_json(profiles[name])


# --------------------------------------------------------------------------- #
# Interactive prompts
# --------------------------------------------------------------------------- #


def ask_choice(prompt: str, options: Sequence[Tuple[str, str]], default: str) -> str:
    keys = [k for k, _ in options]
    default_index = keys.index(default) + 1 if default in keys else 1
    print()
    print(_c("1", prompt))
    for i, (key, label) in enumerate(options, start=1):
        marker = "*" if i == default_index else " "
        print(f"  {marker}{i}) {label}")
    while True:
        raw = input(f"  choice [{default_index}]: ").strip()
        if not raw:
            return keys[default_index - 1]
        if raw.isdigit() and 1 <= int(raw) <= len(keys):
            return keys[int(raw) - 1]
        if raw in keys:
            return raw
        print("  please pick one of the numbers above")


def ask_bool(prompt: str, default: bool) -> bool:
    suffix = "Y/n" if default else "y/N"
    while True:
        raw = input(f"  {prompt} [{suffix}]: ").strip().lower()
        if not raw:
            return default
        if raw in ("y", "yes"):
            return True
        if raw in ("n", "no"):
            return False


def ask_int(prompt: str, default: int, lo: int, hi: int) -> int:
    while True:
        raw = input(f"  {prompt} [{default}]: ").strip()
        if not raw:
            return default
        try:
            v = int(raw)
        except ValueError:
            print(f"  enter a number between {lo} and {hi}")
            continue
        if lo <= v <= hi:
            return v
        print(f"  must be between {lo} and {hi}")


def ask_str(prompt: str, default: str) -> str:
    raw = input(f"  {prompt} [{default}]: ").strip()
    return raw or default


def wizard(s: Settings) -> Settings:
    heading("Configure")

    s.device = ask_choice(
        "Which board / output?",
        [(d.key, f"{d.macro:<18} {d.label}") for d in DEVICES],
        s.device,
    )
    s.mcu = ask_choice(
        "Which MCU?",
        [("auto", "detect automatically by reading the device signature")]
        + [(m.key, m.label) for m in MCUS],
        s.mcu,
    )

    print()
    print(_c("1", "Identification"))
    s.node_id = ask_str("MySensors node id (number, or AUTO)", s.node_id)
    s.sketch_name = ask_str("Module name", s.sketch_name)
    s.sketch_version = ask_str("Firmware version", s.sketch_version)

    print()
    print(_c("1", "Built-in sensors"))
    s.power_sensor = ask_bool("Built-in current sensor?", s.power_sensor)
    if s.dev.supports_internal_temp:
        s.internal_temperature = ask_bool(
            "Built-in temperature sensor?", s.internal_temperature
        )
    else:
        print(f"  {s.dev.macro} has no thermistor - skipping")
        s.internal_temperature = False
    s.error_reporting = ask_bool("Report faults to the controller?", s.error_reporting)

    s.external_probe = ask_choice(
        "External temperature/humidity probe?",
        [
            ("none", "none"),
            ("sht30", "SHT30  (I2C, needs the arduino-sht library)"),
            ("dht22", "DHT22  (1-wire, needs the DHTlib library)"),
        ],
        s.external_probe,
    )
    if s.external_probe != "none":
        print()
        if ask_bool("Also report temperature to a heating controller?",
                    s.heating_controller_node != 0):
            s.heating_controller_node = ask_int(
                "Heating controller node id", s.heating_controller_node or 1, 1, 254
            )
        else:
            s.heating_controller_node = 0

    print()
    print(_c("1", "Digital inputs"))
    print("  Four general-purpose inputs, independent of the wall switches.")
    for i, cfg in enumerate(s.inputs, start=1):
        cfg.enabled = ask_bool(f"INPUT_{i} wired?", cfg.enabled)
        if cfg.enabled:
            cfg.pullup = ask_bool(
                f"  INPUT_{i}: dry contact to ground (needs pull-up)?", cfg.pullup
            )
            cfg.invert = ask_bool(f"  INPUT_{i}: invert the active level?", cfg.invert)

    print()
    print(_c("1", "Electrical"))
    s.max_current_a = ask_int(
        "Max current [A] (2SSR 3, RGBW/4RelayDin 10)", s.max_current_a, 1, 30
    )
    s.mv_per_amp = ask_int(
        "Sensor sensitivity [mV/A] (ACS712-5A 185, -20A 100)", s.mv_per_amp, 1, 255
    )
    s.receiver_voltage = ask_int(
        "Load voltage [V] (230 mains, 12/24 LED)", s.receiver_voltage, 1, 255
    )

    if s.device == "roller_shutter":
        print()
        print(_c("1", "Roller shutter"))
        s.up_time_s = ask_int("Travel time up [s]", s.up_time_s, 1, 255)
        s.down_time_s = ask_int("Travel time down [s]", s.down_time_s, 1, 255)
    if s.device in ("dimmer", "rgb", "rgbw"):
        print()
        print(_c("1", "Dimming"))
        s.dimming_step = ask_int("Dimming step", s.dimming_step, 1, 100)
        s.dimming_interval_ms = ask_int(
            "Dimming interval [ms]", s.dimming_interval_ms, 0, 255
        )

    return s


# --------------------------------------------------------------------------- #
# Commands
# --------------------------------------------------------------------------- #


def cmd_doctor(args: argparse.Namespace) -> int:
    tc = Toolchain(args.arduino_cli)
    heading("Toolchain")
    problems = 0

    if tc.cli:
        v = run([tc.cli, "version"], timeout=60).stdout.strip()
        ok(f"arduino-cli: {v}")
    elif args.fix:
        fail("arduino-cli not found on PATH")
        if not tc.install_arduino_cli():
            return 1
    else:
        fail("arduino-cli not found on PATH")
        print("     re-run with --fix to install it automatically, or:")
        print(f"     curl -fsSL {ARDUINO_CLI_INSTALLER} | BINDIR=~/.local/bin sh")
        return 1

    # Everything downstream (build, upload) must use the binary we just found or
    # installed, which may not be on PATH yet.
    args.arduino_cli = tc.cli

    if tc.has_minicore():
        ok("MiniCore installed")
    else:
        problems += 1
        fail("MiniCore not installed")
        if args.fix and tc.install_minicore():
            problems -= 1
        elif not args.fix:
            print(f"     arduino-cli core install MiniCore:avr --additional-urls {MINICORE_URL}")

    have = set(tc.installed_libs())
    sketchbook = tc.sketchbook_libs()
    extra_dirs = [Path(d) for d in (getattr(args, "libraries", None) or ())]
    for name, git_url in REQUIRED_LIBS.items():
        found_in = next((d for d in extra_dirs if (d / name).exists()), None)
        if found_in is not None:
            ok(f"library {name}  ({found_in})")
            continue
        if name in have or (sketchbook / name).exists():
            ok(f"library {name}")
            continue
        problems += 1
        fail(f"library {name} missing")
        if args.fix and tc.install_lib(name, git_url):
            problems -= 1
        elif not args.fix:
            if git_url:
                print(f"     git clone --depth 1 {git_url} {sketchbook / name}")
            else:
                print(f"     arduino-cli lib install \"{name}\"")

    avrdude, _ = tc._find_avrdude()
    if avrdude:
        ok(f"avrdude: {avrdude}")
    else:
        warn("avrdude not found - build will work, flashing and MCU detection will not")

    heading("Result")
    if problems:
        fail(f"{problems} problem(s) remain")
        if not args.fix:
            print("     re-run with --fix to install what is missing")
        return 1
    ok("ready to build")
    return 0


def cmd_detect(args: argparse.Namespace) -> int:
    tc = Toolchain(args.arduino_cli)
    heading("MCU detection")
    info("reading the device signature through the programmer ...")
    mcu = tc.detect_mcu(args.programmer)
    if not mcu:
        fail("could not identify the MCU")
        print("     pass --mcu 328p or --mcu 328pb explicitly instead")
        return 1
    ok(f"{mcu.label}  (signature 0x{mcu.signature}, variant={mcu.variant})")
    return 0


def resolve_settings(args: argparse.Namespace) -> Settings:
    s = get_profile(args.profile) if getattr(args, "profile", None) else Settings()

    for attr in (
        "device", "mcu", "bootloader", "node_id", "sketch_name", "sketch_version",
        "max_current", "mv_per_amp", "receiver_voltage", "up_time", "down_time",
        "dimming_step", "dimming_interval", "heating_node",
    ):
        value = getattr(args, attr, None)
        if value is None:
            continue
        mapping = {
            "max_current": "max_current_a",
            "up_time": "up_time_s",
            "down_time": "down_time_s",
            "dimming_interval": "dimming_interval_ms",
            "heating_node": "heating_controller_node",
        }
        setattr(s, mapping.get(attr, attr), value)

    if getattr(args, "probe", None) is not None:
        s.external_probe = args.probe
    for attr, flag in (
        ("power_sensor", "power_sensor"),
        ("internal_temperature", "internal_temp"),
        ("error_reporting", "error_reporting"),
    ):
        value = getattr(args, flag, None)
        if value is not None:
            setattr(s, attr, value)

    if getattr(args, "inputs", None) is not None:
        spec = args.inputs.strip().lower()
        if spec in ("none", ""):
            wanted: List[int] = []
        elif spec == "all":
            wanted = [1, 2, 3, 4]
        else:
            try:
                wanted = [int(x) for x in re.split(r"[,\s]+", spec) if x]
            except ValueError:
                die("--inputs takes 'all', 'none', or a list like 1,2,4")
        for n in wanted:
            if not 1 <= n <= 4:
                die(f"--inputs: {n} is not an input (1-4)")
        for i, cfg in enumerate(s.inputs, start=1):
            cfg.enabled = i in wanted

    return s


def prepare(args: argparse.Namespace, interactive: bool) -> Tuple[Toolchain, Settings, Mcu]:
    tc = Toolchain(args.arduino_cli)
    tc.require_cli()
    s = resolve_settings(args)
    if interactive:
        s = wizard(s)

    for note in s.normalise():
        warn(note)

    if s.mcu == "auto":
        info("detecting the MCU ...")
        detected = tc.detect_mcu(args.programmer)
        if detected:
            ok(f"detected {detected.label}")
            mcu = detected
        elif interactive:
            warn("detection failed - choose manually")
            key = ask_choice(
                "Which MCU?", [(m.key, m.label) for m in MCUS], "328p"
            )
            mcu = MCUS_BY_KEY[key]
        else:
            die(
                "MCU detection failed. Connect the programmer, or pass "
                "--mcu 328p / --mcu 328pb explicitly."
            )
    else:
        mcu = MCUS_BY_KEY[s.mcu]

    heading("Configuration")
    print(s.summary(mcu))
    return tc, s, mcu


def maybe_save(args: argparse.Namespace, s: Settings) -> None:
    name = getattr(args, "save", None)
    if not name:
        return
    profiles = load_profiles()
    profiles[name] = s.to_json()
    save_profiles(profiles)
    ok(f"saved profile '{name}' to {PROFILE_FILE.relative_to(SKETCH_DIR)}")


def cmd_build(args: argparse.Namespace) -> int:
    tc, s, mcu = prepare(args, args.interactive)
    heading("Build")
    success, staging, _build_path = build(
        tc, s, mcu, verbose=args.verbose,
        out_dir=Path(args.output) if args.output else None,
        libraries=args.libraries,
    )
    if not args.keep:
        shutil.rmtree(staging, ignore_errors=True)
    else:
        info(f"staging kept at {staging}")
    if success:
        maybe_save(args, s)
        if args.write_config:
            CONFIG_H.write_text(apply_settings(CONFIG_H.read_text(), s))
            ok("Configuration.h updated in your working tree")
    return 0 if success else 1


def cmd_flash(args: argparse.Namespace) -> int:
    tc, s, mcu = prepare(args, args.interactive)

    if args.run_tests:
        heading("Unit tests")
        if run_tests(quiet=not args.verbose) != 0:
            fail("unit tests failed - not flashing")
            return 1

    heading("Build")
    success, staging, build_path = build(tc, s, mcu, verbose=args.verbose, out_dir=None,
                                        libraries=args.libraries)
    if not success:
        shutil.rmtree(staging, ignore_errors=True)
        return 1

    if args.interactive and not ask_bool("Flash this to the device now?", True):
        info("stopping before upload")
        shutil.rmtree(staging, ignore_errors=True)
        return 0

    flashed = flash(tc, s, mcu, staging / SKETCH_NAME, build_path, args.programmer)
    shutil.rmtree(staging, ignore_errors=True)
    if flashed:
        maybe_save(args, s)
        if args.write_config:
            CONFIG_H.write_text(apply_settings(CONFIG_H.read_text(), s))
            ok("Configuration.h updated in your working tree")
    return 0 if flashed else 1


def run_tests(quiet: bool = True) -> int:
    if not shutil.which("cmake"):
        fail("cmake not found; cannot run the unit tests")
        return 1
    build_dir = SKETCH_DIR / "build-test"
    r = run(
        ["cmake", "-S", str(TEST_DIR), "-B", str(build_dir)],
        timeout=1800,
    )
    if r.returncode != 0:
        fail("cmake configure failed")
        print(r.stdout[-2000:])
        return 1
    r = run(["cmake", "--build", str(build_dir), "-j"], timeout=1800)
    if r.returncode != 0:
        fail("test build failed")
        print(r.stdout[-3000:])
        return 1
    r = run([str(build_dir / "gowired_tests")], timeout=600)
    tail = r.stdout.strip().splitlines()[-3:]
    for line in tail:
        print(f"     {line}")
    if r.returncode == 0:
        ok("unit tests passed")
    return r.returncode


def cmd_test(args: argparse.Namespace) -> int:
    heading("Unit tests")
    return run_tests(quiet=not args.verbose)


def cmd_list(args: argparse.Namespace) -> int:
    heading("Devices")
    for d in DEVICES:
        extra = "" if d.supports_internal_temp else "   (no internal thermometer)"
        print(f"  {d.key:<15} {d.macro:<18} {d.label}{extra}")
    heading("MCUs")
    for m in MCUS:
        print(f"  {m.key:<15} variant={m.variant:<9} sig 0x{m.signature}  {m.label}")
    heading("Probes")
    for k, (macro, lib) in PROBES.items():
        print(f"  {k:<15} {macro or '-':<18} {('library: ' + lib) if lib else ''}")
    profiles = load_profiles()
    heading("Saved profiles")
    if not profiles:
        print("  none yet - add one with:  gowired.py build -d rgbw --save attic")
    for name in sorted(profiles):
        p = Settings.from_json(profiles[name])
        print(f"  {name:<15} {p.dev.macro:<18} mcu={p.mcu}")
    return 0


def cmd_profile(args: argparse.Namespace) -> int:
    profiles = load_profiles()
    if args.action == "list":
        return cmd_list(args)
    if args.action == "show":
        if args.name not in profiles:
            die(f"no profile named '{args.name}'")
        s = Settings.from_json(profiles[args.name])
        heading(f"Profile '{args.name}'")
        print(s.summary(MCUS_BY_KEY.get(s.mcu)))
        return 0
    if args.action == "delete":
        if profiles.pop(args.name, None) is None:
            die(f"no profile named '{args.name}'")
        save_profiles(profiles)
        ok(f"deleted profile '{args.name}'")
        return 0
    if args.action == "save":
        s = wizard(resolve_settings(args))
        for note in s.normalise():
            warn(note)
        profiles[args.name] = s.to_json()
        save_profiles(profiles)
        heading("Saved")
        print(s.summary(MCUS_BY_KEY.get(s.mcu)))
        ok(f"profile '{args.name}' -> {PROFILE_FILE.relative_to(SKETCH_DIR)}")
        return 0
    return 1


# --------------------------------------------------------------------------- #
# CLI
# --------------------------------------------------------------------------- #


def add_settings_args(p: argparse.ArgumentParser) -> None:
    p.add_argument("-p", "--profile", help="start from a saved profile")
    p.add_argument(
        "-d", "--device", choices=list(DEVICES_BY_KEY),
        help="board / output type",
    )
    p.add_argument(
        "-m", "--mcu", choices=["auto", *MCUS_BY_KEY],
        help="MCU; 'auto' reads the device signature (default)",
    )
    p.add_argument("--bootloader", choices=list(BOOTLOADER_CHOICES),
                   help="'yes' for an MCU, 'no' for a Gateway")
    p.add_argument("--node-id", help="MySensors node id, or AUTO")
    p.add_argument("--sketch-name", help="module name shown in the controller")
    p.add_argument("--sketch-version", help="firmware version string")
    p.add_argument("--probe", choices=list(PROBES), help="external probe type")
    p.add_argument("--inputs", help="which digital inputs are wired: all, none, or 1,2,4")
    p.add_argument("--heating-node", type=int, help="mirror temperature to this node")
    p.add_argument("--max-current", type=int, help="[A]")
    p.add_argument("--mv-per-amp", type=int, help="[mV/A]")
    p.add_argument("--receiver-voltage", type=int, help="[V]")
    p.add_argument("--up-time", type=int, help="shutter travel up [s]")
    p.add_argument("--down-time", type=int, help="shutter travel down [s]")
    p.add_argument("--dimming-step", type=int)
    p.add_argument("--dimming-interval", type=int, help="[ms]")

    for name, dest in (
        ("power-sensor", "power_sensor"),
        ("internal-temp", "internal_temp"),
        ("error-reporting", "error_reporting"),
    ):
        g = p.add_mutually_exclusive_group()
        g.add_argument(f"--{name}", dest=dest, action="store_true", default=None)
        g.add_argument(f"--no-{name}", dest=dest, action="store_false", default=None)

    p.add_argument("--save", metavar="NAME", help="save these settings as a profile")
    p.add_argument(
        "--write-config", action="store_true",
        help="also write the settings into Configuration.h in your working tree "
             "(by default the tree is left untouched and a staging copy is built)",
    )


def add_global_args(p: argparse.ArgumentParser, *, suppress: bool) -> None:
    """Options accepted both before and after the subcommand.

    On the subparsers every default is SUPPRESS, so an option the user did not
    repeat there leaves whatever the top-level parser already put in the
    namespace, instead of overwriting it with a default.
    """
    d = argparse.SUPPRESS if suppress else None
    p.add_argument("--arduino-cli", default=d, help="path to the arduino-cli binary")
    p.add_argument(
        "--programmer", default=argparse.SUPPRESS if suppress else "usbasp",
        help="ISP programmer (default: usbasp)",
    )
    p.add_argument(
        "--libraries", action="append", metavar="DIR", default=d,
        help="extra library directory; repeatable. Not normally needed -- "
             "`doctor --fix` installs into your sketchbook.",
    )
    p.add_argument(
        "-v", "--verbose", action="store_true",
        default=argparse.SUPPRESS if suppress else False,
    )


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="gowired.py",
        description="Configure, build and flash GoWired module firmware.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=(
            "examples:\n"
            "  gowired.py                          full interactive run\n"
            "  gowired.py doctor --fix             install missing dependencies\n"
            "  gowired.py detect                   identify the connected MCU\n"
            "  gowired.py build -d rgbw -m 328p    non-interactive build\n"
            "  gowired.py flash -p attic           flash a saved profile\n"
            "  gowired.py profile save attic       create a profile interactively\n"
        ),
    )
    add_global_args(p, suppress=False)
    common = argparse.ArgumentParser(add_help=False)
    add_global_args(common, suppress=True)

    sub = p.add_subparsers(dest="command")

    d = sub.add_parser("doctor", help="check the toolchain and dependencies", parents=[common])
    d.add_argument("--fix", action="store_true", help="install whatever is missing")
    d.set_defaults(func=cmd_doctor)

    det = sub.add_parser("detect", help="identify the connected MCU", parents=[common])
    det.set_defaults(func=cmd_detect)

    b = sub.add_parser("build", help="configure and build", parents=[common])
    add_settings_args(b)
    b.add_argument("-i", "--interactive", action="store_true", help="ask, then build")
    b.add_argument("-o", "--output", help="copy the built binaries here")
    b.add_argument("--keep", action="store_true", help="keep the staging directory")
    b.set_defaults(func=cmd_build)

    f = sub.add_parser("flash", help="configure, build and upload", parents=[common])
    add_settings_args(f)
    f.add_argument("-i", "--interactive", action="store_true")
    f.add_argument("--test", dest="run_tests", action="store_true",
                   help="run the host unit tests first and stop if they fail")
    f.set_defaults(func=cmd_flash)

    t = sub.add_parser("test", help="run the host unit tests", parents=[common])
    t.set_defaults(func=cmd_test)

    l = sub.add_parser("list", help="show devices, MCUs and saved profiles", parents=[common])
    l.set_defaults(func=cmd_list)

    pr = sub.add_parser("profile", help="manage saved configurations", parents=[common])
    pr.add_argument("action", choices=["save", "show", "delete", "list"])
    pr.add_argument("name", nargs="?")
    add_settings_args(pr)
    pr.set_defaults(func=cmd_profile)

    return p


def interactive_main(args: argparse.Namespace) -> int:
    print(_c("1", "GoWired module installer"))
    print(_c("90", f"sketch: {SKETCH_DIR}"))

    doctor_args = argparse.Namespace(
        arduino_cli=args.arduino_cli,
        libraries=getattr(args, "libraries", None),
        fix=False,
    )
    if cmd_doctor(doctor_args) != 0:
        print()
        if not ask_bool("Install the missing dependencies now?", True):
            info("nothing installed - stopping")
            return 1
        doctor_args.fix = True
        if cmd_doctor(doctor_args) != 0:
            print()
            fail("could not install everything automatically")
            print("     follow BUILDING.md section 1, then re-run this tool")
            return 1

    # doctor may have installed arduino-cli somewhere not yet on PATH.
    args.arduino_cli = doctor_args.arduino_cli

    base = {k: v for k, v in vars(args).items() if k not in ("command", "func")}
    flash_args = argparse.Namespace(
        **base,
        interactive=True,
        run_tests=False,
        save=None,
        write_config=False,
        profile=None,
        device=None, mcu=None, bootloader=None, node_id=None,
        sketch_name=None, sketch_version=None, probe=None, inputs=None,
        heating_node=None, max_current=None, mv_per_amp=None,
        receiver_voltage=None, up_time=None, down_time=None,
        dimming_step=None, dimming_interval=None,
        power_sensor=None, internal_temp=None, error_reporting=None,
    )
    rc = cmd_flash(flash_args)

    if rc == 0:
        print()
        if ask_bool("Save these settings as a profile for next time?", False):
            name = ask_str("Profile name", "default")
            profiles = load_profiles()
            profiles[name] = resolve_settings(flash_args).to_json()
            save_profiles(profiles)
            ok(f"next time:  ./tools/gowired.py flash -p {name}")
    return rc


def main(argv: Optional[List[str]] = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)

    if not CONFIG_H.exists():
        die(f"Configuration.h not found at {CONFIG_H} - is this tool still inside the sketch?")

    try:
        if not args.command:
            return interactive_main(args)
        return args.func(args)
    except PatchError as e:
        die(str(e))
    except KeyboardInterrupt:
        print()
        info("cancelled")
        return 130
    except EOFError:
        # stdin closed part-way through a prompt (piped input, or ^D)
        print()
        info("no more input - cancelled")
        return 130


if __name__ == "__main__":
    raise SystemExit(main())
