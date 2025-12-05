# GetUpdatesRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**begin_exclusive** | **i64** | Beginning of the requested ledger section (non-negative integer). The response will only contain transactions whose offset is strictly greater than this. If zero, the stream will start from the beginning of the ledger. If positive, the streaming will start after this absolute offset. If the ledger has been pruned, this parameter must be specified and be greater than the pruning offset. | 
**end_inclusive** | Option<**i64**> | End of the requested ledger section. The response will only contain transactions whose offset is less than or equal to this. Optional, if empty, the stream will not terminate. If specified, the stream will terminate after this absolute offset (positive integer) is reached. | [optional]
**filter** | Option<[**models::TransactionFilter**](TransactionFilter.md)> |  | [optional]
**verbose** | **bool** | Provided for backwards compatibility, it will be removed in the Canton version 3.4.0. If enabled, values served over the API will contain more information than strictly necessary to interpret the data. In particular, setting the verbose flag to true triggers the ledger to include labels, record and variant type ids for record fields. Optional for backwards compatibility, if defined update_format must be unset | 
**update_format** | Option<[**models::UpdateFormat**](UpdateFormat.md)> |  | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


