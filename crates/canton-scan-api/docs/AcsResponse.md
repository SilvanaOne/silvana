# AcsResponse

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**record_time** | **String** | The same `record_time` as in the request. | 
**migration_id** | **i64** | The same `migration_id` as in the request. | 
**created_events** | [**Vec<models::CreatedEvent>**](CreatedEvent.md) | Up to `page_size` contracts in the ACS. `create_arguments` are always encoded as `compact_json`.  | 
**next_page_token** | Option<**i64**> | When requesting the next page of results, pass this as `after` to the `AcsRequest` or `HoldingsStateRequest`. Will be absent when there are no more pages.  | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


