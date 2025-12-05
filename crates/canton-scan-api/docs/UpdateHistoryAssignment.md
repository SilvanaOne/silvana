# UpdateHistoryAssignment

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**submitter** | **String** | The party ID who submitted this reassignment  | 
**source_synchronizer** | **String** | The id of the synchronizer from which the contract was reassigned  | 
**target_synchronizer** | **String** | The id of the synchronizer to which the contract was reassigned  | 
**migration_id** | **i64** | The migration id of the target synchronizer  | 
**unassign_id** | **String** | The id of the corresponding unassign event; this assignment will usually, but not always, occur after the so-identified unassignment event.  | 
**created_event** | [**models::CreatedEvent**](CreatedEvent.md) |  | 
**reassignment_counter** | **i64** | Each corresponding assigned and unassigned event has the same reassignment_counter. This strictly increases with each unassign command for the same contract. Creation of the contract corresponds to reassignment_counter 0. | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


