# GetBackfillingStatusResponse

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**complete** | **bool** | True if ALL backfilling processes are complete, false otherwise.  Some scan endpoints return error responses if backfilling is not complete (e.g., `/v1/updates`), others return partial results (e.g., `/v0/transactions`). This endpoint is a simple indicator for whether historical information may be incomplete.  To determine the progress of individual backfilling processes, inspect the corresponding metrics.  | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


