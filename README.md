# embassy-rp235

Embassy-Beispiele für den **Raspberry Pi Pico 2 (RP2350)** und den **Pico (RP2040)**, das Gegenstück zu
`embassy-xmc-2` (XMC4700 Relax Kit). Gleiche Programme, aber auf der fertigen HAL
`embassy-rp` statt einer selbst geschriebenen.

| Programm | Inhalt |
|---|---|
| `src/main.rs` | LED (GP25) blinkt alle 500 ms, jeder Wechsel wird auf dem Serial Monitor ausgegeben |
| `src/bin/spi_loopback.rs` | SPI-Selbsttest ohne Verdrahtung (interne Rückschleife), 4 Modi × 1/10/20/40 MHz |
| `src/bin/ili9341.rs` | ILI9341-Display über SPI mit `mipidsi` + `embedded-graphics` |
| `src/serial.rs` | `print!` / `println!` über USB (CDC-ACM) |

## Vergleich mit dem XMC4700-Projekt

| | XMC4700 (`embassy-xmc-2`) | RP2350 / RP2040 (`embassy-rp235`) |
|---|---|---|
| HAL | selbst geschrieben (GPIO, CCU40-Zeitgeber, USIC-UART/SPI) | `embassy-rp` |
| Takt | `clocks::init_144mhz()` (PLL vom 12-MHz-Quarz) | `embassy_rp::init` (RP2350 150 MHz, RP2040 125 MHz) |
| Serial Monitor | UART über den J-Link-COM-Port, 115200 Baud | USB-CDC direkt vom Chip, Baudrate egal |
| Tick-Rate | 9 MHz | 1 MHz (Standard von `embassy-time`) |
| Target | `thumbv7em-none-eabihf` (Cortex-M4F) | `thumbv8m.main-none-eabihf` (M33) / `thumbv6m-none-eabi` (M0+) |

## Chip wählen

| Board | Befehl | Target / Feature |
|---|---|---|
| Pico 2 (RP2350) | `cargo run --release [--bin …]` | `thumbv8m.main-none-eabihf`, Feature `rp2350` (Standard) |
| Pico (RP2040) | `cargo run-rp2040 [--bin …]` | `thumbv6m-none-eabi`, Feature `rp2040` |

`cargo run-rp2040` / `cargo build-rp2040` sind Aliase in `.cargo/config.toml`. `build.rs`
wählt `memory-rp2350.x` bzw. `memory-rp2040.x` (mit Second-Stage-Bootloader `.boot2`).

## Flashen (ohne Debug Probe)

Der Runner `tools/elf2uf2.py --flash` wandelt die ELF-Datei in UF2 um und kopiert sie auf das
BOOTSEL-Laufwerk (RPI-RP2 bzw. RP2350, wird bei Bedarf mit `udisksctl` eingehängt):

- **Erstes Mal:** Pico mit gedrückter BOOTSEL-Taste einstecken, dann `cargo run…`.
- **Danach ohne Taste:** Läuft schon eine Firmware aus diesem Projekt, schickt der Runner sie
  per **1200-Baud-Touch** in den Bootloader (`serial.rs` startet bei 1200 Baud
  `rom_data::reset_to_usb_boot`).
- Passt das Board nicht zur Firmware (RP2040 ↔ RP2350), bricht der Runner mit Hinweis ab.
  Bei laufender Firmware erkennt er den Chip am USB-Produktnamen („Pico Serial“ /
  „Pico 2 Serial“) und startet das Board dann gar nicht erst neu.

Mit Debug Probe: den `probe-rs`-Runner in `.cargo/config.toml` einkommentieren.

## Serial Monitor

Der Pico meldet sich per USB als serielle Schnittstelle an (Linux: `/dev/ttyACM*`,
USB-ID `c0de:cafe`, Test-IDs aus den Embassy-Beispielen):

    picocom /dev/ttyACM0

Gesendet wird erst 100 ms, nachdem ein Terminal den Port geöffnet hat (DTR); bis dahin
wird gepuffert (2 KiB). Ohne die Wartezeit gingen am Pico 2 die ersten Zeilen verloren,
während Linux den Port noch einrichtete. Schließen und Wiederöffnen verliert nichts.

**Achtung ModemManager:** Er untersucht neue serielle Geräte in den ersten Sekunden nach dem
Anmelden und liest dabei die ersten Zeilen mit. Abhilfe: Terminal sofort nach dem Flashen
öffnen, oder den Pico für den ModemManager sperren:

    echo 'ATTRS{idVendor}=="c0de", ATTRS{idProduct}=="cafe", ENV{ID_MM_DEVICE_IGNORE}="1"' | sudo tee /etc/udev/rules.d/99-embassy-pico.rules
    sudo udevadm control --reload-rules

## Verdrahtung ILI9341 (SPI0, Pico und Pico 2)

| Display | Pico / Pico 2 | Pin |
|---|---|---|
| VCC, LED | 3V3(OUT) | 36 |
| GND | GND | 23 |
| CS | GP17 | 22 |
| RESET | GP21 | 27 |
| DC | GP20 | 26 |
| SDI/MOSI | GP19 | 25 |
| SCK | GP18 | 24 |
| SDO/MISO | GP16 (optional) | 21 |

## Hinweise

- **Pico W / Pico 2 W:** Die LED hängt dort am WLAN-Chip (nicht GP25) und braucht `cyw43`.
- **SPI-Takt:** Der RP2040 (clk_peri 125 MHz) erreicht statt 20 MHz nur 15,6 MHz; die Demo
  gibt den tatsächlichen Takt aus.
- `memory-rp2350.x` / `memory-rp2040.x` stammen aus den Embassy-Beispielen (`examples/rp235x`, `examples/rp`); `_stext` ist auf 8 Byte
  ausgerichtet, sonst warnt der Linker.
- **Stand (am Board getestet):**
  - Pico 2 (RP2350): Blinken mit Serial-Ausgabe (Board-Zeit = PC-Zeit, 1,0001), Startmeldung
    vollständig, SPI-Selbsttest alle 4 Modi bis 37,5 MHz OK.
  - Pico (RP2040): Blinken mit Serial-Ausgabe (1,0000), SPI-Selbsttest alle 4 Modi bis
    31,25 MHz OK.
  - Beide: Flashen per BOOTSEL und per 1200-Baud-Touch; der Runner lehnt Firmware für den
    falschen Chip ab, ohne das Board neu zu starten.
  - Noch offen: ILI9341-Display (auf keinem der beiden getestet).
