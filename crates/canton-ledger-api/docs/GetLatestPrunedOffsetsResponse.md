# GetLatestPrunedOffsetsResponse

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**participant_pruned_up_to_inclusive** | **i64** | It will always be a non-negative integer. If positive, the absolute offset up to which the ledger has been pruned, disregarding the state of all divulged contracts pruning. If zero, the ledger has not been pruned yet. | 
**all_divulged_contracts_pruned_up_to_inclusive** | **i64** | It will always be a non-negative integer. If positive, the absolute offset up to which all divulged events have been pruned on the ledger. It can be at or before the ``participant_pruned_up_to_inclusive`` offset. For more details about all divulged events pruning, see ``PruneRequest.prune_all_divulged_contracts`` in ``participant_pruning_service.proto``. If zero, the divulged events have not been pruned yet. | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


