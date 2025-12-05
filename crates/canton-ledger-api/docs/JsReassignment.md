# JsReassignment

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**update_id** | **String** | Assigned by the server. Useful for correlating logs. Must be a valid LedgerString (as described in ``value.proto``). Required | 
**command_id** | **String** | The ID of the command which resulted in this reassignment. Missing for everyone except the submitting party on the submitting participant. Must be a valid LedgerString (as described in ``value.proto``). Optional | 
**workflow_id** | **String** | The workflow ID used in reassignment command submission. Only set if the ``workflow_id`` for the command was set. Must be a valid LedgerString (as described in ``value.proto``). Optional | 
**offset** | **i64** | The participant's offset. The details of this field are described in ``community/ledger-api/README.md``. Required, must be a valid absolute offset (positive integer). | 
**events** | Option<[**Vec<models::JsReassignmentEvent>**](JsReassignmentEvent.md)> | The collection of reassignment events. Required. | [optional]
**trace_context** | Option<[**models::TraceContext**](TraceContext.md)> |  | [optional]
**record_time** | **String** | The time at which the reassignment was recorded. The record time refers to the source/target synchronizer for an unassign/assign event respectively. Required | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


