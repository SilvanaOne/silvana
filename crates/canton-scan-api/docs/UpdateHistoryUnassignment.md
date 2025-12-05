# UpdateHistoryUnassignment

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**submitter** | **String** | The party who submitted this reassignment  | 
**source_synchronizer** | **String** | The id of the synchronizer from which the contract was reassigned  | 
**migration_id** | **i64** | The migration id of the synchronizer from which the contract was reassigned  | 
**target_synchronizer** | **String** | The id of the synchronizer to which the contract was reassigned  | 
**unassign_id** | **String** | The id of the unassign event, to later be correlated to an assign event  | 
**reassignment_counter** | **i64** | Each corresponding assigned and unassigned event has the same reassignment_counter. This strictly increases with each unassign command for the same contract. Creation of the contract corresponds to reassignment_counter 0. | 
**contract_id** | **String** | The id of the unassigned contract  | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


