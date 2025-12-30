# DisclosedContract

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**template_id** | Option<**String**> | The template id of the contract. The identifier uses the package-id reference format.  If provided, used to validate the template id of the contract serialized in the created_event_blob. Optional | [optional]
**contract_id** | **String** | The contract id  If provided, used to validate the contract id of the contract serialized in the created_event_blob. Optional | 
**created_event_blob** | **String** | Opaque byte string containing the complete payload required by the Daml engine to reconstruct a contract not known to the receiving participant. Required | 
**synchronizer_id** | **String** | The ID of the synchronizer where the contract is currently assigned Optional | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


