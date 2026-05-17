#![allow(unsafe_code)]

use ariel_os::log::*;
use embassy_boot::{FirmwareUpdater, FirmwareUpdaterConfig};
use embassy_embedded_hal::flash::partition::Partition;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embedded_storage_async::nor_flash::NorFlash;

pub trait FromLinkerExt<'a, DFU, STATE> {
    fn from_arielos_linkerfile(
        dfu_flash: &'a embassy_sync::mutex::Mutex<NoopRawMutex, DFU>,
        state_flash: &'a embassy_sync::mutex::Mutex<NoopRawMutex, STATE>,
    ) -> Self;
}

impl<'a, DFU: NorFlash, STATE: NorFlash> FromLinkerExt<'a, DFU, STATE>
    for FirmwareUpdaterConfig<Partition<'a, NoopRawMutex, DFU>, Partition<'a, NoopRawMutex, STATE>>
{
    fn from_arielos_linkerfile(
        dfu_flash: &'a embassy_sync::mutex::Mutex<NoopRawMutex, DFU>,
        state_flash: &'a embassy_sync::mutex::Mutex<NoopRawMutex, STATE>,
    ) -> Self {
        unsafe extern "C" {
            static _bootloader_state_start: u32;
            static _bootloader_state_length: u32;
            static _DFU_start: u32;
            static _DFU_length: u32;
        }

        let dfu = unsafe {
            let start = &_DFU_start as *const u32 as u32;
            let end = start + &_DFU_length as *const u32 as u32;
            trace!("DFU: 0x{:x} - 0x{:x}", start, end);

            Partition::new(dfu_flash, start, end - start)
        };
        let state = unsafe {
            let start = &_bootloader_state_start as *const u32 as u32;
            let end = start + &_bootloader_state_length as *const u32 as u32;
            trace!("STATE: 0x{:x} - 0x{:x}", start, end);

            Partition::new(state_flash, start, end - start)
        };

        Self { dfu, state }
    }
}

pub trait FirmwareUpdaterExt {
    fn capacity(&self) -> usize;
}

impl<'a, DFU: NorFlash, STATE: NorFlash> FirmwareUpdaterExt for FirmwareUpdater<'a, DFU, STATE> {
    fn capacity(&self) -> usize {
        0x7E000
    }
}
