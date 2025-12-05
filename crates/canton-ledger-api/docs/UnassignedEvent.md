# UnassignedEvent

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**unassign_id** | **String** | The ID of the unassignment. This needs to be used as an input for a assign ReassignmentCommand. For one contract the (unassign_id, source synchronizer) pair is unique. Must be a valid LedgerString (as described in ``value.proto``). Required | 
**contract_id** | **String** | The ID of the reassigned contract. Must be a valid LedgerString (as described in ``value.proto``). Required | 
**template_id** | Option<**String**> | The template of the reassigned contract. The identifier uses the package-id reference format.  Required | [optional]
**source** | **String** | The ID of the source synchronizer Must be a valid synchronizer id Required | 
**target** | **String** | The ID of the target synchronizer Must be a valid synchronizer id Required | 
**submitter** | **String** | Party on whose behalf the unassign command was executed. Empty if the unassignment happened offline via the repair service. Must be a valid PartyIdString (as described in ``value.proto``). Optional | 
**reassignment_counter** | **i64** | Each corresponding assigned and unassigned event has the same reassignment_counter. This strictly increases with each unassign command for the same contract. Creation of the contract corresponds to reassignment_counter equals zero. Required | 
**assignment_exclusivity** | Option<**String**> | Assignment exclusivity Before this time (measured on the target synchronizer), only the submitter of the unassignment can initiate the assignment Defined for reassigning participants. Optional | [optional]
**witness_parties** | Option<**Vec<String>**> | The parties that are notified of this event. Required | [optional]
**package_name** | **String** | The package name of the contract. Required | 
**offset** | **i64** | The offset of origin. Offsets are managed by the participant nodes. Reassignments can thus NOT be assumed to have the same offsets on different participant nodes. Required, it is a valid absolute offset (positive integer) | 
**node_id** | **i32** | The position of this event in the originating reassignment. Node IDs are not necessarily equal across participants, as these may see different projections/parts of reassignments. Required, must be valid node ID (non-negative integer) | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


