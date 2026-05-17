#![no_main]
#![no_std]
#![allow(unsafe_code)]

use ariel_os_boards::pins;

use ariel_os::{
    gpio::{Level, Output},
    time::Timer,
};

use core::cell::RefCell;

use ariel_os::log::*;
use ariel_os::{asynch::Spawner, time::Duration};
use coap_handler::Handler;
use coap_handler_implementations::{
    HandlerBuilder as _, ReportingHandlerBuilder as _, SimpleRendered, new_dispatcher,
};
use coap_message::{MessageOption as _, MinimalWritableMessage, ReadableMessage};
use coap_message_utils::OptionsExt as _;
use coap_request::Stack;
use dress_up::AsyncOperatingHooks;
use embassy_boot::{FirmwareUpdater, FirmwareUpdaterConfig};
use embassy_embedded_hal::adapter::BlockingAsync;
use embassy_embedded_hal::flash::partition::Partition;
use embassy_nrf::nvmc::Nvmc;
use embassy_sync::blocking_mutex::raw::{CriticalSectionRawMutex, NoopRawMutex};
use embassy_sync::mutex::Mutex;
use embassy_sync::signal::Signal;
use embedded_nal_coap::{self, RequestingCoAPClient};
use embedded_storage_async::nor_flash::NorFlash;
//use coap_message_utils::Error;
use heapless::Vec;
use uuid::Uuid;

mod coap_ext;
use coap_ext::{Block2Opt, OptionMessageWriter as _};
mod firmware_ext;
use crate::coap_ext::Block2RequestDataExt;
use crate::firmware_ext::FirmwareUpdaterExt;
use firmware_ext::FromLinkerExt as _;

use embassy_nrf::wdt::{self, Watchdog};

static SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static BUF: critical_section::Mutex<RefCell<Vec<u8, 256>>> =
    critical_section::Mutex::new(RefCell::new(Vec::new()));
const KEYS: &[u8] = include_bytes!("../key_cose_minicbor.cbor");

#[derive(Debug, Clone)]
struct SuitPayloadRequest<'a> {
    block_num: u32,
    path: &'a str,
}

impl<'a> SuitPayloadRequest<'a> {
    fn new(path: &'a str) -> Self {
        Self { block_num: 0, path }
    }

    fn set_blocknum(&self, block_num: u32) -> Self {
        Self {
            block_num,
            path: self.path,
        }
    }
}

impl<'a> coap_request::Request<RequestingCoAPClient<'static, 3>> for SuitPayloadRequest<'a> {
    type Output = Option<(bool, Vec<u8, 64>)>;

    type Carry = (u32, u8);

    async fn build_request(
        &mut self,
        request: &mut <RequestingCoAPClient<'static, 3> as Stack>::RequestMessage<'_>,
    ) -> Result<Self::Carry, <RequestingCoAPClient<'static, 3> as Stack>::RequestUnionError> {
        let szx = 2;
        request.set_code(coap_numbers::code::GET);
        request.add_option_uri_path(self.path)?;
        request.add_option_block2(szx, self.block_num)?;
        Ok((self.block_num, szx))
    }

    async fn process_response(
        &mut self,
        response: &<RequestingCoAPClient<'static, 3> as Stack>::ResponseMessage<'_>,
        carry: Self::Carry,
    ) -> Self::Output {
        let (blocknum, _szx) = carry;
        let Some(block2) = response.options().find(|o| {
            o.number() == coap_numbers::option::BLOCK2 && o.value_uint::<u32>().is_some()
        }) else {
            error!("No block number in response");
            return None;
        };
        let block2: Block2Opt = block2.value_uint::<u32>().unwrap().into();

        if block2.size() != 64 && block2.blocknum() != blocknum {
            error!(
                "Unexpected block {} or size {}",
                block2.blocknum(),
                block2.size()
            );
            return None;
        }

        let out: Vec<u8, 64> = Vec::from_slice(response.payload()).unwrap();
        Some((block2.has_more(), out))
    }
}

struct SuitHook<'a, DFU: NorFlash, STATE: NorFlash> {
    vendor_id: Uuid,
    class_id: Uuid,
    updater: RefCell<
        FirmwareUpdater<'a, Partition<'a, NoopRawMutex, DFU>, Partition<'a, NoopRawMutex, STATE>>,
    >,
    buf: RefCell<Vec<u8, 4096>>,
    offset: RefCell<usize>,
    update_len: RefCell<usize>,
}

impl<'a, DFU: NorFlash, STATE: NorFlash> SuitHook<'a, DFU, STATE> {
    fn new(
        vendor_id: Uuid,
        class_id: Uuid,
        updater: FirmwareUpdater<
            'a,
            Partition<'a, NoopRawMutex, DFU>,
            Partition<'a, NoopRawMutex, STATE>,
        >,
    ) -> Self {
        Self {
            vendor_id,
            class_id,
            updater: RefCell::new(updater),
            buf: RefCell::new(Vec::new()),
            offset: RefCell::new(0),
            update_len: RefCell::new(0),
        }
    }
    async fn handle_suit_manifest(&self, buf: &[u8]) -> Result<(), dress_up::error::Error> {
        let suit = dress_up::SuitManifest::from_bytes(&buf);
        let suit = suit.authenticate(|cose, payload| {
            let cose_untagged = cose
                .get(1..)
                .ok_or(dress_up::error::Error::UnexpectedCbor { position: 0 })?;
            let sign1: cose_minicbor::cose::CoseSign1<'_> = minicbor::decode(cose_untagged)
                .map_err(|_| dress_up::error::Error::UnexpectedCbor { position: 0 })?;
            let res = sign1.suit_verify_cose_sign1(Some(payload), KEYS);

            //let sign1 = CoseSign1::from_bytes(cs).unwrap();
            //let key = CoseKey::from_bytes(KEY).unwrap();
            //let res = sign1.verify_detached(payload, &key, Some(Algorithm::Esp256), None);
            if let Err(e) = res {
                error!("Cose error: {:?} ", e);
                return Err(dress_up::error::Error::AuthenticationFailure);
            }
            Ok(true)
        })?;
        info!("authencated the manifest :)");
        let envelope = suit.envelope()?;
        let manifest = envelope.manifest()?;
        info!(
            "Manifest version {} sequence number {}",
            manifest.version()?,
            manifest.sequence_number()?
        );

        if let Err(e) = manifest.async_execute_payload_installation(self).await {
            error!("Could not install payload: {:?}", e);
        }
        info!("Completed manifest processing");
        Ok(())
    }
}

impl<'a, DFU: NorFlash, STATE: NorFlash> AsyncOperatingHooks for SuitHook<'a, DFU, STATE> {
    type ReadWriteBufferSize = generic_array::typenum::U64;

    async fn match_vendor_id(
        &self,
        uuid: Uuid,
        _component: &dress_up::component::Component<'_>,
    ) -> Result<bool, dress_up::error::Error> {
        Ok(self.vendor_id == uuid)
    }

    async fn match_class_id(
        &self,
        uuid: Uuid,
        _component: &dress_up::component::Component<'_>,
    ) -> Result<bool, dress_up::error::Error> {
        Ok(self.class_id == uuid)
    }

    async fn component_read(
        &self,
        _component: &dress_up::component::Component<'_>,
        _slot: Option<u64>,
        offset: usize,
        bytes: &mut [u8],
    ) -> Result<(), dress_up::error::Error> {
        self.updater
            .borrow_mut()
            .read_dfu(offset as u32, bytes)
            .await
            .map_err(|_| dress_up::error::Error::InvalidCommandSequence { position: 0 })
    }

    async fn component_write(
        &self,
        _component: &dress_up::component::Component<'_>,
        _slot: Option<u64>,
        _offset: usize,
        _bytes: &[u8],
    ) -> Result<(), dress_up::error::Error> {
        todo!()
    }

    async fn component_size(
        &self,
        _component: &dress_up::component::Component<'_>,
    ) -> Result<usize, dress_up::error::Error> {
        Ok(*self.update_len.borrow())
    }

    async fn component_capacity(
        &self,
        _component: &dress_up::component::Component<'_>,
    ) -> Result<usize, dress_up::error::Error> {
        Ok(self.updater.borrow().capacity())
    }

    async fn fetch(
        &self,
        _component: &dress_up::component::Component<'_>,
        _slot: Option<u64>,
        uri: &str,
    ) -> Result<(), dress_up::error::Error> {
        info!("Fetching component from \"{}\"!", uri);
        let mut blocknum = 0u32;
        let mut more = true;
        let request = SuitPayloadRequest::new(uri);
        let client = ariel_os::coap::coap_client().await;
        let addr = "10.42.0.1:5683"; // IPv4 🔔
        let suit_server = addr.parse().unwrap();
        while more {
            let cur_request = request.set_blocknum(blocknum);
            let req = client.to(suit_server).request(cur_request).await;
            if let Ok(Some((resp_more, data))) = req {
                let mut buffer = self.buf.borrow_mut();
                if buffer.is_empty() {
                    self.offset.replace(blocknum as usize * 64);
                }
                if buffer.extend_from_slice(&data).is_err() {
                    error!("Error extending buffer for data");
                    return Err(dress_up::error::Error::ConditionMatchFail { position: 0 });
                }
                if buffer.is_full() || !resp_more {
                    self.update_len.replace_with(|f| *f + buffer.len());
                    let res = self
                        .updater
                        .borrow_mut()
                        .write_firmware(*self.offset.borrow(), &buffer)
                        .await;
                    if let Err(e) = res {
                        error!("Failed to write block {e:?}");
                        return Err(dress_up::error::Error::ConditionMatchFail { position: 0 });
                    }
                    buffer.clear();
                }
                more = resp_more;
                blocknum += 1;
            } else {
                error!("Error installing payload");
                return Err(dress_up::error::Error::ConditionMatchFail { position: 0 });
            }
        }
        self.offset.replace(0);
        let _ = self.updater.borrow_mut().mark_updated().await;
        Ok(())
    }
}

#[derive(Debug, Default)]
struct SuitHandler {}

impl SuitHandler {}

impl Handler for SuitHandler {
    type RequestData = (Option<u32>, u8);
    type ExtractRequestError = coap_message_utils::Error;
    type BuildResponseError<M: MinimalWritableMessage> = coap_message_utils::Error;

    fn extract_request_data<M: ReadableMessage>(
        &mut self,
        request: &M,
    ) -> Result<Self::RequestData, Self::ExtractRequestError> {
        match request.code().into() {
            coap_numbers::code::PUT => {
                let mut block1: Option<u32> = None;
                if SIGNAL.signaled() {
                    return Err(coap_message_utils::Error::service_unavailable());
                }
                request
                    .options()
                    .filter(|o| {
                        if o.number() == coap_numbers::option::BLOCK1
                            && let Some(n) = o.value_uint()
                            && block1.is_none()
                        {
                            block1 = Some(n);
                            false
                        } else {
                            true
                        }
                    })
                    .ignore_elective_others()?;
                let block1 = block1.unwrap_or(0);

                let szx = block1 & 0x7;
                let blocksize = 1 << (4 + szx);
                let offset = (block1 >> 4) * blocksize;

                let res = critical_section::with(|cs| {
                    let mut buf = BUF.borrow(cs).borrow_mut();
                    if offset == 0 {
                        buf.clear();
                    }
                    buf.extend_from_slice(request.payload())
                        .map_err(|_| coap_message_utils::Error::bad_request())
                });
                res?;

                if block1 & 0x8 == 0x8 {
                    Ok((Some(block1), coap_numbers::code::CONTINUE))
                } else {
                    SIGNAL.signal(());
                    Ok((Some(block1), coap_numbers::code::CHANGED))
                }
            }
            _ => Err(coap_message_utils::Error::method_not_allowed()),
        }
    }

    fn estimate_length(&mut self, _request: &Self::RequestData) -> usize {
        1
    }

    fn build_response<M: coap_message::MutableWritableMessage>(
        &mut self,
        response: &mut M,
        request: Self::RequestData,
    ) -> Result<(), Self::BuildResponseError<M>> {
        use coap_message::{Code as _, OptionNumber as _};

        let (block1, code) = request;

        response.set_code(M::Code::new(code).map_err(coap_message_utils::Error::from_unionerror)?);
        if let Some(block1) = block1 {
            response
                .add_option_uint(
                    M::OptionNumber::new(coap_numbers::option::BLOCK1)
                        .map_err(coap_message_utils::Error::from_unionerror)?,
                    block1,
                )
                .map_err(coap_message_utils::Error::from_unionerror)?;
        }
        Ok(())
    }
}

#[ariel_os::task]
async fn suit_handler() {
    let vendor_id = uuid::uuid!("019c9a95-f6cb-71a7-a0a6-aac148fc4743");
    let class_id = uuid::uuid!("019c9a96-347b-7d98-acc9-b90117f4a665");

    let nvmc = Nvmc::new(unsafe { ariel_os_nrf::peripherals::NVMC::steal() });
    let nvmc = Mutex::new(BlockingAsync::new(nvmc));
    let config = FirmwareUpdaterConfig::from_arielos_linkerfile(&nvmc, &nvmc);
    let mut magic = [0; 4];
    let mut updater = FirmwareUpdater::new(config, &mut magic);
    let res = updater.mark_booted().await;
    info!("Update state marked as booted: {:?}", res);

    let suit_hook = SuitHook::new(vendor_id, class_id, updater);
    loop {
        SIGNAL.wait().await;
        {
            let buf = critical_section::with(|cs| BUF.borrow(cs).take());
            info!("Signalled buffer of {:?}", buf.len());
            let res = suit_hook.handle_suit_manifest(&buf).await;
            if let Err(e) = res {
                error!("error handling manifest: {:?}", e);
            }
        }
        SIGNAL.reset();
    }
}

#[ariel_os::task]
async fn coap_run() {
    let handler = new_dispatcher()
        // We offer a single resource: /hello, which responds just with a text string.
        .at_with_attributes(&["suit"], &[], SuitHandler::default())
        .with_wkc()
        .at(
            &["hello"],
            SimpleRendered("Hello from firmware updated Ariel OS!"),
        );

    ariel_os::coap::coap_run(handler).await;
}

#[ariel_os::task]
async fn petter(mut handle: embassy_nrf::wdt::WatchdogHandle) {
    loop {
        if !handle.is_pet() {
            handle.pet();
        }
        Timer::after(Duration::from_millis(1000)).await;
    }
}

#[ariel_os::task(autostart, peripherals)]
async fn blinky(peripherals: pins::LedPeripherals) {
    let mut led0 = Output::new(peripherals.led0, Level::Low);

    loop {
        led0.toggle();
        Timer::after_millis(500).await;
    }
}

#[ariel_os::spawner(autostart)]
fn main(spawner: Spawner) {
    info!("Back to plain");
    // stealing the watchdog to pet it later.
    let watchdog = unsafe { ariel_os_nrf::peripherals::WDT::steal() };
    let wdt_config = wdt::Config::try_new(&watchdog).unwrap();
    let (_wdt, [wdt_handle]) = match Watchdog::try_new(watchdog, wdt_config) {
        Ok(x) => x,
        Err(_) => {
            // Watchdog already active with the wrong number of handles, waiting for it to timeout...
            ariel_os::power::reboot()
        }
    };

    spawner.spawn(petter(wdt_handle)).unwrap();
    spawner.spawn(coap_run()).unwrap();
    spawner.spawn(suit_handler()).unwrap();
}
