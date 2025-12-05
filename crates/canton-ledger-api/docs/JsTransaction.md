# JsTransaction

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**update_id** | **String** | Assigned by the server. Useful for correlating logs. Must be a valid LedgerString (as described in ``value.proto``). Required | 
**command_id** | **String** | The ID of the command which resulted in this transaction. Missing for everyone except the submitting party. Must be a valid LedgerString (as described in ``value.proto``). Optional | 
**workflow_id** | **String** | The workflow ID used in command submission. Must be a valid LedgerString (as described in ``value.proto``). Optional | 
**effective_at** | **String** | Ledger effective time. Required | 
**events** | Option<[**Vec<models::Event>**](Event.md)> | The collection of events. Contains:  - ``CreatedEvent`` or ``ArchivedEvent`` in case of ACS_DELTA transaction shape - ``CreatedEvent`` or ``ExercisedEvent`` in case of LEDGER_EFFECTS transaction shape  Required | [optional]
**offset** | **i64** | The absolute offset. The details of this field are described in ``community/ledger-api/README.md``. Required, it is a valid absolute offset (positive integer). | 
**synchronizer_id** | **String** | A valid synchronizer id. Identifies the synchronizer that synchronized the transaction. Required | 
**trace_context** | Option<[**models::TraceContext**](TraceContext.md)> |  | [optional]
**record_time** | **String** | The time at which the transaction was recorded. The record time refers to the synchronizer which synchronized the transaction. Required | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


