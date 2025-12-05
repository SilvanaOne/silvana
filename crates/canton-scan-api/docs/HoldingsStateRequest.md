# HoldingsStateRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**migration_id** | **i64** | The migration id for which to return the ACS.  | 
**record_time** | **String** | The timestamp at which the contract set was active.  This needs to be an exact timestamp, i.e., needs to correspond to a timestamp reported by `/v0/state/acs/snapshot-timestamp` if `record_time_match` is set to `exact` (which is the default). If `record_time_match` is set to `at_or_before`, this can be any timestamp, and the most recent snapshot at or before the given `record_time` will be returned.  | 
**record_time_match** | Option<**String**> | How to match the record_time. \"exact\" requires the record_time to match exactly. \"at_or_before\" finds the most recent snapshot at or before the given record_time.  | [optional][default to Exact]
**after** | Option<**i64**> | Pagination token for the next page of results.  | [optional]
**page_size** | **i32** | The maximum number of created events returned for this request.  | 
**owner_party_ids** | **Vec<String>** | Filters by contracts in which these party_ids are the owners of the amulets.  | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


