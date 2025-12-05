# CreatedEvent

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**event_type** | **String** |  | 
**event_id** | **String** | The ID of this particular event. Equal to the key of this element of the containing `events_by_id` if this is part of a `TreeEvent`.  | 
**contract_id** | **String** | The ID of the created contract.  | 
**template_id** | **String** | The template of the created contract.  | 
**package_name** | **String** | The package name of the created contract.  | 
**create_arguments** | [**serde_json::Value**](.md) | The arguments that have been used to create the contract, in the form of JSON representation of a Daml record.  | 
**created_at** | **String** | Ledger effective time of the transaction that created the contract.  | 
**signatories** | **Vec<String>** | Signatories to the contract, in the form of party IDs.  | 
**observers** | **Vec<String>** | Observers to the contract, in the form of party IDs.  | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


