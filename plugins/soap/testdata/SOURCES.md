# Test data of the SOAP plugin (wave 19, strand T)

Everything here is synthetic and written for this repository (Apache-2.0); nothing is copied
from a vendor, a standard's example or a third party. Namespaces are `urn:requesttailor:test:*`,
endpoints are on loopback only, and no file holds a real name, address or credential.

| File | What it is |
| --- | --- |
| `calculator.wsdl` | WSDL 1.1: document/literal and rpc (`use="encoded"`) operations, a SOAP 1.1 and a SOAP 1.2 binding, an HTTP GET binding (not importable), a one-way operation, a SOAP header, a choice, an optional repeated element, a recursive type and `xs:any`. Endpoints `127.0.0.1:48812`. |
| `types.xsd` | The schema `calculator.wsdl` imports: an enumeration, a simple-content type with attributes, an address, a recursive node. |
| `secure.wsdl` | WSDL 1.1 with a WS-Policy `UsernameToken` and `IncludeTimestamp` assertion. Endpoint `127.0.0.1:48813`. |
| `weather20.wsdl` | WSDL 2.0 with a SOAP 1.2 and a SOAP 1.1 binding. |

The SOAP test service these files describe lives in `tests/common/mod.rs` (loopback only). Ports
48812 to 48817 belong to this strand: 48812 calculator service, 48813 secured service
