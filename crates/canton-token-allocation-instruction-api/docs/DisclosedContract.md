# DisclosedContract

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**template_id** | **String** |  | 
**contract_id** | **String** |  | 
**created_event_blob** | **String** |  | 
**synchronizer_id** | **String** | The synchronizer to which the contract is currently assigned. If the contract is in the process of being reassigned, then a \"409\" response is returned.  | 
**debug_package_name** | Option<**String**> | The name of the Daml package that was used to create the contract. Use this data only if you trust the provider, as it might not match the data in the `createdEventBlob`.  | [optional]
**debug_payload** | Option<[**serde_json::Value**](.md)> | The contract arguments that were used to create the contract. Use this data only if you trust the provider, as it might not match the data in the `createdEventBlob`.  | [optional]
**debug_created_at** | Option<**String**> | The ledger effective time at which the contract was created. Use this data only if you trust the provider, as it might not match the data in the `createdEventBlob`.  | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


