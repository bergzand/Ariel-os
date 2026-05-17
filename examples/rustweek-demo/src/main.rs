#![no_main]
#![no_std]

mod pins;

use ariel_os::{
    gpio::{Level, Output},
    hal,
    i2c::controller::{I2cDevice, Kilohertz, highest_freq_in},
    log::*,
    time::{Delay, Duration, Timer},
};
use ariel_os_boards::pins::LedPeripherals;

use bme280::i2c::AsyncBME280;
use embassy_sync::mutex::Mutex;
use scd4x::Scd4xAsync;

mod coap_ext;
mod firmware_ext;
mod suit;

const DELAY: u64 = 5;

#[ariel_os::task(autostart, peripherals)]
async fn temperature(peripherals: pins::TempPeripherals) {
    let mut i2c_config = hal::i2c::controller::Config::default();
    i2c_config.frequency = const { highest_freq_in(Kilohertz::kHz(100)..=Kilohertz::kHz(400)) };
    debug!("Selected frequency: {:?}", i2c_config.frequency);
    let i2c_bus = pins::TempI2c::new(peripherals.i2c_sda, peripherals.i2c_scl, i2c_config);
    let i2c_bus = Mutex::new(i2c_bus);
    let i2c_device = I2cDevice::new(&i2c_bus);

    let mut bme280 = AsyncBME280::new_primary(i2c_device);

    bme280.init(&mut Delay).await.unwrap();

    loop {
        match bme280.measure(&mut Delay).await {
            Ok(measurements) => {
                info!("Relative Humidity = {}%", measurements.humidity);
                info!("Temperature = {} deg C", measurements.temperature);
                info!("Pressure = {} pascals", measurements.pressure);
            }
            Err(e) => {
                error!("Could not read bme280 due to error {:?}", e);
            }
        }

        Timer::after(Duration::from_secs(DELAY)).await;
    }
}

#[ariel_os::task(autostart, peripherals)]
async fn co2(peripherals: pins::Co2Peripherals) {
    let mut i2c_config = hal::i2c::controller::Config::default();
    i2c_config.frequency = const { highest_freq_in(Kilohertz::kHz(100)..=Kilohertz::kHz(400)) };
    debug!("Selected frequency: {:?}", i2c_config.frequency);
    let i2c_bus = pins::Co2I2c::new(peripherals.i2c_sda, peripherals.i2c_scl, i2c_config);
    let i2c_bus = Mutex::new(i2c_bus);
    let i2c_device = I2cDevice::new(&i2c_bus);

    let mut scd4x = Scd4xAsync::new(i2c_device, Delay);
    let _ = scd4x.stop_periodic_measurement().await;
    Timer::after_millis(500).await;
    let _ = scd4x.reinit().await;
    Timer::after_millis(500).await;

    let serial = scd4x.serial_number().await;
    match serial {
        Ok(serial) => info!("SCD4x serial: {}", serial),
        Err(e) => error!("SCD4x: Unable to retrieve serial {:?}", e),
    }

    let _ = scd4x.start_periodic_measurement().await;
    loop {
        Timer::after(Duration::from_secs(DELAY)).await;
        match scd4x.data_ready_status().await {
            Ok(true) => match scd4x.measurement().await {
                Ok(data) => {
                    info!(
                        "CO2: {} ppm, Temperature: {:.1} \u{00b0}C, Humidity: {:.1} %RH",
                        data.co2, data.temperature, data.humidity
                    );
                }
                Err(e) => {
                    error!("SCD4x measurement error: {:?}", e);
                }
            },
            Ok(false) => {}
            Err(e) => {
                error!("SCD4x I2C error: {:?}", e);
            }
        }
    }
}

#[ariel_os::task(autostart, peripherals)]
async fn blinky(peripherals: LedPeripherals) {
    let mut led0 = Output::new(peripherals.led0, Level::Low);

    loop {
        led0.toggle();
        Timer::after_millis(500).await;
    }
}
