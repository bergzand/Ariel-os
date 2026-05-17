use ariel_os::hal::{i2c, peripherals};

#[cfg(any(context = "nrf52840"))]
pub type TempI2c = i2c::controller::TWISPI1;
#[cfg(any(context = "nrf52840"))]
pub type Co2I2c = i2c::controller::TWISPI0;
#[cfg(any(context = "nrf52840"))]
ariel_os::hal::define_peripherals!(TempPeripherals {
    i2c_sda: P1_14,
    i2c_scl: P1_15,
});
#[cfg(any(context = "nrf52840"))]
ariel_os::hal::define_peripherals!(Co2Peripherals {
    i2c_sda: P0_26,
    i2c_scl: P0_27,
});
