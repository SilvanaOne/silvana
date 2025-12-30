# CostEstimationHints

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**disabled** | **bool** | Disable cost estimation Default (not set) is false | 
**expected_signatures** | Option<**Vec<String>**> | Details on the keys that will be used to sign the transaction (how many and of which type). Signature size impacts the cost of the transaction. If empty, the signature sizes will be approximated with threshold-many signatures (where threshold is defined in the PartyToKeyMapping of the external party), using keys in the order they are registered. Optional (empty list is equivalent to not providing this field) | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


