# JsActiveContract

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**created_event** | [**models::CreatedEvent**](CreatedEvent.md) |  | 
**synchronizer_id** | **String** | A valid synchronizer id Required | 
**reassignment_counter** | **i64** | Each corresponding assigned and unassigned event has the same reassignment_counter. This strictly increases with each unassign command for the same contract. Creation of the contract corresponds to reassignment_counter equals zero. This field will be the reassignment_counter of the latest observable activation event on this synchronizer, which is before the active_at_offset. Required | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


