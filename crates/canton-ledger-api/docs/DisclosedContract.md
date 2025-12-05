# DisclosedContract

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**template_id** | Option<**String**> | The template id of the contract. The identifier uses the package-id reference format.  Required | [optional]
**contract_id** | **String** | The contract id Required | 
**created_event_blob** | **String** | Opaque byte string containing the complete payload required by the Daml engine to reconstruct a contract not known to the receiving participant. Required | 
**synchronizer_id** | **String** | The ID of the synchronizer where the contract is currently assigned Optional | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


