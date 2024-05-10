#![no_std]
#![no_main]
#![allow(unused_imports)]

use core::cell::RefCell;
use core::panic::PanicInfo;
use embassy_executor::Spawner;

// GPIO
use embassy_rp::gpio::{Input, Level, Output, Pull};
use embassy_rp::peripherals::{PIN_0, PIN_12, PIN_13, PIN_14, PIN_15, PIN_16, PIN_17, SPI0};

// PWM
use embassy_rp::pwm::{Config as PwmConfig, Pwm};

// ADC
use embassy_rp::adc::{
    Adc, Async, Channel as AdcChannel, Config as AdcConfig, InterruptHandler as InterruptHandlerAdc,
};

// USB
use embassy_rp::usb::{Driver, InterruptHandler};
use embassy_rp::{bind_interrupts, peripherals::USB};
use log::info;

// Channel
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::channel::{Channel, Receiver, Sender};

// Timer
use embassy_time::{Delay, Timer};

// Select futures
use embassy_futures::select::select;
use embassy_futures::select::Either::{First, Second};

// Display
use core::fmt::Write;
use embassy_embedded_hal::shared_bus::blocking::spi::SpiDeviceWithConfig;
use embassy_rp::spi;
use embassy_rp::spi::{Blocking, Spi};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embedded_graphics::mono_font::iso_8859_16::FONT_10X20;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::text::renderer::CharacterStyle;
use embedded_graphics::text::Text;
use heapless::String;
use micro_rand::Random;
use embassy_futures::select;
use embassy_time::Duration;
use embassy_futures::select::select3;
// LCD
use embassy_rp::i2c::{I2c, Config};
use ag_lcd::{Cursor, LcdDisplay};
use port_expander::dev::pcf8574::Pcf8574;
use panic_halt as _;
use shared_bus::NullMutex;
use embassy_rp::peripherals::I2C0;

use no_std_strings;

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
    ADC_IRQ_FIFO => InterruptHandlerAdc;
});

// Channel for button presses
static BUTTON_PRESS: Channel<ThreadModeRawMutex, bool, 1> = Channel::new();


#[embassy_executor::task]
async fn button_listener( mut button: Input<'static>) {
    loop {
        // Wait for button press
        button.wait_for_falling_edge().await;
        // Send a signal that the button has been pressed
        BUTTON_PRESS.send(true).await;
    }
}
#[embassy_executor::task]
async fn game_logic(mut rand: Random, i2c: I2c<'static, I2C0, embassy_rp::i2c::Blocking>) {
    let mut i2c_expander = Pcf8574::new(i2c, true, true, true);
    // Initiate LCD
    let mut lcd: LcdDisplay<_, _> = LcdDisplay::new_pcf8574(&mut i2c_expander, Delay)
    .with_cursor(Cursor::Off)
    .with_reliable_init(10000)
    .build();

    let mut random_int_part = rand.next_int_i64(1, 20);  // Generate the integer part
    let mut random_frac_part = rand.next_int_i64(1, 99);  // Generate the fractional part as another integer
    let mut random_num = random_int_part as f64 + random_frac_part as f64 / 100.0;  // Combine to simulate a floating point
    let mut current_num = 1.00;  // Start from multiplier 1.00
    let mut finished = false;
    let mut money = 1000;
    info!("Random number: {:.2}", random_num);  // Print the number formatted to two decimal places
    // lcd.autoscroll_on();

    let mut currentDelay = 150;  // Initial delay of 500ms
    loop {
        if !finished {
            // Timer to increment number every second
            Timer::after(Duration::from_millis(currentDelay)).await;
            current_num += 0.01;  // Increment by 0.01
            info!("Current number: {:.2}. (Delay: {})", current_num, currentDelay);  // Print the number formatted to two decimal places
            // print to LCD
            lcd.clear();
            let mut s = String::<32>::new();
            write!(s, "Odd: {:.2}", current_num).unwrap();
            lcd.print(&s);
            if current_num > 1.00 && current_num % 1.20 == 0.00 {
                currentDelay -= 50;
            }
            if current_num > random_num {
                info!("You didn't win this time. The number was: {:.2}", random_num);
                lcd.clear();
                lcd.set_position(0, 0);
                let mut s = String::<32>::new();
                write!(s, "Number was: {:.2}", random_num).unwrap();
                lcd.print(&s);
                let mut str = String::<32>::new();
                write!(str, "Waiting 5sec...").unwrap();
                lcd.print(&str);
                Timer::after(Duration::from_secs(5)).await;
                finished = true;
            }
        } else {
            // Game over or reset game state
            // Optionally wait for a signal to restart or end the game session here
            Timer::after(Duration::from_secs(5)).await; // Short delay before reset
            random_int_part = rand.next_int_i64(1, 20);
            random_frac_part = rand.next_int_i64(1, 99);
            random_num = random_int_part as f64 + random_frac_part as f64 / 100.0;
            current_num = 1.00;  // Reset current_num to start at 1.00
            finished = false;
            info!("New random number: {:.2}", random_num);
        }

        // Handle the button press
        match BUTTON_PRESS.try_receive() {
            Ok(true) => {
                if !finished && current_num < random_num {
                    info!("You won! Your multiplier is: {:.2}", current_num);
                    finished = true;  // Stop incrementing, game is won
                } else if !finished {
                    // Current num is greater or equal to random num
                    info!("You didn't win this time. The number was: {:.2}", random_num);
                    finished = true;
                }
            },
            Ok(false) => {
                // Handle if false is ever sent through the channel
            },
            Err(_) => {
                // No button was pressed; ignore or handle logging if necessary
            }
        }
    }
}


// The task used by the serial port driver over USB
#[embassy_executor::task]
async fn logger_task(driver: Driver<'static, USB>) {
    embassy_usb_logger::run!(1024, log::LevelFilter::Info, driver);
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let button = Input::new(p.PIN_1, Pull::Up);
    let rand = Random::new(1024);
    let driver = Driver::new(p.USB, Irqs);

        
    // Initiate SDA and SCL pins
    let sda = p.PIN_4;
    let scl = p.PIN_5;

    // I2C Config
    let mut config = Config::default();
    config.frequency = 400_000;

    // Initiate I2C
    let i2c = I2c::new_blocking(p.I2C0, scl, sda, config.clone());

    spawner.spawn(logger_task(driver)).unwrap();
    spawner.spawn(button_listener(button)).unwrap();
    spawner.spawn(game_logic(rand, i2c)).unwrap();
}
