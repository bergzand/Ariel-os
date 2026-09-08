#![no_main]
#![no_std]

mod pins;

use ariel_os::{
    gpio::{Level, Output}, hal, i2c::controller::{I2cDevice, Kilohertz, highest_freq_in}, identity::device_id_bytes, log::*, time::{Delay, Duration, Timer}
};
use ariel_os_boards::pins::LedPeripherals;

use coap_message::MinimalWritableMessage;
use bme280::i2c::AsyncBME280;
use coap_request::Stack;
use embassy_sync::mutex::Mutex;
use embedded_nal_coap::RequestingCoAPClient;
use scd4x::Scd4xAsync;
use twoten::twoten_buf;
use minicbor::{Encode, Encoder};
use heapless::Vec;

use crate::coap_ext::OptionMessageWriter;

mod coap_ext;
mod firmware_ext;
mod suit;

const DELAY: u64 = 5;

#[derive(Clone)]
struct RdRequest<'a> {
    name: &'a str,
    lt: &'a str,
}

impl<'a> RdRequest<'a> {
    fn new(name: &'a str, lt: &'a str) -> Self {
        Self { name, lt }
    }
}

impl<'a> coap_request::Request<RequestingCoAPClient<'static, 3>> for RdRequest<'a> {
    type Output = Option<()>;

    type Carry = ();

    async fn build_request(
        &mut self,
        request: &mut <RequestingCoAPClient<'static, 3> as Stack>::RequestMessage<'_>,
    ) -> Result<Self::Carry, <RequestingCoAPClient<'static, 3> as Stack>::RequestUnionError> {

        let mut ep: heapless::String<16> = heapless::String::new();
        ep.push_str("ep=").unwrap();
        ep.push_str(self.name).unwrap();
        request.set_code(coap_numbers::code::POST);
        request.add_option_uri_path(".well-known/rd")?;
        request.add_option_uri_query(self.lt)?;
        request.add_option_uri_query(&ep)?;
        Ok(())
    }

    async fn process_response(
        &mut self,
        _response: &<RequestingCoAPClient<'static, 3> as Stack>::ResponseMessage<'_>,
        _carry: Self::Carry,
    ) -> Self::Output {
        Some(())
    }
}

#[ariel_os::task(autostart)]
async fn registration() {
    let name = twoten_buf(device_id_bytes().unwrap().as_ref());
    info!("Name: {}", name);
    let lifetime = "lt=600";
    let request = RdRequest::new(&name, &lifetime);//".well-known/rd");
    let client = ariel_os::coap::coap_client().await;
    let addr = "10.42.0.1:5684"; // IPv4 🔔, port incremented by one
    let rd_server = addr.parse().unwrap();

    loop {
        info!("sending rd");
        let _ = client.to(rd_server).request(request.clone()).await;
        Timer::after(Duration::from_secs(10)).await;
    }
}

#[derive(Encode)]
#[cbor(map)]
struct MeasurementPayload<'a> {
    #[b(0)]
    name: &'a str,
    #[b(2)]
    value: f32,
}

impl<'a> MeasurementPayload<'a> {
    fn new(name: &'a str, value: f32) -> Self {
        Self { name, value }
    }
}

#[derive(Clone)]
struct ClimateRequest {
    temperature: f32,
    relhum: f32,
    pressure: f32
}

impl ClimateRequest {
    fn new(temperature: f32, relhum: f32, pressure: f32) -> Self {
        Self { temperature, relhum, pressure }
    }
}

impl coap_request::Request<RequestingCoAPClient<'static, 3>> for ClimateRequest {
    type Output = Option<()>;

    type Carry = ();

    async fn build_request(
        &mut self,
        request: &mut <RequestingCoAPClient<'static, 3> as Stack>::RequestMessage<'_>,
    ) -> Result<Self::Carry, <RequestingCoAPClient<'static, 3> as Stack>::RequestUnionError> {
        request.set_code(coap_numbers::code::POST);
        request.add_option_uri_path("sensor")?;
        request.add_option_content_format(112)?;
        // add body
        let mut payload: Vec<u8, 64> = Vec::new();
        let mut encoder = Encoder::new(minicbor_adapters::WriteToHeapless(&mut payload));
        let temp = MeasurementPayload::new("b:temp", self.temperature);
        let hum = MeasurementPayload::new("b:hum", self.relhum);
        let press = MeasurementPayload::new("b:press", self.pressure);
        encoder.array(3).unwrap()
            .encode(temp).unwrap()
            .encode(hum).unwrap()
            .encode(press).unwrap();
        let _ = request.set_payload(&payload);
        Ok(())
    }

    async fn process_response(
        &mut self,
        _response: &<RequestingCoAPClient<'static, 3> as Stack>::ResponseMessage<'_>,
        _carry: Self::Carry,
    ) -> Self::Output {
        Some(())
    }
}


#[derive(Clone)]
struct Co2Request {
    co2: u16,
}

impl Co2Request {
    fn new(co2: u16) -> Self {
        Self { co2 }
    }
}

impl coap_request::Request<RequestingCoAPClient<'static, 3>> for Co2Request {
    type Output = Option<()>;

    type Carry = ();

    async fn build_request(
        &mut self,
        request: &mut <RequestingCoAPClient<'static, 3> as Stack>::RequestMessage<'_>,
    ) -> Result<Self::Carry, <RequestingCoAPClient<'static, 3> as Stack>::RequestUnionError> {
        request.set_code(coap_numbers::code::POST);
        request.add_option_uri_path("sensor")?;
        request.add_option_content_format(112)?;
        // add body
        let mut payload: Vec<u8, 32> = Vec::new();
        let mut encoder = Encoder::new(minicbor_adapters::WriteToHeapless(&mut payload));
        let co2= MeasurementPayload::new("c:co2", self.co2 as f32);
        encoder.array(1).unwrap()
            .encode(co2).unwrap();
        let _ = request.set_payload(&payload);
        Ok(())
    }

    async fn process_response(
        &mut self,
        _response: &<RequestingCoAPClient<'static, 3> as Stack>::ResponseMessage<'_>,
        _carry: Self::Carry,
    ) -> Self::Output {
        Some(())
    }
}


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

    let client = ariel_os::coap::coap_client().await;
    let addr = "10.42.0.1:5684"; // IPv4 🔔, port incremented by one
    let rd_server = addr.parse().unwrap();
    loop {
        match bme280.measure(&mut Delay).await {
            Ok(measurements) => {
                info!("Relative Humidity = {}%", measurements.humidity);
                info!("Temperature = {} deg C", measurements.temperature);
                info!("Pressure = {} pascals", measurements.pressure);
                let request = ClimateRequest::new(
                    measurements.temperature,
                    measurements.humidity,
                    measurements.pressure,
                );
                let _ = client.to(rd_server).request(request.clone()).await;
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

    let client = ariel_os::coap::coap_client().await;
    let addr = "10.42.0.1:5684"; // IPv4 🔔, port incremented by one
    let rd_server = addr.parse().unwrap();
    loop {
        Timer::after(Duration::from_secs(DELAY)).await;
        match scd4x.data_ready_status().await {
            Ok(true) => match scd4x.measurement().await {
                Ok(data) => {
                    info!(
                        "CO2: {} ppm, Temperature: {:.1} \u{00b0}C, Humidity: {:.1} %RH",
                        data.co2, data.temperature, data.humidity
                    );
                    let request = Co2Request::new(data.co2);
                    let _ = client.to(rd_server).request(request.clone()).await;
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
        Timer::after_millis(150).await;
    }
}
