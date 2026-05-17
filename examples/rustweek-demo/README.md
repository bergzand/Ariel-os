# SUIT update

## About

This application demos SUIT OTA updates

The server offers a simple resource, `/hello`, which returns a friendly message.
A second endpoint is for SUIT: `/suit`, this accepts a manifest over PUT.

The default policy allows access to all the resources,

## Running

* Run on any board with networking, eg. `laze build -b particle-xenon run`.
* [Set up networking](../README.md#networking).
* Run `aiocoap-client`
  to list the resources of the device:

  ```sh
  $ pipx install 'aiocoap[oscore,prettyprint]'
  $ aiocoap-client coap://10.42.0.61/.well-known/core --credentials client.diag
  # application/link-format content was re-formatted
  </hello>
  ```

  If you prefer not to install the CoAP client, you can
  replace any call to `aiocoap-client` with `pipx run --spec 'aiocoap[oscore,prettyprint]' aiocoap-client` instead.

  The output tells you there is a `/hello` resource, so read that next:

  ```sh
  $ aiocoap-client coap://10.42.0.61/hello --credentials client.diag
  Hello from Ariel OS
  ```

### Submitting the manifest

```sh
$ aiocoap-client coap://10.42.0.61/suit -m PUT --payload-initial-szx 3 --payload @suit.cbor
```

### Producing a manifest

#### Obtain the binary code

```
objcopy -Obinary build/bin/nrf52840dk/cargo/thumbv7em-none-eabihf/release/suit-update suit-update.bin
```

Make sure the binary can be hosted by the aiocoap-fileserver

#### Create the manifest

Template

```
{
    "manifest-version": 1,
    "manifest-sequence-number": 7,
    "components" : [
        {
            "install-id" : ["00"],
            "install-digest": {
                "algorithm-id": "sha256",
                "digest-bytes": "8894cc19182246c801a1c1581f9f35de1fd233dc15690ebfe446e730d604dfd7"
            },
            "uri": "suit_update.bin",
            "vendor-id" : "019c9a95-f6cb-71a7-a0a6-aac148fc4743",
            "class-id" : "019c9a96-347b-7d98-acc9-b90117f4a665",
            "install-on-download" : true
        }
    ]
}
```

Create:
```
suit-tool create -i input.json -o output.cbor
```
sign:

```
suit-tool



## Further references

There is a [chapter in the book](https://ariel-os.github.io/ariel-os/dev/docs/book/tooling/coap.html)
that describes more concepts and background.


