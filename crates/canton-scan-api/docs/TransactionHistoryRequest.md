# TransactionHistoryRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**page_end_event_id** | Option<**String**> | Note that all transactions carry some monotonically-increasing event_id. Omit this page_end_event_id to start reading the first page, from the beginning or the end of the ledger, depending on the sort_order column. A subsequent request can fill the page_end_event_id with the last event_id of the TransactionHistoryResponse to continue reading in the same sort_order. The transaction with event_id == page_end_event_id will be skipped in the next response, making it possible to continuously read pages in the same sort_order.  | [optional]
**sort_order** | Option<**String**> | Sort order for the transactions. For ascending order, from beginning to the end of the ledger, use \"asc\". For descending order, from end to beginning of the ledger, use \"desc\". \"asc\" is used if the sort_order is omitted.  | [optional]
**page_size** | **i64** | The maximum number of transactions returned for this request.  | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


