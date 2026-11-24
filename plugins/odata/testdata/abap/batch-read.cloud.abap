" RequestTailor: Two reads in a batch
" POST /sap/opu/odata/sap/ZRT_SALES_SRV/$batch
" Generated, not run against any system: review it before you use it.
" Logon data stays in the communication arrangement (Fiori app "Communication
" Arrangements"). The outbound service is called through it.

CONSTANTS lc_comm_scenario TYPE if_com_management=>ty_cscn_id
  VALUE '<communication scenario>'.
CONSTANTS lc_service_id TYPE if_com_management=>ty_cs_id
  VALUE '<outbound service>'.
" <scenario and service of the arrangement to https://s4.example.test:44300>

DATA lv_uri TYPE string.
DATA lv_csrf TYPE string.
DATA lv_body TYPE string.
DATA(lv_crlf) = cl_abap_char_utilities=>cr_lf.
DATA lr_data TYPE REF TO data.

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

TRY.
  DATA(lo_destination) = cl_http_destination_provider=>create_by_comm_arrangement(
    comm_scenario = lc_comm_scenario
    service_id    = lc_service_id ).
  DATA(lo_client) = cl_web_http_client_manager=>create_by_http_destination( lo_destination ).
  DATA(lo_request) = lo_client->get_http_request( ).

  " 1. Fetch the CSRF token (GET on the service root). It is read at run time.
  lo_request->set_uri_path( i_uri_path = `/sap/opu/odata/sap/ZRT_SALES_SRV/` ).
  lo_request->set_header_field( i_name = 'x-csrf-token' i_value = `fetch` ).
  DATA(lo_fetch) = lo_client->execute( if_web_http_client=>get ).
  lv_csrf = lo_fetch->get_header_field( 'x-csrf-token' ).

  " 2. The change, in the same session.
  lo_request->set_uri_path( i_uri_path = lv_uri ).
  lo_request->set_header_field(
    i_name  = 'Accept'
    i_value = `application/json` ).
  lo_request->set_header_field(
    i_name  = 'x-csrf-token'
    i_value = lv_csrf ).
  lo_request->set_header_field(
    i_name  = 'Content-Type'
    i_value = `multipart/mixed; boundary=batch_rt1` ).
  lo_request->set_text( lv_body ).
  DATA(lo_response) = lo_client->execute( if_web_http_client=>post ).
  DATA(lv_status) = lo_response->get_status( )-code.
  DATA(lv_response) = lo_response->get_text( ).
  lo_client->close( ).

  IF lv_status BETWEEN 200 AND 299 AND lv_response IS NOT INITIAL.
    /ui2/cl_json=>deserialize(
      EXPORTING json = lv_response
      CHANGING  data = lr_data ).
  ENDIF.
  " A batch answers 202 with a multipart body: split lv_response at its boundary.
CATCH cx_http_dest_provider_error cx_web_http_client_error
    cx_web_message_error INTO DATA(lx_error).
  " Handle the error: lx_error->get_text( ).
ENDTRY.
