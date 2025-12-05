# GetTransactionByIdRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**update_id** | **String** | The ID of a particular transaction. Must be a valid LedgerString (as described in ``value.proto``). Required | 
**requesting_parties** | Option<**Vec<String>**> | Provided for backwards compatibility, it will be removed in the Canton version 3.4.0. The parties whose events the client expects to see. Events that are not visible for the parties in this collection will not be present in the response. Each element must be a valid PartyIdString (as described in ``value.proto``). Must be set for GetTransactionTreeById request. Optional for backwards compatibility for GetTransactionById request: if defined transaction_format must be unset (falling back to defaults). | [optional]
**transaction_format** | Option<[**models::TransactionFormat**](TransactionFormat.md)> |  | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


