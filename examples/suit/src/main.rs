#![no_main]
#![no_std]

use ariel_os::log::{debug, error, info};
use cose_minicbor::{
    cose::{CoseSign, CoseSign1},
    cose_keys::{CoseAlg, CoseKey, CoseKeySetBuilder, Curve, KeyType},
};
use dress_up::SuitManifest;
use minicbor::data::Tagged;

static MANIFEST: &[u8] = include_bytes!("../suit.cbor");
// Raw P256 public key
static PUBKEY_X: &[u8] = &[
    0x1b, 0xf6, 0x75, 0x14, 0xf9, 0x4b, 0x3d, 0x70, 0x3b, 0x2f, 0x85, 0xf6, 0x4e, 0xc8, 0x4b, 0xc0,
    0x01, 0x1b, 0x4a, 0xe6, 0xe5, 0x72, 0x3f, 0x01, 0x86, 0x31, 0xb0, 0x6e, 0x25, 0x0b, 0x8b, 0x08,
];
static PUBKEY_Y: &[u8] = &[
    0x54, 0x9a, 0xd1, 0x1f, 0xb1, 0xc9, 0xe7, 0xb0, 0xf8, 0xa4, 0x2d, 0x66, 0xfd, 0x9e, 0xf4, 0x05,
    0xe0, 0xe6, 0x2f, 0xc1, 0xeb, 0x90, 0x59, 0x2c, 0x3d, 0x54, 0x9a, 0x33, 0x70, 0xcc, 0x53, 0x57,
];

fn build_cose_key() -> CoseKey<'static> {
    let mut key = CoseKey::new(KeyType::Ec2);
    key.alg(CoseAlg::ES256);
    key.crv(Curve::P256).unwrap();
    key.x(PUBKEY_X).expect("invalid x coordinate for key");
    key.y(PUBKEY_Y).expect("invalid y coordinate for key");
    key
}

#[ariel_os::task(autostart)]
async fn run_client_operations() {
    use coap_request::Stack;

    let client = ariel_os::coap::coap_client().await;

    // create cose key structure
    let mut key_builder: CoseKeySetBuilder<100> =
        CoseKeySetBuilder::try_new().expect("no valid builder");
    key_builder
        .push_key(build_cose_key())
        .expect("key set is full");
    let key_bytes = key_builder.into_bytes().expect("bytes not okay");

    let suit = SuitManifest::from_bytes(&MANIFEST);
    let suit = suit
        .authenticate(|cose, payload| {
            debug!("cose length {}, bytes: {:x?}", cose.len(), &cose);
            debug!("payload length {}, bytes: {:x?}", payload.len(), &payload);

            if let Ok(sign1) = minicbor::decode::<Tagged<18, CoseSign1<'_>>>(cose) {
                return Ok(sign1
                    .suit_verify_cose_sign1(Some(payload), &key_bytes)
                    .is_ok());
            } else if let Ok(sign) = minicbor::decode::<Tagged<98, CoseSign<'_>>>(cose) {
                return Ok(sign
                    .suit_verify_cose_sign(Some(payload), &key_bytes)
                    .is_ok());
            }
            error!("unable to verify manifest signature");
            return Ok(false);
        })
        .expect("error authenticating manifest");
    info!("authenticated manifest!");

    let envelope = suit.envelope().expect("invalid envelope");
    let manifest = envelope.manifest().expect("invalid manifest");

    // Corresponding to the fixed network setup, we select a fixed server address; this may need to
    // be updated on hosts that are configured differently.
    let addr = "10.42.0.1:5683"; // IPv4 🔔
    let demoserver = addr.parse().unwrap();

    info!("Sending POST to {}...", demoserver);
    let request = coap_request_implementations::Code::post()
        .with_path("/uppercase")
        .with_request_payload_slice(b"This is Ariel OS")
        .processing_response_payload_through(|p| {
            info!(
                "Uppercase response is {:?}",
                core::str::from_utf8(p).map_err(|_| "not Unicode?")
            );
        });
    let response = client.to(demoserver).request(request).await;
    info!("Response {:?}", response.map_err(|_| "TransportError"));
}
