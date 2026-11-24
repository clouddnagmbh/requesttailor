" RequestTailor: Two reads in a batch
" POST /sap/opu/odata/sap/ZRT_SALES_SRV/$batch
" Generated, not run against any system: review it before you use it.
" Logon data stays in the RFC destination (SM59, connection type G).

CONSTANTS lc_destination TYPE rfcdest VALUE '<destination>'.
" <RFC destination to https://s4.example.test:44300>

DATA lo_client TYPE REF TO if_http_client.
DATA lv_uri TYPE string.
DATA lv_status TYPE i.
DATA lv_response TYPE string.
DATA lr_data TYPE REF TO data.
DATA lv_csrf TYPE string.
DATA lv_body TYPE string.
DATA(lv_crlf) = cl_abap_char_utilities=>cr_lf.

lv_uri = `/sap/opu/odata/sap/ZRT_SALES_SRV/$batch`.
lv_body = ``.
lv_body = lv_body &&
          `--batch_rt1` &&
          lv_crlf.
lv_body = lv_body &&
          `Content-Type: application/http` &&
          lv_crlf.
lv_body = lv_body &&
          `Content-Transfer-Encoding: binary` &&
          lv_crlf.
lv_body = lv_body &&
          `` &&
          lv_crlf.
lv_body = lv_body &&
          `GET SalesOrderSet('0500000001') HTTP/1.1` &&
          lv_crlf.
lv_body = lv_body &&
          `Accept: application/json` &&
          lv_crlf.
lv_body = lv_body &&
          `` &&
          lv_crlf.
lv_body = lv_body &&
          `` &&
          lv_crlf.
lv_body = lv_body &&
          `--batch_rt1` &&
          lv_crlf.
lv_body = lv_body &&
          `Content-Type: application/http` &&
          lv_crlf.
lv_body = lv_body &&
          `Content-Transfer-Encoding: binary` &&
          lv_crlf.
lv_body = lv_body &&
          `` &&
          lv_crlf.
lv_body = lv_body &&
          `GET SalesOrderSet?$top=2 HTTP/1.1` &&
          lv_crlf.
lv_body = lv_body &&
          `Accept: application/json` &&
          lv_crlf.
lv_body = lv_body &&
          `` &&
          lv_crlf.
lv_body = lv_body &&
          `` &&
          lv_crlf.
lv_body = lv_body &&
          `--batch_rt1--` &&
          lv_crlf.

cl_http_client=>create_by_destination(
  EXPORTING
    destination = lc_destination
  IMPORTING
    client      = lo_client
  EXCEPTIONS
    OTHERS      = 1 ).
IF sy-subrc <> 0.
  RETURN.
ENDIF.
lo_client->propertytype_accept_cookie = if_http_client=>co_enabled.

" 1. Fetch the CSRF token (GET on the service root). It is read at run time.
lo_client->request->set_method( 'GET' ).
cl_http_utility=>set_request_uri(
  request = lo_client->request
  uri     = `/sap/opu/odata/sap/ZRT_SALES_SRV/` ).
lo_client->request->set_header_field(
  name  = 'x-csrf-token'
  value = `fetch` ).
lo_client->send( EXCEPTIONS OTHERS = 1 ).
IF sy-subrc = 0.
  lo_client->receive( EXCEPTIONS OTHERS = 1 ).
ENDIF.
IF sy-subrc <> 0.
  lo_client->close( ).
  RETURN.
ENDIF.
lv_csrf = lo_client->response->get_header_field( 'x-csrf-token' ).
lo_client->refresh_request( ).

" 2. The change, in the same session.
lo_client->request->set_method( 'POST' ).
cl_http_utility=>set_request_uri(
  request = lo_client->request
  uri     = lv_uri ).
lo_client->request->set_header_field(
  name  = 'Accept'
  value = `application/json` ).
lo_client->request->set_header_field(
  name  = 'x-csrf-token'
  value = lv_csrf ).
lo_client->request->set_header_field(
  name  = 'Content-Type'
  value = `multipart/mixed; boundary=batch_rt1` ).
lo_client->request->set_cdata( lv_body ).
lo_client->send( EXCEPTIONS OTHERS = 1 ).
IF sy-subrc = 0.
  lo_client->receive( EXCEPTIONS OTHERS = 1 ).
ENDIF.
IF sy-subrc <> 0.
  lo_client->close( ).
  RETURN.
ENDIF.
lo_client->response->get_status( IMPORTING code = lv_status ).
lv_response = lo_client->response->get_cdata( ).
lo_client->close( ).

IF lv_status BETWEEN 200 AND 299 AND lv_response IS NOT INITIAL.
  /ui2/cl_json=>deserialize(
    EXPORTING json = lv_response
    CHANGING  data = lr_data ).
ENDIF.
" A batch answers 202 with a multipart body: split lv_response at its boundary.
