//! `print!` / `println!` auf den Serial Monitor über **USB** (CDC-ACM).
//!
//! Der Pico 2 hat keinen USB-UART-Wandler auf der Platine. Stattdessen meldet sich der
//! RP2350 selbst als serielle Schnittstelle am PC an (unter Linux `/dev/ttyACM*`).
//! Die Baudrate im Terminal ist dabei egal.
//!
//! ```ignore
//! let p = embassy_rp::init(Default::default());
//! serial::init(spawner, p.USB);
//! println!("Hallo {}", 42);
//! ```
//!
//! Die Makros blockieren nie: Der Text landet in einem Puffer (2 KiB), den eine eigene Task
//! über USB verschickt, sobald ein Terminal den Port geöffnet hat (DTR gesetzt). Bis dahin
//! sammelt sich die Ausgabe im Puffer; ist er voll, wird Neues verworfen. Beim Öffnen des
//! Terminals erscheint also auch die Startmeldung noch. `\n` wird zu `\r\n`.

use core::fmt;

use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_rp::peripherals::USB;
use embassy_rp::usb::{Driver, InterruptHandler};
use embassy_rp::{bind_interrupts, Peri};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::pipe::Pipe;
use embassy_time::{Duration, Timer};
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::{Builder, Config, UsbDevice};
use static_cell::StaticCell;

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

type UsbDriver = Driver<'static, USB>;

/// Ausgabepuffer zwischen den Makros und der USB-Task
static OUTPUT: Pipe<CriticalSectionRawMutex, 2048> = Pipe::new();

/// USB-Gerät mit einer seriellen Schnittstelle einrichten und die beiden Tasks starten.
pub fn init(spawner: Spawner, usb: Peri<'static, USB>) {
    let driver = Driver::new(usb, Irqs);

    // 0xc0de:0xcafe sind Test-IDs (wie in den Embassy-Beispielen), nur für Entwicklung.
    let mut config = Config::new(0xc0de, 0xcafe);
    config.manufacturer = Some("Embassy");
    config.product = Some(if cfg!(feature = "rp2040") {
        "Pico Serial"
    } else {
        "Pico 2 Serial"
    });
    config.serial_number = Some("rp2350");
    config.max_power = 100;
    config.max_packet_size_0 = 64;

    static CONFIG_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static CONTROL_BUF: StaticCell<[u8; 64]> = StaticCell::new();
    static STATE: StaticCell<State> = StaticCell::new();

    let mut builder = Builder::new(
        driver,
        config,
        CONFIG_DESC.init([0; 256]),
        BOS_DESC.init([0; 256]),
        &mut [],
        CONTROL_BUF.init([0; 64]),
    );
    let class = CdcAcmClass::new(&mut builder, STATE.init(State::new()), 64);
    let device = builder.build();

    spawner.spawn(usb_task(device).unwrap());
    spawner.spawn(writer_task(class).unwrap());
}

#[embassy_executor::task]
async fn usb_task(mut device: UsbDevice<'static, UsbDriver>) -> ! {
    device.run().await
}

/// Baudrate, mit der der PC einen Neustart in den USB-Bootloader auslöst ("1200-Baud-Touch",
/// wie bei Arduino und dem Pico-SDK). `tools/elf2uf2.py --flash` nutzt das, damit zum
/// Flashen niemand die BOOTSEL-Taste drücken muss.
const BOOTSEL_BAUD: u32 = 1200;

/// Leert den Puffer paketweise (64 Byte) zum PC.
#[embassy_executor::task]
async fn writer_task(class: CdcAcmClass<'static, UsbDriver>) -> ! {
    // Sender + Monitor für DTR/Baudrate; der Empfänger bleibt ungenutzt, aber erhalten.
    let (mut tx, _rx, control) = class.split_with_control();
    let mut buf = [0u8; 64];
    loop {
        tx.wait_connection().await;
        let mut was_open = false;
        loop {
            if tx.line_coding().data_rate() == BOOTSEL_BAUD {
                // Kurz warten, damit die USB-Antwort auf SET_LINE_CODING noch rausgeht
                Timer::after(Duration::from_millis(50)).await;
                embassy_rp::rom_data::reset_to_usb_boot(0, 0);
            }
            // Erst senden, wenn ein Terminal den Port wirklich geöffnet hat (DTR gesetzt).
            // Sonst liest z. B. der ModemManager beim Anmelden die ersten Zeilen und
            // verwirft sie. Bis dahin sammelt sich die Ausgabe im Puffer.
            if !control.dtr() {
                was_open = false;
                control.control_changed().await;
                continue;
            }
            if !was_open {
                // Terminal gerade geöffnet: Linux richtet den Port noch ein. Pakete, die in
                // diesem Moment ankommen, können verloren gehen (am Pico 2 fehlten so die
                // ersten Zeilen). Daher kurz warten.
                was_open = true;
                Timer::after(Duration::from_millis(100)).await;
            }
            // Auf Daten warten, aber auch auf DTR-/Baudraten-Änderungen reagieren.
            // (Ein abgebrochenes `read` verliert keine Daten.)
            match select(OUTPUT.read(&mut buf), control.control_changed()).await {
                Either::First(n) => {
                    if tx.write_packet(&buf[..n]).await.is_err() {
                        break; // USB getrennt: auf neue Verbindung warten
                    }
                }
                Either::Second(()) => {}
            }
        }
    }
}

/// Schreiber hinter den Makros: `\n` → `\r\n`, nie blockierend.
struct Writer;

impl Writer {
    fn put(bytes: &[u8]) {
        let mut rest = bytes;
        while !rest.is_empty() {
            match OUTPUT.try_write(rest) {
                Ok(n) => rest = &rest[n..],
                Err(_) => return, // Puffer voll: Rest verwerfen
            }
        }
    }
}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for (i, line) in s.split('\n').enumerate() {
            if i > 0 {
                Writer::put(b"\r\n");
            }
            Writer::put(line.as_bytes());
        }
        Ok(())
    }
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments<'_>) {
    let _ = fmt::Write::write_fmt(&mut Writer, args);
}

/// Wie `std::print!`, Ausgabe über USB-Serial (siehe [`serial`](crate::serial)).
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::serial::_print(core::format_args!($($arg)*))
    };
}

/// Wie `std::println!`, Ausgabe über USB-Serial (siehe [`serial`](crate::serial)).
#[macro_export]
macro_rules! println {
    () => {
        $crate::print!("\n")
    };
    ($($arg:tt)*) => {
        $crate::serial::_print(core::format_args!("{}\n", core::format_args!($($arg)*)))
    };
}
