# ExercisedEvent

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**event_type** | **String** |  | 
**event_id** | **String** | The ID of this particular event. Equal to the key of this element of the containing `events_by_id` if this is part of a `TreeEvent`.  | 
**contract_id** | **String** | The ID of the created contract.  | 
**template_id** | **String** | The template of the created contract.  | 
**package_name** | **String** | The package name of the created contract.  | 
**choice** | **String** | The choice that was exercised on the target contract, as an unqualified choice name, i.e. with no package or module name qualifiers.  | 
**choice_argument** | [**serde_json::Value**](.md) | The argument of the exercised choice, in the form of JSON representation of a Daml value. This is usually a record with field names being the argument names, even in the case of a single apparent choice argument, which is represented as a single-element Daml record.  | 
**child_event_ids** | **Vec<String>** | References to further events in the same transaction that appeared as a result of this ExercisedEvent. It contains only the immediate children of this event, not all members of the subtree rooted at this node. The order of the children is the same as the event order in the transaction.  | 
**exercise_result** | [**serde_json::Value**](.md) | The result of exercising the choice, as the JSON representation of a Daml value.  | 
**consuming** | **bool** | If true, the target contract may no longer be exercised.  | 
**acting_parties** | **Vec<String>** | The parties that exercised the choice, in the form of party IDs.  | 
**interface_id** | Option<**String**> | The interface where the choice is defined, if inherited.  | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


