# GetUpdatesBeforeRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**migration_id** | **i64** |  | 
**synchronizer_id** | **String** |  | 
**before** | **String** | Only return updates with a record time strictly smaller than this time.  | 
**at_or_after** | Option<**String**> | Only return updates with a record time equal to or greater than this time.  | [optional]
**count** | **i32** | Return at most this many updates. The actual number of updates returned may be smaller.  | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


