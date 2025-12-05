# JsAssignedEvent

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**source** | **String** | The ID of the source synchronizer. Must be a valid synchronizer id. Required | 
**target** | **String** | The ID of the target synchronizer. Must be a valid synchronizer id. Required | 
**unassign_id** | **String** | The ID from the unassigned event. For correlation capabilities. For one contract the (unassign_id, source synchronizer) pair is unique. Must be a valid LedgerString (as described in ``value.proto``). Required | 
**submitter** | **String** | Party on whose behalf the assign command was executed. Empty if the assignment happened offline via the repair service. Must be a valid PartyIdString (as described in ``value.proto``). Optional | 
**reassignment_counter** | **i64** | Each corresponding assigned and unassigned event has the same reassignment_counter. This strictly increases with each unassign command for the same contract. Creation of the contract corresponds to reassignment_counter equals zero. Required | 
**created_event** | [**models::CreatedEvent**](CreatedEvent.md) |  | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


