#!/usr/bin/env python3
"""ELF -> UF2 für RP2040 und RP2350, optional direkt aufs BOOTSEL-Laufwerk (ohne picotool).

    python3 tools/elf2uf2.py <firmware.elf> [<firmware.uf2>]   # nur umwandeln
    python3 tools/elf2uf2.py --flash <firmware.elf>             # umwandeln + flashen

Mit `--flash` (so ruft `cargo run` das Skript auf, siehe .cargo/config.toml):
  1. Chip aus dem Target-Pfad der ELF-Datei: `thumbv6m` = RP2040, sonst RP2350.
  2. BOOTSEL-Laufwerk suchen (RPI-RP2 bzw. RP2350), bei Bedarf mit `udisksctl` einhängen.
     Läuft schon eine Firmware aus diesem Projekt (USB-ID c0de:cafe), wird sie per
     1200-Baud-Touch in den Bootloader geschickt; die BOOTSEL-Taste ist dann unnötig.
  3. Prüfen, ob das Board (INFO_UF2.TXT) zum Chip passt; sonst Abbruch.
  4. UF2 kopieren; danach meldet sich die Firmware als /dev/ttyACM* (USB-Serial).

Die UF2-Blöcke (256 Byte Nutzdaten) entstehen aus den ladbaren Segmenten (PT_LOAD) an ihren
physischen Flash-Adressen, mit der Family-ID des Chips.
"""
import argparse
import glob
import os
import shutil
import struct
import subprocess
import sys
import time

UF2_MAGIC0, UF2_MAGIC1, UF2_MAGIC_END = 0x0A324655, 0x9E5D5157, 0x0AB16F30
FLAG_FAMILY_ID = 0x00002000
FAMILY = {"rp2040": 0xE48BFF56, "rp2350": 0xE48BFF59}  # RP2350: ARM Secure
LABEL = {"rp2040": "RPI-RP2", "rp2350": "RP2350"}
FLASH_START, FLASH_END = 0x10000000, 0x12000000
BLOCK = 256


def load_segments(path):
    data = open(path, "rb").read()
    if data[:4] != b"\x7fELF" or data[4] != 1 or data[5] != 1:
        sys.exit("keine 32-Bit-Little-Endian-ELF-Datei")
    e_phoff, = struct.unpack_from("<I", data, 28)
    e_phentsize, e_phnum = struct.unpack_from("<HH", data, 42)
    image = {}
    for i in range(e_phnum):
        p_type, p_offset, _vaddr, p_paddr, p_filesz, _memsz, _flags, _align = struct.unpack_from(
            "<IIIIIIII", data, e_phoff + i * e_phentsize)
        if p_type != 1 or p_filesz == 0:  # nur PT_LOAD mit Inhalt
            continue
        if not (FLASH_START <= p_paddr and p_paddr + p_filesz <= FLASH_END):
            sys.exit(f"Segment bei 0x{p_paddr:08x} liegt nicht im Flash")
        for off, byte in enumerate(data[p_offset:p_offset + p_filesz]):
            image[p_paddr + off] = byte
    if not image:
        sys.exit("keine ladbaren Segmente gefunden")
    return image


def build_uf2(image, family):
    pages = sorted({addr & ~(BLOCK - 1) for addr in image})
    blocks = []
    for n, page in enumerate(pages):
        payload = bytes(image.get(page + i, 0) for i in range(BLOCK))
        header = struct.pack("<IIIIIIII", UF2_MAGIC0, UF2_MAGIC1, FLAG_FAMILY_ID, page, BLOCK,
                             n, len(pages), family)
        blocks.append(header + payload + b"\x00" * (476 - BLOCK) + struct.pack("<I", UF2_MAGIC_END))
    return b"".join(blocks), pages


def chip_of(elf_path):
    return "rp2040" if "thumbv6m" in os.path.abspath(elf_path) else "rp2350"


def board_model(mount):
    """'rp2040' / 'rp2350' laut INFO_UF2.TXT, sonst None."""
    try:
        info = open(os.path.join(mount, "INFO_UF2.TXT")).read()
    except OSError:
        return None
    if "RP2350" in info:
        return "rp2350"
    if "RPI-RP2" in info or "Raspberry Pi RP2" in info:
        return "rp2040"
    return None


def find_drive():
    """Eingehängtes BOOTSEL-Laufwerk suchen, notfalls einhängen."""
    user = os.environ.get("USER", "")
    for base in (f"/run/media/{user}", f"/media/{user}", "/media"):
        for mount in glob.glob(os.path.join(base, "*")):
            if board_model(mount):
                return mount
    # Nicht eingehängt: Blockgerät mit passendem Label suchen und per udisksctl einhängen
    try:
        out = subprocess.run(["lsblk", "-rno", "PATH,LABEL"], capture_output=True, text=True).stdout
    except OSError:
        return None
    for line in out.splitlines():
        parts = line.split(" ", 1)
        if len(parts) == 2 and parts[1] in LABEL.values():
            res = subprocess.run(["udisksctl", "mount", "-b", parts[0]], capture_output=True, text=True)
            if res.returncode == 0 and " at " in res.stdout:
                return res.stdout.strip().split(" at ", 1)[1].rstrip(".")
    return None


PRODUCT = {"rp2040": "Pico Serial", "rp2350": "Pico 2 Serial"}  # siehe src/serial.rs


def touch_1200(chip):
    """Laufende Firmware (USB-ID c0de:cafe) per 1200-Baud-Touch in den Bootloader schicken.

    Läuft die Firmware auf dem anderen Chip (USB-Produktname), wird abgebrochen, ohne das
    Board neu zu starten. Gibt True zurück, wenn ein Port angestoßen wurde.
    """
    import termios
    found = False
    for tty in glob.glob("/sys/class/tty/ttyACM*"):
        usb_dev = os.path.realpath(os.path.join(tty, "device", ".."))
        try:
            vid = open(os.path.join(usb_dev, "idVendor")).read().strip()
            pid = open(os.path.join(usb_dev, "idProduct")).read().strip()
        except OSError:
            continue
        if (vid, pid) != ("c0de", "cafe"):
            continue
        try:
            product = open(os.path.join(usb_dev, "product")).read().strip()
        except OSError:
            product = ""
        other = "rp2350" if chip == "rp2040" else "rp2040"
        if product == PRODUCT[other]:
            hint = "cargo run-rp2040" if other == "rp2040" else "cargo run --release"
            sys.exit(f"Angeschlossen ist ein {other.upper()} ({product}), die Firmware ist für "
                     f"{chip.upper()}. Für dieses Board: {hint}")
        dev = "/dev/" + os.path.basename(tty)
        try:
            fd = os.open(dev, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
            attrs = termios.tcgetattr(fd)
            attrs[4] = attrs[5] = termios.B1200  # ispeed, ospeed
            termios.tcsetattr(fd, termios.TCSANOW, attrs)
            os.close(fd)
            print(f"1200-Baud-Touch auf {dev}: Pico startet in den Bootloader ...")
            found = True
        except OSError as e:
            print(f"{dev}: {e}")
    return found


def flash(elf, uf2_path):
    chip = chip_of(elf)
    uf2, pages = build_uf2(load_segments(elf), FAMILY[chip])
    open(uf2_path, "wb").write(uf2)
    print(f"{os.path.basename(uf2_path)}: {chip.upper()}, {len(pages)} Blöcke, "
          f"0x{pages[0]:08x}..0x{pages[-1] + BLOCK:08x}")

    mount = find_drive()
    if not mount and touch_1200(chip):
        for _ in range(40):
            time.sleep(0.25)
            mount = find_drive()
            if mount:
                break
    if not mount:
        print("Warte auf BOOTSEL-Laufwerk: Pico mit gedrückter BOOTSEL-Taste einstecken ...")
        for _ in range(30):
            time.sleep(1)
            mount = find_drive()
            if mount:
                break
    if not mount:
        sys.exit("Kein BOOTSEL-Laufwerk gefunden (RPI-RP2 / RP2350).")
    board = board_model(mount)
    if board != chip:
        hint = ("cargo run-rp2040" if board == "rp2040" else "cargo run --release")
        sys.exit(f"Angeschlossen ist ein {board.upper()}, die Firmware ist für {chip.upper()}. "
                 f"Für dieses Board: {hint}")

    before = set(glob.glob("/dev/ttyACM*"))
    shutil.copyfile(uf2_path, os.path.join(mount, os.path.basename(uf2_path)))
    os.sync()
    print(f"Geflasht über {mount}")
    for _ in range(50):
        time.sleep(0.2)
        new = sorted(set(glob.glob("/dev/ttyACM*")) - before)
        if new:
            print(f"Serial Monitor: {new[0]}   (z. B. picocom {new[0]})")
            return
    print("Kein neuer /dev/ttyACM* (Programm ohne USB-Serial?)")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--flash", action="store_true", help="auf das BOOTSEL-Laufwerk kopieren")
    ap.add_argument("elf")
    ap.add_argument("uf2", nargs="?")
    ap.add_argument("rest", nargs=argparse.REMAINDER, help=argparse.SUPPRESS)
    a = ap.parse_args()
    uf2_path = a.uf2 if (a.uf2 and a.uf2.endswith(".uf2")) else a.elf + ".uf2"
    if a.flash:
        flash(a.elf, uf2_path)
    else:
        uf2, pages = build_uf2(load_segments(a.elf), FAMILY[chip_of(a.elf)])
        open(uf2_path, "wb").write(uf2)
        print(f"{uf2_path}: {len(pages)} Blöcke")


if __name__ == "__main__":
    main()
