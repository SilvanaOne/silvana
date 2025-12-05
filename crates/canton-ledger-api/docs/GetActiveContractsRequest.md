# GetActiveContractsRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**filter** | Option<[**models::TransactionFilter**](TransactionFilter.md)> |  | [optional]
**verbose** | **bool** | Provided for backwards compatibility, it will be removed in the Canton version 3.4.0. If enabled, values served over the API will contain more information than strictly necessary to interpret the data. In particular, setting the verbose flag to true triggers the ledger to include labels for record fields. Optional, if specified event_format must be unset. | 
**active_at_offset** | **i64** | The offset at which the snapshot of the active contracts will be computed. Must be no greater than the current ledger end offset. Must be greater than or equal to the last pruning offset. Required, must be a valid absolute offset (positive integer) or ledger begin offset (zero). If zero, the empty set will be returned. | 
**event_format** | Option<[**models::EventFormat**](EventFormat.md)> |  | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


