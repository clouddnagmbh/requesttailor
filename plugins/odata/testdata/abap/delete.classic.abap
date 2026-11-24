" RequestTailor: Delete sales order
" DELETE /sap/opu/odata/sap/ZRT_SALES_SRV/SalesOrderSet('0500000003')
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
DATA lv_etag TYPE string.
" <set lv_etag to the ETag: header 'etag' of a read of this entity>

lv_uri = `/sap/opu/odata/sap/ZRT_SALES_SRV/SalesOrderSet(` &&
         `'0500000003')`.

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
lo_client->request->set_method( 'DELETE' ).
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
  name  = 'If-Match'
  value = lv_etag ).
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
