" RequestTailor: Open orders with items
" GET /sap/opu/odata/sap/ZRT_SALES_SRV/SalesOrderSet
" Generated, not run against any system: review it before you use it.
" Logon data stays in the communication arrangement (Fiori app "Communication
" Arrangements"). The outbound service is called through it.

CONSTANTS lc_comm_scenario TYPE if_com_management=>ty_cscn_id
  VALUE '<communication scenario>'.
CONSTANTS lc_service_id TYPE if_com_management=>ty_cs_id
  VALUE '<outbound service>'.
" <scenario and service of the arrangement to https://s4.example.test:44300>

DATA lv_uri TYPE string.
DATA lr_data TYPE REF TO data.

lv_uri = `/sap/opu/odata/sap/ZRT_SALES_SRV/SalesOrderSet?$filter=` &&
         `GrossAmount%20gt%20100M&$expand=ToItems&$top=5&$select=` &&
         `SalesOrderID,CustomerName,GrossAmount&sap-client=100`.

TRY.
  DATA(lo_destination) = cl_http_destination_provider=>create_by_comm_arrangement(
    comm_scenario = lc_comm_scenario
    service_id    = lc_service_id ).
  DATA(lo_client) = cl_web_http_client_manager=>create_by_http_destination( lo_destination ).
  DATA(lo_request) = lo_client->get_http_request( ).

  lo_request->set_uri_path( i_uri_path = lv_uri ).
  lo_request->set_header_field(
    i_name  = 'Accept'
    i_value = `application/json` ).
  DATA(lo_response) = lo_client->execute( if_web_http_client=>get ).
  DATA(lv_status) = lo_response->get_status( )-code.
  DATA(lv_response) = lo_response->get_text( ).
  lo_client->close( ).

  IF lv_status BETWEEN 200 AND 299 AND lv_response IS NOT INITIAL.
    /ui2/cl_json=>deserialize(
      EXPORTING json = lv_response
      CHANGING  data = lr_data ).
  ENDIF.
CATCH cx_http_dest_provider_error cx_web_http_client_error
    cx_web_message_error INTO DATA(lx_error).
  " Handle the error: lx_error->get_text( ).
ENDTRY.
